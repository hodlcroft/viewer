//! Image normalization - resize and convert to WebP.
//!
//! All images are normalized to:
//! - WebP format for consistent handling
//! - Max 2048px on longest edge (no upscaling)
//! - Configurable quality level

use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};
use thiserror::Error;

/// Image normalization errors.
#[derive(Debug, Error)]
pub enum NormalizeError {
    #[error("Failed to open image: {0}")]
    Open(#[from] std::io::Error),

    #[error("Failed to decode image: {0}")]
    Decode(#[from] image::ImageError),

    #[error("Unsupported image format")]
    UnsupportedFormat,
}

/// Configuration for image normalization.
#[derive(Debug, Clone)]
pub struct NormalizeConfig {
    /// Maximum dimension for longest edge
    pub max_dimension: u32,
    /// WebP quality (0-100)
    pub quality: u8,
}

impl Default for NormalizeConfig {
    fn default() -> Self {
        Self {
            max_dimension: 2048,
            quality: 85,
        }
    }
}

/// Result of normalizing an image.
#[derive(Debug)]
pub struct NormalizeResult {
    /// Original dimensions
    pub original_width: u32,
    pub original_height: u32,
    /// Final dimensions
    pub final_width: u32,
    pub final_height: u32,
    /// Whether the image was resized
    pub resized: bool,
    /// Output file size in bytes
    pub file_size: u64,
}

/// Normalize a single image.
///
/// Reads the source image, resizes if necessary (no upscaling),
/// and saves as WebP to the output path.
pub fn normalize_image(
    source: &Path,
    output: &Path,
    config: &NormalizeConfig,
) -> Result<NormalizeResult, NormalizeError> {
    // Read source image
    let img = ImageReader::open(source)?.decode()?;

    let original_width = img.width();
    let original_height = img.height();

    // Calculate target dimensions (maintain aspect ratio, no upscaling)
    let (final_width, final_height, resized) =
        calculate_dimensions(original_width, original_height, config.max_dimension);

    // Resize if needed
    let final_img = if resized {
        img.resize(final_width, final_height, FilterType::Lanczos3)
    } else {
        img
    };

    // Encode as WebP
    let webp_data = encode_webp(&final_img, config.quality)?;

    // Write to output
    std::fs::write(output, &webp_data)?;

    Ok(NormalizeResult {
        original_width,
        original_height,
        final_width,
        final_height,
        resized,
        file_size: webp_data.len() as u64,
    })
}

/// Calculate target dimensions while maintaining aspect ratio.
///
/// Returns (width, height, was_resized).
fn calculate_dimensions(width: u32, height: u32, max_dimension: u32) -> (u32, u32, bool) {
    let longest = width.max(height);

    if longest <= max_dimension {
        // No resize needed
        (width, height, false)
    } else {
        // Scale down to fit within max_dimension
        let scale = max_dimension as f32 / longest as f32;
        let new_width = (width as f32 * scale).round() as u32;
        let new_height = (height as f32 * scale).round() as u32;
        (new_width, new_height, true)
    }
}

/// Encode image as WebP.
fn encode_webp(img: &DynamicImage, _quality: u8) -> Result<Vec<u8>, NormalizeError> {
    use std::io::Cursor;

    let mut buffer = Cursor::new(Vec::new());

    // The image crate's WebP encoder doesn't expose quality directly,
    // it uses a reasonable default. For finer control we'd need webp crate.
    img.write_to(&mut buffer, image::ImageFormat::WebP)?;

    Ok(buffer.into_inner())
}

/// Batch normalize images with progress tracking.
pub struct BatchNormalizer {
    config: NormalizeConfig,
}

impl BatchNormalizer {
    pub fn new(config: NormalizeConfig) -> Self {
        Self { config }
    }

    /// Normalize a batch of images.
    ///
    /// Takes a list of (source_path, output_path) pairs.
    /// Returns the number of successfully normalized images.
    pub fn normalize_batch<F>(
        &self,
        images: &[(impl AsRef<Path>, impl AsRef<Path>)],
        mut on_progress: F,
    ) -> Vec<Result<NormalizeResult, NormalizeError>>
    where
        F: FnMut(usize, usize, Option<&NormalizeResult>),
    {
        let total = images.len();
        let mut results = Vec::with_capacity(total);

        for (i, (source, output)) in images.iter().enumerate() {
            let result = normalize_image(source.as_ref(), output.as_ref(), &self.config);

            // Call progress callback
            match &result {
                Ok(r) => on_progress(i + 1, total, Some(r)),
                Err(_) => on_progress(i + 1, total, None),
            }

            results.push(result);
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_dimensions_no_resize() {
        let (w, h, resized) = calculate_dimensions(1000, 500, 2048);
        assert_eq!(w, 1000);
        assert_eq!(h, 500);
        assert!(!resized);
    }

    #[test]
    fn test_calculate_dimensions_resize_landscape() {
        let (w, h, resized) = calculate_dimensions(4096, 2048, 2048);
        assert_eq!(w, 2048);
        assert_eq!(h, 1024);
        assert!(resized);
    }

    #[test]
    fn test_calculate_dimensions_resize_portrait() {
        let (w, h, resized) = calculate_dimensions(2000, 4000, 2048);
        assert_eq!(w, 1024);
        assert_eq!(h, 2048);
        assert!(resized);
    }

    #[test]
    fn test_calculate_dimensions_exact_max() {
        let (w, h, resized) = calculate_dimensions(2048, 2048, 2048);
        assert_eq!(w, 2048);
        assert_eq!(h, 2048);
        assert!(!resized);
    }
}
