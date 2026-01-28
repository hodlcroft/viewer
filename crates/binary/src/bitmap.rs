//! Bitmap size selection for trait filtering.
//!
//! Each token stores a bitmask where each bit represents a trait:value combination.
//! The bitmap size is chosen during ingestion based on the total number of trait values.

/// Bitmap size variants for attribute storage.
///
/// Selected during ingestion based on total trait:value count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BitmapSize {
    /// Up to 64 trait:value combinations (8 bytes per token)
    U64 = 0,
    /// Up to 128 trait:value combinations (16 bytes per token)
    U128 = 1,
    /// Up to 256 trait:value combinations (32 bytes per token)
    U256 = 2,
    /// Up to 512 trait:value combinations (64 bytes per token)
    U512 = 3,
    /// Up to 1024 trait:value combinations (128 bytes per token)
    U1024 = 4,
}

impl BitmapSize {
    /// Select the smallest bitmap size that fits the given number of trait values.
    ///
    /// Returns `None` if the count exceeds 1024 (the maximum supported).
    pub fn for_count(trait_value_count: usize) -> Option<Self> {
        match trait_value_count {
            0..=64 => Some(BitmapSize::U64),
            65..=128 => Some(BitmapSize::U128),
            129..=256 => Some(BitmapSize::U256),
            257..=512 => Some(BitmapSize::U512),
            513..=1024 => Some(BitmapSize::U1024),
            _ => None,
        }
    }

    /// Number of bytes needed to store this bitmap.
    pub const fn byte_size(self) -> usize {
        match self {
            BitmapSize::U64 => 8,
            BitmapSize::U128 => 16,
            BitmapSize::U256 => 32,
            BitmapSize::U512 => 64,
            BitmapSize::U1024 => 128,
        }
    }

    /// Maximum number of trait:value combinations this size can hold.
    pub const fn max_values(self) -> usize {
        match self {
            BitmapSize::U64 => 64,
            BitmapSize::U128 => 128,
            BitmapSize::U256 => 256,
            BitmapSize::U512 => 512,
            BitmapSize::U1024 => 1024,
        }
    }

    /// Convert from raw u8 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(BitmapSize::U64),
            1 => Some(BitmapSize::U128),
            2 => Some(BitmapSize::U256),
            3 => Some(BitmapSize::U512),
            4 => Some(BitmapSize::U1024),
            _ => None,
        }
    }

    /// Maximum supported trait:value combinations across all bitmap sizes.
    pub const fn max_supported() -> usize {
        BitmapSize::U1024.max_values()
    }
}

impl std::fmt::Display for BitmapSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BitmapSize::U64 => write!(f, "U64 (8 bytes)"),
            BitmapSize::U128 => write!(f, "U128 (16 bytes)"),
            BitmapSize::U256 => write!(f, "U256 (32 bytes)"),
            BitmapSize::U512 => write!(f, "U512 (64 bytes)"),
            BitmapSize::U1024 => write!(f, "U1024 (128 bytes)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_for_count() {
        assert_eq!(BitmapSize::for_count(0), Some(BitmapSize::U64));
        assert_eq!(BitmapSize::for_count(64), Some(BitmapSize::U64));
        assert_eq!(BitmapSize::for_count(65), Some(BitmapSize::U128));
        assert_eq!(BitmapSize::for_count(128), Some(BitmapSize::U128));
        assert_eq!(BitmapSize::for_count(129), Some(BitmapSize::U256));
        assert_eq!(BitmapSize::for_count(256), Some(BitmapSize::U256));
        assert_eq!(BitmapSize::for_count(257), Some(BitmapSize::U512));
        assert_eq!(BitmapSize::for_count(512), Some(BitmapSize::U512));
        assert_eq!(BitmapSize::for_count(513), Some(BitmapSize::U1024));
        assert_eq!(BitmapSize::for_count(1024), Some(BitmapSize::U1024));
        assert_eq!(BitmapSize::for_count(1025), None);
    }

    #[test]
    fn test_byte_size() {
        assert_eq!(BitmapSize::U64.byte_size(), 8);
        assert_eq!(BitmapSize::U128.byte_size(), 16);
        assert_eq!(BitmapSize::U256.byte_size(), 32);
        assert_eq!(BitmapSize::U512.byte_size(), 64);
        assert_eq!(BitmapSize::U1024.byte_size(), 128);
    }
}
