//! String reference size selection.
//!
//! String references can be 16-bit (up to 64KB string table) or 32-bit (up to 4GB).
//! The size is chosen during ingestion based on the total string table size.

/// String reference size variants.
///
/// Selected during ingestion based on total string table size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum StringRefSize {
    /// 16-bit offsets, up to 64KB string table (2 bytes per reference)
    #[default]
    U16 = 0,
    /// 32-bit offsets, up to 4GB string table (4 bytes per reference)
    U32 = 1,
}

impl StringRefSize {
    /// Select the smallest size that fits the given string table size in bytes.
    pub fn for_size(string_table_bytes: usize) -> Self {
        if string_table_bytes <= u16::MAX as usize {
            StringRefSize::U16
        } else {
            StringRefSize::U32
        }
    }

    /// Number of bytes per string reference.
    pub const fn byte_size(self) -> usize {
        match self {
            StringRefSize::U16 => 2,
            StringRefSize::U32 => 4,
        }
    }

    /// Maximum string table size in bytes.
    pub const fn max_size(self) -> usize {
        match self {
            StringRefSize::U16 => u16::MAX as usize,
            StringRefSize::U32 => u32::MAX as usize,
        }
    }

    /// Convert from raw u8 value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(StringRefSize::U16),
            1 => Some(StringRefSize::U32),
            _ => None,
        }
    }
}

impl std::fmt::Display for StringRefSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StringRefSize::U16 => write!(f, "U16 (2 bytes, max 64KB)"),
            StringRefSize::U32 => write!(f, "U32 (4 bytes, max 4GB)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_for_size() {
        assert_eq!(StringRefSize::for_size(0), StringRefSize::U16);
        assert_eq!(StringRefSize::for_size(65535), StringRefSize::U16);
        assert_eq!(StringRefSize::for_size(65536), StringRefSize::U32);
        assert_eq!(StringRefSize::for_size(1_000_000), StringRefSize::U32);
    }

    #[test]
    fn test_byte_size() {
        assert_eq!(StringRefSize::U16.byte_size(), 2);
        assert_eq!(StringRefSize::U32.byte_size(), 4);
    }
}
