//! Token table entries with variable-size bitmaps.
//!
//! Sprite locations are stored separately in thumbnails.bin.
//! HCF locations are stored in a separate index section, not inline with tokens.
//! This allows the token table to be built before HCF bundling is complete.

use crate::{BitmapSize, HcfIndexSize};

/// Bit mask for custom name flag in name_ref (high bit of u32)
pub const NAME_REF_CUSTOM_FLAG: u32 = 0x8000_0000;

/// Bit mask for string table offset in name_ref (lower 31 bits)
pub const NAME_REF_OFFSET_MASK: u32 = 0x7FFF_FFFF;

/// Fixed fields in a token entry (before variable-size data).
///
/// Layout (single-source):
/// - rarity_rank: u16
/// - rarity_score: u16 (fixed-point, score * 100)
/// - name_ref: u32
///   Total fixed: 8 bytes
///
/// Layout (multi-source, FLAG_MULTI_SOURCE set):
/// - source_index: u8 (added at start)
/// - rarity_rank: u16
/// - rarity_score: u16 (fixed-point, score * 100)
/// - name_ref: u32
///   Total fixed: 9 bytes
///
/// Note: Sprite locations are stored in thumbnails.bin, not here.
pub const TOKEN_FIXED_SIZE: usize = 8;
pub const TOKEN_FIXED_SIZE_MULTI_SOURCE: usize = 9;

/// A single token entry in the token table.
#[derive(Debug, Clone)]
pub struct TokenEntry {
    // Source index (only present if FLAG_MULTI_SOURCE, 1 byte)
    // For single-source collections, all tokens implicitly belong to source 0
    pub source_index: Option<u8>,

    // Rarity (4 bytes)
    pub rarity_rank: u16,
    pub rarity_score: u16, // Fixed-point: actual_score * 100

    // Name reference (4 bytes)
    // High bit set: offset into string table for custom name
    // Otherwise: token number for "{Collection} #{n}" pattern
    pub name_ref: u32,
    // Variable-size attributes bitmap (stored separately)
    // Variable-size HCF location (stored separately)
}

impl TokenEntry {
    /// Calculate the total entry size including variable bitmap.
    ///
    /// Note: HCF locations are stored separately in the HCF index section,
    /// not inline with token entries.
    pub fn entry_size(bitmap_size: BitmapSize, multi_source: bool) -> usize {
        let fixed = if multi_source {
            TOKEN_FIXED_SIZE_MULTI_SOURCE
        } else {
            TOKEN_FIXED_SIZE
        };
        fixed + bitmap_size.byte_size()
    }

    /// Serialize fixed fields to bytes.
    ///
    /// For multi-source collections, writes source_index as first byte.
    pub fn write_fixed(&self, buf: &mut [u8], multi_source: bool) {
        let offset = if multi_source {
            buf[0] = self.source_index.unwrap_or(0);
            1
        } else {
            0
        };
        buf[offset..offset + 2].copy_from_slice(&self.rarity_rank.to_le_bytes());
        buf[offset + 2..offset + 4].copy_from_slice(&self.rarity_score.to_le_bytes());
        buf[offset + 4..offset + 8].copy_from_slice(&self.name_ref.to_le_bytes());
    }

    /// Read fixed fields from bytes.
    ///
    /// For multi-source collections, reads source_index from first byte.
    pub fn read_fixed(buf: &[u8], multi_source: bool) -> Self {
        let (source_index, offset) = if multi_source {
            (Some(buf[0]), 1)
        } else {
            (None, 0)
        };
        Self {
            source_index,
            rarity_rank: u16::from_le_bytes([buf[offset], buf[offset + 1]]),
            rarity_score: u16::from_le_bytes([buf[offset + 2], buf[offset + 3]]),
            name_ref: u32::from_le_bytes([
                buf[offset + 4],
                buf[offset + 5],
                buf[offset + 6],
                buf[offset + 7],
            ]),
        }
    }
}

/// Write a bitmap to bytes based on size variant.
#[allow(dead_code)]
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
    fn test_entry_size_single_source() {
        // U64 bitmap (8) + fixed (8) = 16
        assert_eq!(TokenEntry::entry_size(BitmapSize::U64, false), 16);

        // U128 bitmap (16) + fixed (8) = 24
        assert_eq!(TokenEntry::entry_size(BitmapSize::U128, false), 24);

        // U256 bitmap (32) + fixed (8) = 40
        assert_eq!(TokenEntry::entry_size(BitmapSize::U256, false), 40);
    }

    #[test]
    fn test_entry_size_multi_source() {
        // U64 bitmap (8) + fixed (9) = 17
        assert_eq!(TokenEntry::entry_size(BitmapSize::U64, true), 17);

        // U128 bitmap (16) + fixed (9) = 25
        assert_eq!(TokenEntry::entry_size(BitmapSize::U128, true), 25);
    }

    #[test]
    fn test_token_entry_roundtrip_single_source() {
        let entry = TokenEntry {
            source_index: None,
            rarity_rank: 42,
            rarity_score: 1234,
            name_ref: 100,
        };

        let mut buf = [0u8; TOKEN_FIXED_SIZE];
        entry.write_fixed(&mut buf, false);
        let restored = TokenEntry::read_fixed(&buf, false);

        assert_eq!(restored.source_index, None);
        assert_eq!(restored.rarity_rank, 42);
        assert_eq!(restored.rarity_score, 1234);
        assert_eq!(restored.name_ref, 100);
    }

    #[test]
    fn test_token_entry_roundtrip_multi_source() {
        let entry = TokenEntry {
            source_index: Some(2),
            rarity_rank: 42,
            rarity_score: 1234,
            name_ref: 100,
        };

        let mut buf = [0u8; TOKEN_FIXED_SIZE_MULTI_SOURCE];
        entry.write_fixed(&mut buf, true);
        let restored = TokenEntry::read_fixed(&buf, true);

        assert_eq!(restored.source_index, Some(2));
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
