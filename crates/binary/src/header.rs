//! Binary format header.
//!
//! The header is 128 bytes and contains format metadata plus offsets to all sections.
//! Reserved bytes at the end allow for future expansion without changing header size.

use crate::{BitmapSize, HcfIndexSize, MAGIC, StringRefSize, VERSION};

/// Fixed header size in bytes.
pub const HEADER_SIZE: usize = 128;

/// Feature flag: Collection has multiple sources, tokens include source_index field.
pub const FLAG_MULTI_SOURCE: u16 = 1 << 0;

/// Feature flag: Hide rarity rankings in the UI.
pub const FLAG_HIDE_RARITY: u16 = 1 << 1;

/// Header structure for collection.bin files.
///
/// All multi-byte integers are little-endian.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Header {
    /// Magic bytes: "COLL"
    pub magic: [u8; 4],
    /// Format version
    pub version: u16,
    /// Feature flags (reserved)
    pub flags: u16,
    /// Number of tokens in the collection
    pub token_count: u32,
    /// Number of traits
    pub trait_count: u8,
    /// Bitmap size enum
    pub bitmap_size: u8,
    /// HCF index size enum
    pub hcf_index_size: u8,
    /// Number of sources (1 = single chain, >1 = multi-chain)
    pub source_count: u8,

    // Section offsets (from start of file)
    /// Offset to string table
    pub string_table_offset: u32,
    /// Offset to trait schema
    pub trait_schema_offset: u32,
    /// Offset to trait index (inverted)
    pub trait_index_offset: u32,
    /// Offset to token table
    pub token_table_offset: u32,
    /// Offset to PHF data
    pub phf_offset: u32,
    /// Reserved (was sprites_offset, now in sprites.bin)
    pub reserved_sprites: u32,
    /// Offset to HCF metadata
    pub hcf_metadata_offset: u32,
    /// Offset to HCF index (array of offset/length per token)
    pub hcf_index_offset: u32,
    /// Offset to sources section
    pub sources_offset: u32,
    /// Offset to asset ID index (array of u16 string refs, one per token)
    pub asset_id_index_offset: u32,

    /// String reference size enum (0 = u16, 1 = u32)
    pub string_ref_size: u8,

    /// Reserved for future use (must be zero)
    pub reserved: [u8; 71],
}

impl Header {
    /// Create a new header with default values.
    pub fn new(
        token_count: u32,
        trait_count: u8,
        bitmap_size: BitmapSize,
        hcf_index_size: HcfIndexSize,
        source_count: u8,
    ) -> Self {
        Self::with_string_ref_size(
            token_count,
            trait_count,
            bitmap_size,
            hcf_index_size,
            source_count,
            StringRefSize::U16,
        )
    }

    /// Create a new header with explicit string reference size.
    pub fn with_string_ref_size(
        token_count: u32,
        trait_count: u8,
        bitmap_size: BitmapSize,
        hcf_index_size: HcfIndexSize,
        source_count: u8,
        string_ref_size: StringRefSize,
    ) -> Self {
        let flags = if source_count > 1 {
            FLAG_MULTI_SOURCE
        } else {
            0
        };
        Self {
            magic: MAGIC,
            version: VERSION,
            flags,
            token_count,
            trait_count,
            bitmap_size: bitmap_size as u8,
            hcf_index_size: hcf_index_size as u8,
            source_count,
            string_table_offset: 0,
            trait_schema_offset: 0,
            trait_index_offset: 0,
            token_table_offset: 0,
            phf_offset: 0,
            reserved_sprites: 0,
            hcf_metadata_offset: 0,
            hcf_index_offset: 0,
            sources_offset: 0,
            asset_id_index_offset: 0,
            string_ref_size: string_ref_size as u8,
            reserved: [0u8; 71],
        }
    }

    /// Check if this collection has multiple sources.
    pub fn is_multi_source(&self) -> bool {
        self.flags & FLAG_MULTI_SOURCE != 0
    }

    /// Check if rarity rankings should be hidden.
    pub fn hide_rarity(&self) -> bool {
        self.flags & FLAG_HIDE_RARITY != 0
    }

    /// Get the bitmap size enum.
    pub fn bitmap_size(&self) -> Option<BitmapSize> {
        BitmapSize::from_u8(self.bitmap_size)
    }

    /// Get the HCF index size enum.
    pub fn hcf_index_size(&self) -> Option<HcfIndexSize> {
        HcfIndexSize::from_u8(self.hcf_index_size)
    }

    /// Get the string reference size enum.
    pub fn string_ref_size(&self) -> StringRefSize {
        StringRefSize::from_u8(self.string_ref_size).unwrap_or_default()
    }

