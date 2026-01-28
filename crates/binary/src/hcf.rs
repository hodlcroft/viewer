//! HCF (High-Compression Format) bundle metadata.
//!
//! Tokens reference images in HCF bundle shards via offset/length pairs.
//! The index size is chosen based on total bundle size and max image size.

/// HCF index size variants for image location storage.
///
/// Selected during ingestion based on total HCF size and max image size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HcfIndexSize {
    /// offset: u32, length: u16 - 6 bytes (up to 4GB total, 64KB per image)
    U32U16 = 0,
    /// offset: u32, length: u24 - 7 bytes (up to 4GB total, 16MB per image)
    U32U24 = 1,
    /// offset: u40, length: u24 - 8 bytes (up to 1TB total, 16MB per image)
    U40U24 = 2,
}

impl HcfIndexSize {
    /// Select the appropriate index size based on total size and max image size.
    pub fn for_sizes(total_size: u64, max_image_size: u32) -> Self {
        let needs_large_offset = total_size > u32::MAX as u64;
        let needs_large_length = max_image_size > u16::MAX as u32;

        match (needs_large_offset, needs_large_length) {
            (false, false) => HcfIndexSize::U32U16,
            (false, true) => HcfIndexSize::U32U24,
            (true, _) => HcfIndexSize::U40U24,
        }
    }

    /// Number of bytes per token for HCF location.
    pub const fn byte_size(self) -> usize {
        match self {
            HcfIndexSize::U32U16 => 6,
            HcfIndexSize::U32U24 => 7,
            HcfIndexSize::U40U24 => 8,
        }
    }

    /// Maximum total HCF size this index can address.
    pub const fn max_total_size(self) -> u64 {
        match self {
            HcfIndexSize::U32U16 => u32::MAX as u64,
            HcfIndexSize::U32U24 => u32::MAX as u64,
            HcfIndexSize::U40U24 => 1 << 40, // 1 TB
        }
    }

    /// Maximum individual image size this index can store.
    pub const fn max_image_size(self) -> u32 {
        match self {
            HcfIndexSize::U32U16 => u16::MAX as u32,
            HcfIndexSize::U32U24 => (1 << 24) - 1, // 16 MB
            HcfIndexSize::U40U24 => (1 << 24) - 1, // 16 MB
        }
    }

    /// Convert from raw u8 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(HcfIndexSize::U32U16),
            1 => Some(HcfIndexSize::U32U24),
            2 => Some(HcfIndexSize::U40U24),
            _ => None,
        }
    }
}

impl std::fmt::Display for HcfIndexSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HcfIndexSize::U32U16 => write!(f, "U32+U16 (6 bytes)"),
            HcfIndexSize::U32U24 => write!(f, "U32+U24 (7 bytes)"),
            HcfIndexSize::U40U24 => write!(f, "U40+U24 (8 bytes)"),
        }
    }
}

/// Image format for HCF bundles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ImageFormat {
    WebP = 0,
    Png = 1,
    Avif = 2,
    Jpeg = 3,
}

impl ImageFormat {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(ImageFormat::WebP),
            1 => Some(ImageFormat::Png),
            2 => Some(ImageFormat::Avif),
            3 => Some(ImageFormat::Jpeg),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::WebP => "webp",
            ImageFormat::Png => "png",
            ImageFormat::Avif => "avif",
            ImageFormat::Jpeg => "jpg",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            ImageFormat::WebP => "image/webp",
            ImageFormat::Png => "image/png",
            ImageFormat::Avif => "image/avif",
            ImageFormat::Jpeg => "image/jpeg",
        }
    }
}

/// HCF bundle metadata stored in collection.bin.
#[derive(Debug, Clone, Copy)]
pub struct HcfMetadata {
    /// Fixed shard size in bytes (e.g., 250 MB)
    pub shard_size: u32,
    /// Number of HCF bundle shards
    pub shard_count: u16,
    /// Image format
    pub image_format: ImageFormat,
    /// Max width/height of images
    pub max_dimension: u16,
}

