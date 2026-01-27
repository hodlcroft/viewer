//! Binary format header.
//!
//! The header is always 40 bytes and contains format metadata plus offsets to all sections.

use crate::{BitmapSize, HcfIndexSize, MAGIC, VERSION};

/// Fixed header size in bytes.
pub const HEADER_SIZE: usize = 40;

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
    /// Reserved for future use
    pub _reserved: u8,

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
    /// Offset to sprite metadata
    pub sprites_offset: u32,
    /// Offset to HCF metadata
    pub hcf_metadata_offset: u32,
}

impl Header {
    /// Create a new header with default values.
    pub fn new(
        token_count: u32,
        trait_count: u8,
        bitmap_size: BitmapSize,
        hcf_index_size: HcfIndexSize,
    ) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            flags: 0,
            token_count,
            trait_count,
            bitmap_size: bitmap_size as u8,
            hcf_index_size: hcf_index_size as u8,
            _reserved: 0,
            string_table_offset: 0,
            trait_schema_offset: 0,
            trait_index_offset: 0,
            token_table_offset: 0,
            phf_offset: 0,
            sprites_offset: 0,
            hcf_metadata_offset: 0,
        }
    }

    /// Get the bitmap size enum.
    pub fn bitmap_size(&self) -> Option<BitmapSize> {
        BitmapSize::from_u8(self.bitmap_size)
    }

    /// Get the HCF index size enum.
    pub fn hcf_index_size(&self) -> Option<HcfIndexSize> {
        HcfIndexSize::from_u8(self.hcf_index_size)
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
        buf[15] = self._reserved;
        buf[16..20].copy_from_slice(&self.string_table_offset.to_le_bytes());
        buf[20..24].copy_from_slice(&self.trait_schema_offset.to_le_bytes());
        buf[24..28].copy_from_slice(&self.trait_index_offset.to_le_bytes());
        buf[28..32].copy_from_slice(&self.token_table_offset.to_le_bytes());
        buf[32..36].copy_from_slice(&self.phf_offset.to_le_bytes());
        buf[36..38].copy_from_slice(&(self.sprites_offset as u16).to_le_bytes());
        buf[38..40].copy_from_slice(&(self.hcf_metadata_offset as u16).to_le_bytes());

        buf
    }

    /// Deserialize header from bytes.
    pub fn from_bytes(buf: &[u8; HEADER_SIZE]) -> Self {
        Self {
            magic: [buf[0], buf[1], buf[2], buf[3]],
            version: u16::from_le_bytes([buf[4], buf[5]]),
            flags: u16::from_le_bytes([buf[6], buf[7]]),
            token_count: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            trait_count: buf[12],
            bitmap_size: buf[13],
            hcf_index_size: buf[14],
            _reserved: buf[15],
            string_table_offset: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            trait_schema_offset: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            trait_index_offset: u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
            token_table_offset: u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]),
            phf_offset: u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]),
            sprites_offset: u16::from_le_bytes([buf[36], buf[37]]) as u32,
            hcf_metadata_offset: u16::from_le_bytes([buf[38], buf[39]]) as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = Header::new(10000, 10, BitmapSize::U128, HcfIndexSize::U32U16);
        let bytes = header.to_bytes();
        let parsed = Header::from_bytes(&bytes);

        assert_eq!(parsed.magic, MAGIC);
        assert_eq!(parsed.version, VERSION);
        assert_eq!(parsed.token_count, 10000);
        assert_eq!(parsed.trait_count, 10);
        assert_eq!(parsed.bitmap_size().unwrap(), BitmapSize::U128);
        assert_eq!(parsed.hcf_index_size().unwrap(), HcfIndexSize::U32U16);
    }

    #[test]
    fn test_header_size() {
        assert_eq!(HEADER_SIZE, 40);
        let header = Header::new(0, 0, BitmapSize::U64, HcfIndexSize::U32U16);
        assert_eq!(header.to_bytes().len(), HEADER_SIZE);
    }
}
