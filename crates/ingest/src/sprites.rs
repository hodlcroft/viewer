//! Sprite sheet generation.
//!
//! Generates sprite sheets from source images for fast thumbnail display.
//! Sprites are created from raw source images (faster than decoding WebP).
//!
//! Each sprite sheet is a grid of thumbnail images, typically 10x10 = 100 images
//! per sheet. The sheets are saved as WebP for efficient delivery.

use std::path::Path;
use std::time::Instant;

use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, RgbaImage};
use thiserror::Error;
use tracing::info;

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
    /// Thumbnail cell width in pixels
    pub thumb_width: u32,
    /// Thumbnail cell height in pixels
    pub thumb_height: u32,
    /// Grid columns per sheet
    pub grid_columns: u32,
    /// Grid rows per sheet
    pub grid_rows: u32,
    /// Background color (RGBA)
    pub background: [u8; 4],
    /// Maximum sheet dimension (default 2048)
    pub max_sheet_size: u32,
}

impl Default for SpriteConfig {
    fn default() -> Self {
        Self {
            thumb_width: 256,
            thumb_height: 256,
            grid_columns: 4,
            grid_rows: 4,
            background: [0, 0, 0, 0], // Transparent
            max_sheet_size: 1024,
        }
    }
}

impl SpriteConfig {
    /// Create config for a specific aspect ratio.
    ///
    /// Calculates optimal cell size and grid dimensions to fit within max_sheet_size.
    pub fn for_aspect_ratio(width: u32, height: u32, max_sheet_size: u32) -> Self {
        // Determine the larger dimension and scale to fit max cell size
        let max_cell = 256u32;
        let (thumb_width, thumb_height) = if width >= height {
            // Landscape or square
            let scale = max_cell as f32 / width as f32;
            (max_cell, ((height as f32 * scale).round() as u32).max(1))
        } else {
            // Portrait
            let scale = max_cell as f32 / height as f32;
            (((width as f32 * scale).round() as u32).max(1), max_cell)
        };

        // Calculate grid size to fit within max_sheet_size
        let grid_columns = max_sheet_size / thumb_width;
        let grid_rows = max_sheet_size / thumb_height;

        Self {
            thumb_width,
            thumb_height,
            grid_columns,
            grid_rows,
            background: [0, 0, 0, 0],
            max_sheet_size,
        }
    }

    /// Number of thumbnails per sheet.
    pub fn thumbs_per_sheet(&self) -> u32 {
        self.grid_columns * self.grid_rows
    }

