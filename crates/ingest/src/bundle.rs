//! HCF (High-Compression Format) bundle creation.
//!
//! Packs normalized WebP images into fixed-size shard files for efficient
//! HTTP Range request delivery.
//!
//! Bundle structure:
//! - Each shard is a configurable size (default 250MB)
//! - Images are concatenated with no internal structure
//! - Token entries in collection.bin store offset/length for each image
//! - Client fetches individual images via HTTP Range requests

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use thiserror::Error;

/// HCF bundling errors.
#[derive(Debug, Error)]
pub enum HcfError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Image file too large: {size} bytes (max {max})")]
    ImageTooLarge { size: u64, max: u64 },

    #[error("No images provided")]
    NoImages,
}

/// Configuration for HCF bundle creation.
#[derive(Debug, Clone)]
pub struct HcfConfig {
    /// Target shard size in bytes
    pub shard_size: usize,
    /// Maximum allowed image size
    pub max_image_size: u32,
}

impl Default for HcfConfig {
    fn default() -> Self {
        Self {
            shard_size: 250 * 1024 * 1024,    // 250 MB
            max_image_size: 16 * 1024 * 1024, // 16 MB
        }
    }
}

/// Location of an image within HCF bundles.
#[derive(Debug, Clone, Copy)]
pub struct ImageLocation {
    /// Global offset from start of concatenated shards
    pub global_offset: u64,
    /// Length of image data in bytes
    pub length: u32,
    /// Shard index (computed from global_offset / shard_size)
    pub shard_index: u32,
    /// Offset within the shard
    pub shard_offset: u32,
}

/// Information about a generated HCF shard.
#[derive(Debug, Clone)]
pub struct ShardInfo {
    /// Shard index
    pub index: u32,
    /// Path to shard file
    pub path: std::path::PathBuf,
    /// Number of images in this shard
    pub image_count: u32,
    /// Total size in bytes
    pub size: u64,
}

/// Result of HCF bundling.
#[derive(Debug)]
pub struct HcfBundleResult {
    /// List of generated shards
    pub shards: Vec<ShardInfo>,
    /// Location of each image (in same order as input)
    pub locations: Vec<ImageLocation>,
    /// Total size of all shards
    pub total_size: u64,
    /// Largest image size
    pub max_image_size: u32,
}

/// HCF bundle builder.
pub struct HcfBundler {
    config: HcfConfig,
    output_dir: std::path::PathBuf,
    current_writer: Option<BufWriter<File>>,
    current_shard: u32,
    current_shard_size: u64,
    current_shard_images: u32,
    global_offset: u64,
    locations: Vec<ImageLocation>,
    shards: Vec<ShardInfo>,
    max_image_size: u32,
}

impl HcfBundler {
    /// Create a new HCF bundler.
    pub fn new(config: HcfConfig, output_dir: impl AsRef<Path>) -> Self {
        Self {
            config,
            output_dir: output_dir.as_ref().to_path_buf(),
            current_writer: None,
            current_shard: 0,
            current_shard_size: 0,
            current_shard_images: 0,
            global_offset: 0,
            locations: Vec::new(),
            shards: Vec::new(),
            max_image_size: 0,
        }
    }

    /// Get path for a shard file.
    fn shard_path(&self, index: u32) -> std::path::PathBuf {
        self.output_dir.join(format!("images_{:03}.hcf", index))
    }

    /// Start a new shard.
    fn start_shard(&mut self) -> Result<(), HcfError> {
        // Finalize current shard if exists
        self.finalize_shard()?;

        let path = self.shard_path(self.current_shard);
        let file = File::create(&path)?;
        self.current_writer = Some(BufWriter::new(file));
        self.current_shard_size = 0;
        self.current_shard_images = 0;

        Ok(())
    }

    /// Finalize the current shard.
    fn finalize_shard(&mut self) -> Result<(), HcfError> {
        if let Some(writer) = self.current_writer.take() {
            drop(writer); // Ensure all data is flushed

            if self.current_shard_images > 0 {
                self.shards.push(ShardInfo {
                    index: self.current_shard,
                    path: self.shard_path(self.current_shard),
                    image_count: self.current_shard_images,
                    size: self.current_shard_size,
                });
                self.current_shard += 1;
            }
        }
        Ok(())
    }