impl HcfMetadata {
    /// Serialize to bytes (12 bytes total).
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&self.shard_size.to_le_bytes());
        buf[4..6].copy_from_slice(&self.shard_count.to_le_bytes());
        buf[6] = self.image_format as u8;
        buf[7..9].copy_from_slice(&self.max_dimension.to_le_bytes());
        // bytes 9-11 reserved
        buf
    }

    /// Deserialize from bytes.
    pub fn from_bytes(buf: &[u8; 12]) -> Option<Self> {
        Some(Self {
            shard_size: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            shard_count: u16::from_le_bytes([buf[4], buf[5]]),
            image_format: ImageFormat::from_u8(buf[6])?,
            max_dimension: u16::from_le_bytes([buf[7], buf[8]]),
        })
    }

    /// Calculate which shard and offset for a global byte offset.
    pub fn locate(&self, global_offset: u64) -> HcfLocation {
        let shard_index = (global_offset / self.shard_size as u64) as u32;
        let offset_in_shard = (global_offset % self.shard_size as u64) as u32;
        HcfLocation {
            shard_index,
            offset: offset_in_shard,
            length: 0, // Must be filled in from token entry
        }
    }
}

/// Location of an image within the HCF bundles.
#[derive(Debug, Clone, Copy)]
pub struct HcfLocation {
    /// Shard index (0 = images_000.hcf)
    pub shard_index: u32,
    /// Byte offset within the shard
    pub offset: u32,
    /// Byte length of the image
    pub length: u32,
}

impl HcfLocation {
    /// Generate the shard filename.
    pub fn shard_filename(&self) -> String {
        format!("images_{:03}.hcf", self.shard_index)
    }

    /// Generate the full URL for the shard.
    pub fn shard_url(&self, base_url: &str, policy_id: &str) -> String {
        format!(
            "{}/cardano/{}/{}",
            base_url,
            policy_id,
            self.shard_filename()
        )
    }

    /// Generate HTTP Range header value.
    pub fn range_header(&self) -> String {
        format!("bytes={}-{}", self.offset, self.offset + self.length - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hcf_index_size_selection() {
        // Small collection with small images
        assert_eq!(
            HcfIndexSize::for_sizes(500_000_000, 50_000),
            HcfIndexSize::U32U16
        );

        // Medium collection with larger images
        assert_eq!(
            HcfIndexSize::for_sizes(1_000_000_000, 200_000),
            HcfIndexSize::U32U24
        );

        // Large collection
        assert_eq!(
            HcfIndexSize::for_sizes(5_000_000_000, 100_000),
            HcfIndexSize::U40U24
        );
    }

    #[test]
    fn test_hcf_metadata_roundtrip() {
        let meta = HcfMetadata {
            shard_size: 250_000_000,
            shard_count: 4,
            image_format: ImageFormat::WebP,
            max_dimension: 2048,
        };

        let bytes = meta.to_bytes();
        let parsed = HcfMetadata::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.shard_size, 250_000_000);
        assert_eq!(parsed.shard_count, 4);
        assert_eq!(parsed.image_format, ImageFormat::WebP);
        assert_eq!(parsed.max_dimension, 2048);
    }

    #[test]
    fn test_hcf_location() {
        let meta = HcfMetadata {
            shard_size: 250_000_000,
            shard_count: 4,
            image_format: ImageFormat::WebP,
            max_dimension: 2048,
        };

        // First shard
        let loc = meta.locate(100_000_000);
        assert_eq!(loc.shard_index, 0);
        assert_eq!(loc.offset, 100_000_000);

        // Second shard
        let loc = meta.locate(300_000_000);
        assert_eq!(loc.shard_index, 1);
        assert_eq!(loc.offset, 50_000_000);
    }
}
