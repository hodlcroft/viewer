//! Sprite sheet generation.
//!
//! Generates sprite sheets from source images for fast thumbnail display.
//! Sprites are created from raw source images (faster than decoding WebP).
//!
//! Each sprite sheet is a grid of thumbnail images, typically 10x10 = 100 images
//! per sheet. The sheets are saved as WebP for efficient delivery.

use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, RgbaImage};
use thiserror::Error;

/// Sprite generation errors.
#[derive(Debug, Error)]
pub enum SpriteError {
    #[error("Failed to open image: {0}")]
    Open(#[from] std::io::Error),

    #[error("Failed to decode image: {0}")]
    Decode(#[from] image::ImageError),

    #[error("No images provided")]
    NoImages,
}

/// Configuration for sprite generation.
#[derive(Debug, Clone)]
pub struct SpriteConfig {
    /// Thumbnail size in pixels (square)
    pub thumb_size: u32,
    /// Grid size (NxN thumbnails per sheet)
    pub grid_size: u32,
    /// Background color (RGBA)
    pub background: [u8; 4],
}

impl Default for SpriteConfig {
    fn default() -> Self {
        Self {
            thumb_size: 150,
            grid_size: 10,
            background: [0, 0, 0, 0], // Transparent
        }
    }
}

impl SpriteConfig {
    /// Number of thumbnails per sheet.
    pub fn thumbs_per_sheet(&self) -> u32 {
        self.grid_size * self.grid_size
    }

    /// Sheet dimensions in pixels.
    pub fn sheet_dimensions(&self) -> (u32, u32) {
        let size = self.grid_size * self.thumb_size;
        (size, size)
    }
}

/// Information about a generated sprite sheet.
#[derive(Debug, Clone)]
pub struct SpriteSheet {
    /// Sheet index
    pub index: u32,
    /// Number of thumbnails in this sheet
    pub count: u32,
    /// Output file path
    pub path: std::path::PathBuf,
    /// File size in bytes
    pub file_size: u64,
}

/// Result of sprite generation for a single image.
#[derive(Debug, Clone)]
pub struct SpriteLocation {
    /// Sheet index
    pub sheet: u16,
    /// X position in grid (0-based)
    pub x: u8,
    /// Y position in grid (0-based)
    pub y: u8,
}

/// Sprite sheet generator.
pub struct SpriteGenerator {
    config: SpriteConfig,
    current_sheet: Option<RgbaImage>,
    current_index: u32,
    current_count: u32,
    sheets: Vec<SpriteSheet>,
}

impl SpriteGenerator {
    pub fn new(config: SpriteConfig) -> Self {
        Self {
            config,
            current_sheet: None,
            current_index: 0,
            current_count: 0,
            sheets: Vec::new(),
        }
    }

    /// Add an image to the sprite sheets.
    ///
    /// Returns the location where the thumbnail was placed.
    pub fn add_image(&mut self, source: &Path) -> Result<SpriteLocation, SpriteError> {
        // Load and resize image
        let img = ImageReader::open(source)?.decode()?;
        let thumb = resize_to_thumb(&img, self.config.thumb_size);

        // Ensure we have a current sheet
        if self.current_sheet.is_none() {
            self.start_new_sheet();
        }

        // Calculate position in grid
        let pos = self.current_count;
        let x = pos % self.config.grid_size;
        let y = pos / self.config.grid_size;

        // Place thumbnail on sheet
        let sheet = self.current_sheet.as_mut().unwrap();
        let px = x * self.config.thumb_size;
        let py = y * self.config.thumb_size;

        // Copy thumbnail pixels
        for (dx, dy, pixel) in thumb.enumerate_pixels() {
            sheet.put_pixel(px + dx, py + dy, *pixel);
        }

        let location = SpriteLocation {
            sheet: self.current_index as u16,
            x: x as u8,
            y: y as u8,
        };

        self.current_count += 1;

        // Check if sheet is full
        if self.current_count >= self.config.thumbs_per_sheet() {
            // Sheet will be finalized when we start a new one or finish
        }

        Ok(location)
    }

    /// Start a new sprite sheet.
    fn start_new_sheet(&mut self) {
        let (width, height) = self.config.sheet_dimensions();
        let mut sheet = RgbaImage::new(width, height);

        // Fill with background color
        let bg = image::Rgba(self.config.background);
        for pixel in sheet.pixels_mut() {
            *pixel = bg;
        }

        self.current_sheet = Some(sheet);
        self.current_count = 0;
    }

    /// Finalize current sheet and save to disk.
    fn finalize_sheet(&mut self, output_dir: &Path) -> Result<(), SpriteError> {
        if let Some(sheet) = self.current_sheet.take() {
            if self.current_count > 0 {
                let path = output_dir.join(format!("sprites_{:03}.webp", self.current_index));

                // Save as WebP
                sheet.save(&path)?;

                let file_size = std::fs::metadata(&path)?.len();

                self.sheets.push(SpriteSheet {
                    index: self.current_index,
                    count: self.current_count,
                    path,
                    file_size,
                });

                self.current_index += 1;
            }
        }
        Ok(())
    }

    /// Finish sprite generation and save any remaining sheets.
    pub fn finish(mut self, output_dir: &Path) -> Result<Vec<SpriteSheet>, SpriteError> {
        self.finalize_sheet(output_dir)?;
        Ok(self.sheets)
    }

    /// Generate sprite sheets from a list of source images.
    ///
    /// Returns the list of generated sheets and sprite locations for each input image.
    pub fn generate_batch<F>(
        config: SpriteConfig,
        sources: &[impl AsRef<Path>],
        output_dir: &Path,
        mut on_progress: F,
    ) -> Result<(Vec<SpriteSheet>, Vec<SpriteLocation>), SpriteError>
    where
        F: FnMut(usize, usize),
    {
        if sources.is_empty() {
            return Err(SpriteError::NoImages);
        }

        let mut generator = Self::new(config);
        let mut locations = Vec::with_capacity(sources.len());
        let total = sources.len();

        for (i, source) in sources.iter().enumerate() {
            // Check if we need to start a new sheet
            if generator.current_count >= generator.config.thumbs_per_sheet() {
                generator.finalize_sheet(output_dir)?;
                generator.start_new_sheet();
            }

            let location = generator.add_image(source.as_ref())?;
            locations.push(location);

            on_progress(i + 1, total);
        }

        let sheets = generator.finish(output_dir)?;
        Ok((sheets, locations))
    }
}

/// Resize image to a square thumbnail.
///
/// The image is scaled to fit within the thumbnail size while maintaining
/// aspect ratio, then centered on a transparent background.
fn resize_to_thumb(img: &DynamicImage, size: u32) -> RgbaImage {
    let (w, h) = (img.width(), img.height());

    // Calculate scale to fit within size
    let scale = (size as f32) / (w.max(h) as f32);
    let new_w = ((w as f32) * scale).round() as u32;
    let new_h = ((h as f32) * scale).round() as u32;

    // Resize with high-quality filter
    let resized = img.resize_exact(new_w, new_h, FilterType::Lanczos3);

    // Create square canvas and center the image
    let mut canvas = RgbaImage::new(size, size);
    let offset_x = (size - new_w) / 2;
    let offset_y = (size - new_h) / 2;

    // Copy resized image to canvas
    let rgba = resized.to_rgba8();
    for (x, y, pixel) in rgba.enumerate_pixels() {
        canvas.put_pixel(offset_x + x, offset_y + y, *pixel);
    }

    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_config_defaults() {
        let config = SpriteConfig::default();
        assert_eq!(config.thumb_size, 150);
        assert_eq!(config.grid_size, 10);
        assert_eq!(config.thumbs_per_sheet(), 100);
        assert_eq!(config.sheet_dimensions(), (1500, 1500));
    }

    #[test]
    fn test_sprite_config_custom() {
        let config = SpriteConfig {
            thumb_size: 100,
            grid_size: 8,
            background: [255, 255, 255, 255],
        };
        assert_eq!(config.thumbs_per_sheet(), 64);
        assert_eq!(config.sheet_dimensions(), (800, 800));
    }
}