    /// Add an image to the bundle.
    pub fn add_image(&mut self, image_path: &Path) -> Result<ImageLocation, HcfError> {
        // Read image data
        let data = std::fs::read(image_path)?;
        let size = data.len() as u64;

        // Check size limit
        if size > self.config.max_image_size as u64 {
            return Err(HcfError::ImageTooLarge {
                size,
                max: self.config.max_image_size as u64,
            });
        }

        // Update max image size
        self.max_image_size = self.max_image_size.max(data.len() as u32);

        // Check if we need a new shard
        let would_exceed = self.current_shard_size + size > self.config.shard_size as u64;
        if self.current_writer.is_none() || would_exceed {
            self.start_shard()?;
        }

        // Record location
        let location = ImageLocation {
            global_offset: self.global_offset,
            length: data.len() as u32,
            shard_index: self.current_shard,
            shard_offset: self.current_shard_size as u32,
        };

        // Write data
        if let Some(ref mut writer) = self.current_writer {
            writer.write_all(&data)?;
        }

        // Update state
        self.current_shard_size += size;
        self.current_shard_images += 1;
        self.global_offset += size;
        self.locations.push(location);

        Ok(location)
    }

    /// Finish bundling and return results.
    pub fn finish(mut self) -> Result<HcfBundleResult, HcfError> {
        self.finalize_shard()?;

        Ok(HcfBundleResult {
            total_size: self.global_offset,
            max_image_size: self.max_image_size,
            shards: self.shards,
            locations: self.locations,
        })
    }

    /// Bundle a batch of images.
    pub fn bundle_batch<F>(
        config: HcfConfig,
        images: &[impl AsRef<Path>],
        output_dir: &Path,
        mut on_progress: F,
    ) -> Result<HcfBundleResult, HcfError>
    where
        F: FnMut(usize, usize),
    {
        if images.is_empty() {
            return Err(HcfError::NoImages);
        }

        let mut bundler = Self::new(config, output_dir);
        let total = images.len();

        for (i, image) in images.iter().enumerate() {
            bundler.add_image(image.as_ref())?;
            on_progress(i + 1, total);
        }

        bundler.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_hcf_config_defaults() {
        let config = HcfConfig::default();
        assert_eq!(config.shard_size, 250 * 1024 * 1024);
        assert_eq!(config.max_image_size, 16 * 1024 * 1024);
    }

    #[test]
    fn test_hcf_bundling() {
        let dir = tempdir().unwrap();
        let output_dir = dir.path().join("hcf");
        std::fs::create_dir_all(&output_dir).unwrap();

        // Create some test "images" (just byte data)
        let input_dir = dir.path().join("images");
        std::fs::create_dir_all(&input_dir).unwrap();

        let mut image_paths = Vec::new();
        for i in 0..5 {
            let path = input_dir.join(format!("img_{}.webp", i));
            let mut f = File::create(&path).unwrap();
            // Write some data (varying sizes)
            let data = vec![i as u8; 1000 + i * 100];
            f.write_all(&data).unwrap();
            image_paths.push(path);
        }

        // Bundle with tiny shard size for testing
        let config = HcfConfig {
            shard_size: 2000, // Small for testing
            max_image_size: 10000,
        };

        let result =
            HcfBundler::bundle_batch(config, &image_paths, &output_dir, |_, _| {}).unwrap();

        assert_eq!(result.locations.len(), 5);
        assert!(result.shards.len() >= 1);

        // Verify first location
        assert_eq!(result.locations[0].global_offset, 0);
        assert_eq!(result.locations[0].shard_index, 0);
        assert_eq!(result.locations[0].shard_offset, 0);
        assert_eq!(result.locations[0].length, 1000);

        // Verify total size
        let expected_total: u64 = (0..5).map(|i| 1000 + i * 100).sum::<usize>() as u64;
        assert_eq!(result.total_size, expected_total);
    }
}