    /// Sheet dimensions in pixels.
    pub fn sheet_dimensions(&self) -> (u32, u32) {
        (
            self.grid_columns * self.thumb_width,
            self.grid_rows * self.thumb_height,
        )
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
        let thumb = resize_to_thumb(&img, self.config.thumb_width, self.config.thumb_height);

        // Ensure we have a current sheet
        if self.current_sheet.is_none() {
            self.start_new_sheet();
        }

        // Calculate position in grid
        let pos = self.current_count;
        let col = pos % self.config.grid_columns;
        let row = pos / self.config.grid_columns;

        // Place thumbnail on sheet
        let sheet = self.current_sheet.as_mut().unwrap();
        let px = col * self.config.thumb_width;
        let py = row * self.config.thumb_height;

        // Copy thumbnail pixels
        for (dx, dy, pixel) in thumb.enumerate_pixels() {
            sheet.put_pixel(px + dx, py + dy, *pixel);
        }

        let location = SpriteLocation {
            sheet: self.current_index as u16,
            x: col as u8,
            y: row as u8,
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
                let path = output_dir.join(format!("{:04}.webp", self.current_index));

                // Save as lossy WebP
                let save_start = Instant::now();
                let (width, height) = (sheet.width(), sheet.height());
                let encoder = webp::Encoder::from_rgba(&sheet, width, height);
                let webp_data = encoder.encode(85.0); // Quality 85
                std::fs::write(&path, &*webp_data)?;
                let save_time = save_start.elapsed();

                let file_size = std::fs::metadata(&path)?.len();

                info!(
                    sheet_index = self.current_index,
                    thumb_count = self.current_count,
                    file_size_kb = file_size / 1024,
                    save_ms = save_time.as_millis(),
                    path = %path.display(),
                    "Saved sprite sheet"
                );

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

    /// Generate sprite sheets from a list of source images using parallel processing.
    ///
    /// Auto-detects aspect ratio from the first image and configures the grid accordingly.
    /// Returns the list of generated sheets, sprite locations, and the detected config.
    pub fn generate_batch_auto<F>(
        max_sheet_size: u32,
        sources: &[impl AsRef<Path> + Sync],
        output_dir: &Path,
        on_progress: F,
    ) -> Result<(Vec<SpriteSheet>, Vec<SpriteLocation>, SpriteConfig), SpriteError>
    where
        F: FnMut(usize, usize),
    {
        if sources.is_empty() {
            return Err(SpriteError::NoImages);
        }

        // Detect aspect ratio from first image
        let first_img = ImageReader::open(sources[0].as_ref())
            .map_err(SpriteError::Open)?
            .decode()
            .map_err(SpriteError::Decode)?;

        let config =
            SpriteConfig::for_aspect_ratio(first_img.width(), first_img.height(), max_sheet_size);

        info!(
            detected_aspect = format!("{}x{}", first_img.width(), first_img.height()),
            thumb_size = format!("{}x{}", config.thumb_width, config.thumb_height),
            grid = format!("{}x{}", config.grid_columns, config.grid_rows),
            "Auto-detected sprite config from first image"
        );

        let (sheets, locations) =
            Self::generate_batch(config.clone(), sources, output_dir, on_progress)?;
        Ok((sheets, locations, config))
    }

    /// Generate sprite sheets from a list of source images using parallel processing.
    ///
    /// Returns the list of generated sheets and sprite locations for each input image.
    pub fn generate_batch<F>(
        config: SpriteConfig,
        sources: &[impl AsRef<Path> + Sync],
        output_dir: &Path,
        mut on_progress: F,
    ) -> Result<(Vec<SpriteSheet>, Vec<SpriteLocation>), SpriteError>
    where
        F: FnMut(usize, usize),
    {
        use rayon::prelude::*;

        if sources.is_empty() {
            return Err(SpriteError::NoImages);
        }

        let batch_start = Instant::now();
        let images_per_sheet = config.thumbs_per_sheet() as usize;
        let total_sheets = sources.len().div_ceil(images_per_sheet);

        info!(
            total_images = sources.len(),
            thumb_width = config.thumb_width,
            thumb_height = config.thumb_height,
            grid = format!("{}x{}", config.grid_columns, config.grid_rows),
            images_per_sheet = images_per_sheet,
            total_sheets = total_sheets,
            output_dir = %output_dir.display(),
            "Starting parallel sprite generation"
        );

        let mut sheets = Vec::with_capacity(total_sheets);
        let mut locations = Vec::with_capacity(sources.len());
        let total = sources.len();

        // Process one sheet at a time, but parallelize image loading/resizing within each sheet
        for sheet_idx in 0..total_sheets {
            let sheet_start = Instant::now();
            let start = sheet_idx * images_per_sheet;
            let end = (start + images_per_sheet).min(sources.len());
            let sheet_sources = &sources[start..end];

            info!(
                sheet = sheet_idx + 1,
                total_sheets = total_sheets,
                images = sheet_sources.len(),
                "Processing sprite sheet"
            );

            // Load and resize images in parallel using rayon
            let thumb_w = config.thumb_width;
            let thumb_h = config.thumb_height;
            let thumbnails: Vec<Result<(usize, RgbaImage), SpriteError>> = sheet_sources
                .par_iter()
                .enumerate()
                .map(|(i, source)| {
                    let img = ImageReader::open(source.as_ref())
                        .map_err(SpriteError::Open)?
                        .decode()
                        .map_err(SpriteError::Decode)?;
                    let thumb = resize_to_thumb(&img, thumb_w, thumb_h);
                    Ok((i, thumb))
                })
                .collect();

            // Check for errors and collect successful thumbnails
            let mut thumbs_sorted: Vec<(usize, RgbaImage)> =
                Vec::with_capacity(sheet_sources.len());
            for result in thumbnails {
                thumbs_sorted.push(result?);
            }
            thumbs_sorted.sort_by_key(|(i, _)| *i);

            // Create sheet and composite thumbnails
            let (sheet_width, sheet_height) = config.sheet_dimensions();
            let mut sheet_img = RgbaImage::new(sheet_width, sheet_height);

            // Fill with background
            let bg = image::Rgba(config.background);
            for pixel in sheet_img.pixels_mut() {
                *pixel = bg;
            }

            // Place thumbnails
            for (i, thumb) in thumbs_sorted {
                let col = (i as u32) % config.grid_columns;
                let row = (i as u32) / config.grid_columns;
                let x = col * config.thumb_width;
                let y = row * config.thumb_height;

                // Center the thumbnail in its cell (in case resize didn't fill exactly)
                let offset_x = (config.thumb_width - thumb.width()) / 2;
                let offset_y = (config.thumb_height - thumb.height()) / 2;

                image::imageops::overlay(
                    &mut sheet_img,
                    &thumb,
                    (x + offset_x) as i64,
                    (y + offset_y) as i64,
                );

                // Record location
                locations.push(SpriteLocation {
                    sheet: sheet_idx as u16,
                    x: col as u8,
                    y: row as u8,
                });
            }

            // Save sheet as lossy WebP
            let path = output_dir.join(format!("{:04}.webp", sheet_idx));
            let save_start = Instant::now();
            let encoder = webp::Encoder::from_rgba(&sheet_img, sheet_width, sheet_height);
            let webp_data = encoder.encode(85.0); // Quality 85
            std::fs::write(&path, &*webp_data)?;
            let save_time = save_start.elapsed();

            let file_size = std::fs::metadata(&path)?.len();
            let sheet_time = sheet_start.elapsed();

            info!(
                sheet = sheet_idx + 1,
                images = sheet_sources.len(),
                file_size_kb = file_size / 1024,
                process_ms = sheet_time.as_millis() - save_time.as_millis(),
                save_ms = save_time.as_millis(),
                total_ms = sheet_time.as_millis(),
                "Saved sprite sheet"
            );

            sheets.push(SpriteSheet {
                index: sheet_idx as u32,
                count: sheet_sources.len() as u32,
                path,
                file_size,
            });

            on_progress(end, total);
        }

        let total_time = batch_start.elapsed();
        let total_size: u64 = sheets.iter().map(|s| s.file_size).sum();
        info!(
            total_images = sources.len(),
            sheet_count = sheets.len(),
            total_size_mb = format!("{:.2}", total_size as f64 / 1024.0 / 1024.0),
            total_secs = format!("{:.1}", total_time.as_secs_f64()),
            images_per_sec = format!("{:.1}", sources.len() as f64 / total_time.as_secs_f64()),
            "Sprite generation complete"
        );

        Ok((sheets, locations))
    }
}

/// Resize image to a square thumbnail.
///
/// The image is scaled to fit within the thumbnail size while maintaining
/// aspect ratio, then centered on a transparent background.
/// Resize image to fit within cell dimensions while preserving aspect ratio.
fn resize_to_thumb(img: &DynamicImage, cell_width: u32, cell_height: u32) -> RgbaImage {
    let (w, h) = (img.width(), img.height());

    // Calculate scale to fit within cell while preserving aspect ratio
    let scale_w = cell_width as f32 / w as f32;
    let scale_h = cell_height as f32 / h as f32;
    let scale = scale_w.min(scale_h);

    let new_w = ((w as f32) * scale).round() as u32;
    let new_h = ((h as f32) * scale).round() as u32;

    // Resize with high-quality filter
    let resized = img.resize_exact(new_w, new_h, FilterType::Lanczos3);

    // Create canvas and center the image
    let mut canvas = RgbaImage::new(cell_width, cell_height);
    let offset_x = (cell_width - new_w) / 2;
    let offset_y = (cell_height - new_h) / 2;

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
        assert_eq!(config.thumb_width, 256);
        assert_eq!(config.thumb_height, 256);
        assert_eq!(config.grid_columns, 4);
        assert_eq!(config.grid_rows, 4);
        assert_eq!(config.thumbs_per_sheet(), 16);
        assert_eq!(config.sheet_dimensions(), (1024, 1024));
    }

    #[test]
    fn test_sprite_config_custom() {
        let config = SpriteConfig {
            thumb_width: 100,
            thumb_height: 150,
            grid_columns: 8,
            grid_rows: 6,
            background: [255, 255, 255, 255],
            max_sheet_size: 2048,
        };
        assert_eq!(config.thumbs_per_sheet(), 48);
        assert_eq!(config.sheet_dimensions(), (800, 900));
    }

    #[test]
    fn test_sprite_config_for_aspect_ratio_square() {
        let config = SpriteConfig::for_aspect_ratio(1000, 1000, 1024);
        assert_eq!(config.thumb_width, 256);
        assert_eq!(config.thumb_height, 256);
        assert_eq!(config.grid_columns, 4);
        assert_eq!(config.grid_rows, 4);
    }

    #[test]
    fn test_sprite_config_for_aspect_ratio_portrait() {
        // 2:3 portrait (e.g., 600x900)
        let config = SpriteConfig::for_aspect_ratio(600, 900, 1024);
        assert_eq!(config.thumb_width, 171); // 256 * (600/900) ≈ 171
        assert_eq!(config.thumb_height, 256);
        assert_eq!(config.grid_columns, 5); // 1024 / 171 = 5
        assert_eq!(config.grid_rows, 4); // 1024 / 256 = 4
        assert_eq!(config.thumbs_per_sheet(), 20);
    }

    #[test]
    fn test_sprite_config_for_aspect_ratio_landscape() {
        // 3:2 landscape (e.g., 900x600)
        let config = SpriteConfig::for_aspect_ratio(900, 600, 1024);
        assert_eq!(config.thumb_width, 256);
        assert_eq!(config.thumb_height, 171); // 256 * (600/900) ≈ 171
        assert_eq!(config.grid_columns, 4); // 1024 / 256 = 4
        assert_eq!(config.grid_rows, 5); // 1024 / 171 = 5
        assert_eq!(config.thumbs_per_sheet(), 20);
    }
}
