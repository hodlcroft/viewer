//! Sprite sheet metadata for the binary format.

/// Sprite image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SpriteFormat {
    #[default]
    WebP = 0,
    Png = 1,
}

impl From<u8> for SpriteFormat {
    fn from(v: u8) -> Self {
        match v {
            0 => SpriteFormat::WebP,
            1 => SpriteFormat::Png,
            _ => SpriteFormat::WebP,
        }
    }
}

/// Sprite metadata section (12 bytes).
///
/// Describes the sprite sheet configuration for thumbnail display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteMetadata {
    /// Thumbnail cell width in pixels
    pub thumb_width: u16,
    /// Thumbnail cell height in pixels
    pub thumb_height: u16,
    /// Columns per sheet
    pub grid_columns: u8,
    /// Rows per sheet
    pub grid_rows: u8,
    /// Total number of sprite sheets
    pub sheet_count: u16,
    /// Image format (WebP, PNG)
    pub format: SpriteFormat,
    // 3 bytes reserved
}

impl Default for SpriteMetadata {
    fn default() -> Self {
        Self {
            thumb_width: 256,
            thumb_height: 256,
            grid_columns: 8,
            grid_rows: 8,
            sheet_count: 0,
            format: SpriteFormat::WebP,
        }
    }
}

/// Size of SpriteMetadata in bytes.
pub const SPRITE_METADATA_SIZE: usize = 12;

impl SpriteMetadata {
    /// Images per sprite sheet.
    pub fn images_per_sheet(&self) -> u32 {
        self.grid_columns as u32 * self.grid_rows as u32
    }

    /// Sheet dimensions in pixels.
    pub fn sheet_dimensions(&self) -> (u32, u32) {
        (
            self.thumb_width as u32 * self.grid_columns as u32,
            self.thumb_height as u32 * self.grid_rows as u32,
        )
    }

    /// Calculate sprite location for a token index.
    pub fn location_for_index(&self, token_index: u32) -> (u16, u8, u8) {
        let per_sheet = self.images_per_sheet();
        let sheet = (token_index / per_sheet) as u16;
        let pos_in_sheet = token_index % per_sheet;
        let col = (pos_in_sheet % self.grid_columns as u32) as u8;
        let row = (pos_in_sheet / self.grid_columns as u32) as u8;
        (sheet, col, row)
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> [u8; SPRITE_METADATA_SIZE] {
        let mut buf = [0u8; SPRITE_METADATA_SIZE];
        buf[0..2].copy_from_slice(&self.thumb_width.to_le_bytes());
        buf[2..4].copy_from_slice(&self.thumb_height.to_le_bytes());
        buf[4] = self.grid_columns;
        buf[5] = self.grid_rows;
        buf[6..8].copy_from_slice(&self.sheet_count.to_le_bytes());
        buf[8] = self.format as u8;
        // buf[9..12] reserved
        buf
    }

    /// Deserialize from bytes.
    pub fn from_bytes(buf: &[u8; SPRITE_METADATA_SIZE]) -> Self {
        Self {
            thumb_width: u16::from_le_bytes([buf[0], buf[1]]),
            thumb_height: u16::from_le_bytes([buf[2], buf[3]]),
            grid_columns: buf[4],
            grid_rows: buf[5],
            sheet_count: u16::from_le_bytes([buf[6], buf[7]]),
            format: SpriteFormat::from(buf[8]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_metadata_roundtrip() {
        let meta = SpriteMetadata {
            thumb_width: 170,
            thumb_height: 256,
            grid_columns: 12,
            grid_rows: 8,
            sheet_count: 21,
            format: SpriteFormat::WebP,
        };

        let bytes = meta.to_bytes();
        let restored = SpriteMetadata::from_bytes(&bytes);

        assert_eq!(meta, restored);
    }

    #[test]
    fn test_sprite_metadata_defaults() {
        let meta = SpriteMetadata::default();
        assert_eq!(meta.thumb_width, 256);
        assert_eq!(meta.thumb_height, 256);
        assert_eq!(meta.grid_columns, 8);
        assert_eq!(meta.grid_rows, 8);
        assert_eq!(meta.images_per_sheet(), 64);
        assert_eq!(meta.sheet_dimensions(), (2048, 2048));
    }

    #[test]
    fn test_location_for_index() {
        let meta = SpriteMetadata {
            thumb_width: 256,
            thumb_height: 256,
            grid_columns: 8,
            grid_rows: 8,
            sheet_count: 32,
            format: SpriteFormat::WebP,
        };

        // First image
        assert_eq!(meta.location_for_index(0), (0, 0, 0));
        // Last in first row
        assert_eq!(meta.location_for_index(7), (0, 7, 0));
        // First in second row
        assert_eq!(meta.location_for_index(8), (0, 0, 1));
        // Last in first sheet (index 63)
        assert_eq!(meta.location_for_index(63), (0, 7, 7));
        // First in second sheet (index 64)
        assert_eq!(meta.location_for_index(64), (1, 0, 0));
    }

    #[test]
    fn test_non_square_aspect_ratio() {
        // Portrait 2:3 aspect ratio
        let meta = SpriteMetadata {
            thumb_width: 170,
            thumb_height: 256,
            grid_columns: 12,
            grid_rows: 8,
            sheet_count: 21,
            format: SpriteFormat::WebP,
        };

        assert_eq!(meta.images_per_sheet(), 96);
        assert_eq!(meta.sheet_dimensions(), (2040, 2048));
    }
}
