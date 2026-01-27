//! Token table entries with variable-size bitmaps and HCF locations.

use crate::{BitmapSize, HcfIndexSize};

/// Fixed fields in a token entry (before variable-size data).
///
/// Layout:
/// - sprite_sheet: u16
/// - sprite_x: u8
/// - sprite_y: u8
/// - rarity_rank: u16
/// - rarity_score: u16 (fixed-point, score * 100)
/// - name_ref: u16
/// Total fixed: 10 bytes
pub const TOKEN_FIXED_SIZE: usize = 10;

/// A single token entry in the token table.
#[derive(Debug, Clone)]
pub struct TokenEntry {
    // Sprite location (4 bytes)
    pub sprite_sheet: u16,
    pub sprite_x: u8,
    pub sprite_y: u8,

    // Rarity (4 bytes)
    pub rarity_rank: u16,
    pub rarity_score: u16, // Fixed-point: actual_score * 100

    // Name reference (2 bytes)
    // High bit set: index into custom names table
    // Otherwise: token number for "{Collection} #{n}" pattern
    pub name_ref: u16,
    // Variable-size attributes bitmap (stored separately)
    // Variable-size HCF location (stored separately)
}

impl TokenEntry {
    /// Calculate the total entry size including variable fields.
    pub fn entry_size(bitmap_size: BitmapSize, hcf_index_size: HcfIndexSize) -> usize {
        TOKEN_FIXED_SIZE + bitmap_size.byte_size() + hcf_index_size.byte_size()
    }

    /// Serialize fixed fields to bytes.
    pub fn write_fixed(&self, buf: &mut [u8]) {
        buf[0..2].copy_from_slice(&self.sprite_sheet.to_le_bytes());
        buf[2] = self.sprite_x;
        buf[3] = self.sprite_y;
        buf[4..6].copy_from_slice(&self.rarity_rank.to_le_bytes());
        buf[6..8].copy_from_slice(&self.rarity_score.to_le_bytes());
        buf[8..10].copy_from_slice(&self.name_ref.to_le_bytes());
    }

    /// Read fixed fields from bytes.
    pub fn read_fixed(buf: &[u8]) -> Self {
        Self {
            sprite_sheet: u16::from_le_bytes([buf[0], buf[1]]),
            sprite_x: buf[2],
            sprite_y: buf[3],
            rarity_rank: u16::from_le_bytes([buf[4], buf[5]]),
            rarity_score: u16::from_le_bytes([buf[6], buf[7]]),
            name_ref: u16::from_le_bytes([buf[8], buf[9]]),
        }
    }
}

/// Write a bitmap to bytes based on size variant.
pub fn write_bitmap(bitmap: &[u8], buf: &mut [u8], size: BitmapSize) {
    let len = size.byte_size();
    buf[..len].copy_from_slice(&bitmap[..len]);
}

/// Write an HCF location to bytes based on size variant.
pub fn write_hcf_location(offset: u64, length: u32, buf: &mut [u8], size: HcfIndexSize) {
    match size {
        HcfIndexSize::U32U16 => {
            buf[0..4].copy_from_slice(&(offset as u32).to_le_bytes());
            buf[4..6].copy_from_slice(&(length as u16).to_le_bytes());
        }
        HcfIndexSize::U32U24 => {
            buf[0..4].copy_from_slice(&(offset as u32).to_le_bytes());
            buf[4] = length as u8;
            buf[5] = (length >> 8) as u8;
            buf[6] = (length >> 16) as u8;
        }
        HcfIndexSize::U40U24 => {
            // 5 bytes for offset (u40)
            buf[0] = offset as u8;
            buf[1] = (offset >> 8) as u8;
            buf[2] = (offset >> 16) as u8;
            buf[3] = (offset >> 24) as u8;
            buf[4] = (offset >> 32) as u8;
            // 3 bytes for length (u24)
            buf[5] = length as u8;
            buf[6] = (length >> 8) as u8;
            buf[7] = (length >> 16) as u8;
        }
    }
}

/// Read an HCF location from bytes based on size variant.
pub fn read_hcf_location(buf: &[u8], size: HcfIndexSize) -> (u64, u32) {
    match size {
        HcfIndexSize::U32U16 => {
            let offset = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64;
            let length = u16::from_le_bytes([buf[4], buf[5]]) as u32;
            (offset, length)
        }
        HcfIndexSize::U32U24 => {
            let offset = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64;
            let length = buf[4] as u32 | ((buf[5] as u32) << 8) | ((buf[6] as u32) << 16);
            (offset, length)
        }
        HcfIndexSize::U40U24 => {
            let offset = buf[0] as u64
                | ((buf[1] as u64) << 8)
                | ((buf[2] as u64) << 16)
                | ((buf[3] as u64) << 24)
                | ((buf[4] as u64) << 32);
            let length = buf[5] as u32 | ((buf[6] as u32) << 8) | ((buf[7] as u32) << 16);
            (offset, length)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_size() {
        // U64 bitmap (8) + U32U16 HCF (6) + fixed (10) = 24
        assert_eq!(
            TokenEntry::entry_size(BitmapSize::U64, HcfIndexSize::U32U16),
            24
        );

        // U128 bitmap (16) + U32U24 HCF (7) + fixed (10) = 33
        assert_eq!(
            TokenEntry::entry_size(BitmapSize::U128, HcfIndexSize::U32U24),
            33
        );
    }

    #[test]
    fn test_token_entry_roundtrip() {
        let entry = TokenEntry {
            sprite_sheet: 5,
            sprite_x: 3,
            sprite_y: 7,
            rarity_rank: 42,
            rarity_score: 1234,
            name_ref: 100,
        };

        let mut buf = [0u8; TOKEN_FIXED_SIZE];
        entry.write_fixed(&mut buf);
        let restored = TokenEntry::read_fixed(&buf);

        assert_eq!(restored.sprite_sheet, 5);
        assert_eq!(restored.sprite_x, 3);
        assert_eq!(restored.sprite_y, 7);
        assert_eq!(restored.rarity_rank, 42);
        assert_eq!(restored.rarity_score, 1234);
        assert_eq!(restored.name_ref, 100);
    }

    #[test]
    fn test_hcf_location_roundtrip() {
        // U32U16
        let mut buf = [0u8; 6];
        write_hcf_location(123456789, 50000, &mut buf, HcfIndexSize::U32U16);
        let (offset, length) = read_hcf_location(&buf, HcfIndexSize::U32U16);
        assert_eq!(offset, 123456789);
        assert_eq!(length, 50000);

        // U32U24
        let mut buf = [0u8; 7];
        write_hcf_location(123456789, 1_000_000, &mut buf, HcfIndexSize::U32U24);
        let (offset, length) = read_hcf_location(&buf, HcfIndexSize::U32U24);
        assert_eq!(offset, 123456789);
        assert_eq!(length, 1_000_000);

        // U40U24
        let mut buf = [0u8; 8];
        write_hcf_location(500_000_000_000, 5_000_000, &mut buf, HcfIndexSize::U40U24);
        let (offset, length) = read_hcf_location(&buf, HcfIndexSize::U40U24);
        assert_eq!(offset, 500_000_000_000);
        assert_eq!(length, 5_000_000);
    }
}