    /// Serialize header to bytes (little-endian).
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];

        buf[0..4].copy_from_slice(&self.magic);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..8].copy_from_slice(&self.flags.to_le_bytes());
        buf[8..12].copy_from_slice(&self.token_count.to_le_bytes());
        buf[12] = self.trait_count;
        buf[13] = self.bitmap_size;
        buf[14] = self.hcf_index_size;
        buf[15] = self.source_count;
        buf[16..20].copy_from_slice(&self.string_table_offset.to_le_bytes());
        buf[20..24].copy_from_slice(&self.trait_schema_offset.to_le_bytes());
        buf[24..28].copy_from_slice(&self.trait_index_offset.to_le_bytes());
        buf[28..32].copy_from_slice(&self.token_table_offset.to_le_bytes());
        buf[32..36].copy_from_slice(&self.phf_offset.to_le_bytes());
        buf[36..40].copy_from_slice(&self.reserved_sprites.to_le_bytes());
        buf[40..44].copy_from_slice(&self.hcf_metadata_offset.to_le_bytes());
        buf[44..48].copy_from_slice(&self.hcf_index_offset.to_le_bytes());
        buf[48..52].copy_from_slice(&self.sources_offset.to_le_bytes());
        buf[52..56].copy_from_slice(&self.asset_id_index_offset.to_le_bytes());
        buf[56] = self.string_ref_size;
        // buf[57..128] reserved (already zero)

        buf
    }

    /// Deserialize header from bytes.
    pub fn from_bytes(buf: &[u8; HEADER_SIZE]) -> Self {
        let mut reserved = [0u8; 71];
        reserved.copy_from_slice(&buf[57..128]);

        Self {
            magic: [buf[0], buf[1], buf[2], buf[3]],
            version: u16::from_le_bytes([buf[4], buf[5]]),
            flags: u16::from_le_bytes([buf[6], buf[7]]),
            token_count: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            trait_count: buf[12],
            bitmap_size: buf[13],
            hcf_index_size: buf[14],
            source_count: buf[15],
            string_table_offset: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            trait_schema_offset: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            trait_index_offset: u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
            token_table_offset: u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
            phf_offset: u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            reserved_sprites: u32::from_le_bytes([buf[36], buf[37], buf[38], buf[39]]),
            hcf_metadata_offset: u32::from_le_bytes([buf[40], buf[41], buf[42], buf[43]]),
            hcf_index_offset: u32::from_le_bytes([buf[44], buf[45], buf[46], buf[47]]),
            sources_offset: u32::from_le_bytes([buf[48], buf[49], buf[50], buf[51]]),
            asset_id_index_offset: u32::from_le_bytes([buf[52], buf[53], buf[54], buf[55]]),
            string_ref_size: buf[56],
            reserved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = Header::new(10000, 10, BitmapSize::U128, HcfIndexSize::U32U16, 1);
        let bytes = header.to_bytes();
        let parsed = Header::from_bytes(&bytes);

        assert_eq!(parsed.magic, MAGIC);
        assert_eq!(parsed.version, VERSION);
        assert_eq!(parsed.token_count, 10000);
        assert_eq!(parsed.trait_count, 10);
        assert_eq!(parsed.bitmap_size().unwrap(), BitmapSize::U128);
        assert_eq!(parsed.hcf_index_size().unwrap(), HcfIndexSize::U32U16);
        assert_eq!(parsed.source_count, 1);
        assert!(!parsed.is_multi_source());
    }

    #[test]
    fn test_header_multi_source() {
        let header = Header::new(5000, 8, BitmapSize::U64, HcfIndexSize::U32U16, 3);
        let bytes = header.to_bytes();
        let parsed = Header::from_bytes(&bytes);

        assert_eq!(parsed.source_count, 3);
        assert!(parsed.is_multi_source());
        assert_eq!(parsed.flags & FLAG_MULTI_SOURCE, FLAG_MULTI_SOURCE);
    }

    #[test]
    fn test_header_size() {
        assert_eq!(HEADER_SIZE, 128);
        let header = Header::new(0, 0, BitmapSize::U64, HcfIndexSize::U32U16, 1);
        assert_eq!(header.to_bytes().len(), HEADER_SIZE);
    }

    #[test]
    fn test_header_reserved_bytes() {
        let header = Header::new(100, 5, BitmapSize::U64, HcfIndexSize::U32U16, 1);
        let bytes = header.to_bytes();

        // Reserved bytes should be zero (byte 57-127)
        assert_eq!(&bytes[57..128], &[0u8; 71]);

        let parsed = Header::from_bytes(&bytes);
        assert_eq!(parsed.reserved, [0u8; 71]);
    }

    #[test]
    fn test_header_string_ref_size() {
        // Default should be U16
        let header = Header::new(100, 5, BitmapSize::U64, HcfIndexSize::U32U16, 1);
        assert_eq!(header.string_ref_size(), StringRefSize::U16);

        let bytes = header.to_bytes();
        assert_eq!(bytes[56], 0); // U16 = 0

        let parsed = Header::from_bytes(&bytes);
        assert_eq!(parsed.string_ref_size(), StringRefSize::U16);

        // Test with U32
        let header = Header::with_string_ref_size(
            100,
            5,
            BitmapSize::U64,
            HcfIndexSize::U32U16,
            1,
            StringRefSize::U32,
        );
        assert_eq!(header.string_ref_size(), StringRefSize::U32);

        let bytes = header.to_bytes();
        assert_eq!(bytes[56], 1); // U32 = 1

        let parsed = Header::from_bytes(&bytes);
        assert_eq!(parsed.string_ref_size(), StringRefSize::U32);
    }

    #[test]
    fn test_asset_id_index_offset() {
        let mut header = Header::new(100, 5, BitmapSize::U64, HcfIndexSize::U32U16, 1);
        header.asset_id_index_offset = 12345;

        let bytes = header.to_bytes();
        let parsed = Header::from_bytes(&bytes);

        assert_eq!(parsed.asset_id_index_offset, 12345);
    }
}
