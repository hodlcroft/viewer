//! Deduplicated string table for trait names and values.
//!
//! Strings are stored once and referenced by offsets. The offset size can be
//! configured via `StringRefSize` - U16 for tables up to 64KB, U32 for larger.

use crate::{BinaryFormatError, StringRefSize};
use std::collections::HashMap;

/// Reference to a string in the string table.
///
/// Stores a 32-bit offset internally, but serialization size depends on
/// the collection's `StringRefSize` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StringRef(pub u32);

impl StringRef {
    /// The null/empty string reference.
    pub const EMPTY: StringRef = StringRef(0);

    /// Create from a u16 offset (for backwards compatibility).
    pub const fn from_u16(offset: u16) -> Self {
        StringRef(offset as u32)
    }

    /// Get as u16 (panics if > u16::MAX).
    pub fn as_u16(self) -> u16 {
        self.0 as u16
    }

    /// Serialize to bytes based on the ref size.
    pub fn to_bytes(self, size: StringRefSize) -> Vec<u8> {
        match size {
            StringRefSize::U16 => (self.0 as u16).to_le_bytes().to_vec(),
            StringRefSize::U32 => self.0.to_le_bytes().to_vec(),
        }
    }

    /// Deserialize from bytes based on the ref size.
    pub fn from_bytes(bytes: &[u8], size: StringRefSize) -> Option<Self> {
        match size {
            StringRefSize::U16 => {
                if bytes.len() < 2 {
                    return None;
                }
                Some(StringRef(u16::from_le_bytes([bytes[0], bytes[1]]) as u32))
            }
            StringRefSize::U32 => {
                if bytes.len() < 4 {
                    return None;
                }
                Some(StringRef(u32::from_le_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3],
                ])))
            }
        }
    }
}

/// Builder for constructing a deduplicated string table.
#[derive(Clone)]
pub struct StringTableBuilder {
    /// Map from string content to offset
    offsets: HashMap<String, u32>,
    /// Concatenated string data (null-terminated)
    data: Vec<u8>,
    /// Maximum reference size (determines max table size)
    ref_size: StringRefSize,
}

impl StringTableBuilder {
    /// Create a new builder with U16 references (default, up to 64KB).
    pub fn new() -> Self {
        Self::with_ref_size(StringRefSize::U16)
    }

    /// Create a new builder with the specified reference size.
    pub fn with_ref_size(ref_size: StringRefSize) -> Self {
        let mut builder = Self {
            offsets: HashMap::new(),
            data: Vec::new(),
            ref_size,
        };
        // Reserve offset 0 for empty string
        builder.data.push(0);
        builder.offsets.insert(String::new(), 0);
        builder
    }

    /// Add a string and return its reference.
    ///
    /// If the string already exists, returns the existing reference.
    pub fn add(&mut self, s: &str) -> Result<StringRef, BinaryFormatError> {
        if let Some(&offset) = self.offsets.get(s) {
            return Ok(StringRef(offset));
        }

        let offset = self.data.len();
        let max_size = self.ref_size.max_size();

        if offset > max_size {
            return Err(BinaryFormatError::StringTableOverflow(format!(
                "string table data exceeds {} bytes at {} bytes (consider using StringRefSize::U32)",
                max_size, offset
            )));
        }

        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0); // null terminator

        self.offsets.insert(s.to_string(), offset as u32);
        Ok(StringRef(offset as u32))
    }

    /// Get the number of unique strings.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.offsets.len() <= 1 // Only the empty string
    }

    /// Get the current data size in bytes.
    pub fn data_size(&self) -> usize {
        self.data.len()
    }

    /// Get the configured reference size.
    pub fn ref_size(&self) -> StringRefSize {
        self.ref_size
    }

    /// Build the final string table.
    pub fn build(self) -> Result<StringTable, BinaryFormatError> {
        Ok(StringTable {
            data: self.data,
            ref_size: self.ref_size,
        })
    }
}

impl Default for StringTableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable string table for reading.
#[derive(Debug, Clone)]
pub struct StringTable {
    data: Vec<u8>,
    ref_size: StringRefSize,
}

impl StringTable {
    /// Create from raw data bytes with U16 references (default).
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self::from_bytes_with_ref_size(data, StringRefSize::U16)
    }

    /// Create from raw data bytes with the specified reference size.
    pub fn from_bytes_with_ref_size(data: Vec<u8>, ref_size: StringRefSize) -> Self {
        Self { data, ref_size }
    }

    /// Get a string by reference.
    ///
    /// Returns the string slice from the offset to the next null byte.
    pub fn get(&self, r: StringRef) -> Option<&str> {
        let start = r.0 as usize;
        if start >= self.data.len() {
            return None;
        }

        // Find null terminator
        let end = self.data[start..]
            .iter()
            .position(|&b| b == 0)
            .map(|pos| start + pos)?;

        std::str::from_utf8(&self.data[start..end]).ok()
    }

    /// Get the raw data for serialization.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get the reference size.
    pub fn ref_size(&self) -> StringRefSize {
        self.ref_size
    }

    /// Serialize to bytes.
    ///
    /// Format: [data_len: u32][data: bytes]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.data.len());
        buf.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.data);
        buf
    }

    /// Deserialize from bytes with U16 references (default).
    pub fn from_serialized(buf: &[u8]) -> Option<Self> {
        Self::from_serialized_with_ref_size(buf, StringRefSize::U16)
    }

    /// Deserialize from bytes with the specified reference size.
    pub fn from_serialized_with_ref_size(buf: &[u8], ref_size: StringRefSize) -> Option<Self> {
        if buf.len() < 4 {
            return None;
        }
        let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if buf.len() < 4 + len {
            return None;
        }
        Some(Self {
            data: buf[4..4 + len].to_vec(),
            ref_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_table_basic() {
        let mut builder = StringTableBuilder::new();

        let bg_ref = builder.add("Background").unwrap();
        let blue_ref = builder.add("Blue").unwrap();
        let red_ref = builder.add("Red").unwrap();

        // Deduplication
        let bg_ref2 = builder.add("Background").unwrap();
        assert_eq!(bg_ref, bg_ref2);

        let table = builder.build().unwrap();
        assert_eq!(table.get(bg_ref), Some("Background"));
        assert_eq!(table.get(blue_ref), Some("Blue"));
        assert_eq!(table.get(red_ref), Some("Red"));
    }

    #[test]
    fn test_string_table_empty() {
        let builder = StringTableBuilder::new();
        let table = builder.build().unwrap();
        assert_eq!(table.get(StringRef::EMPTY), Some(""));
    }

    #[test]
    fn test_string_table_serialization() {
        let mut builder = StringTableBuilder::new();
        builder.add("Hello").unwrap();
        builder.add("World").unwrap();

        let table = builder.build().unwrap();
        let bytes = table.to_bytes();
        let restored = StringTable::from_serialized(&bytes).unwrap();

        assert_eq!(table.data(), restored.data());
    }

    #[test]
    fn test_string_ref_serialization() {
        let r = StringRef(12345);

        // U16 serialization
        let bytes = r.to_bytes(StringRefSize::U16);
        assert_eq!(bytes.len(), 2);
        let restored = StringRef::from_bytes(&bytes, StringRefSize::U16).unwrap();
        assert_eq!(restored.0, 12345);

        // U32 serialization
        let bytes = r.to_bytes(StringRefSize::U32);
        assert_eq!(bytes.len(), 4);
        let restored = StringRef::from_bytes(&bytes, StringRefSize::U32).unwrap();
        assert_eq!(restored.0, 12345);
    }

    #[test]
    fn test_string_ref_large_offset() {
        // Test offset larger than u16::MAX
        let r = StringRef(100_000);

        // U32 serialization works
        let bytes = r.to_bytes(StringRefSize::U32);
        let restored = StringRef::from_bytes(&bytes, StringRefSize::U32).unwrap();
        assert_eq!(restored.0, 100_000);

        // U16 serialization truncates (as expected)
        let bytes = r.to_bytes(StringRefSize::U16);
        let restored = StringRef::from_bytes(&bytes, StringRefSize::U16).unwrap();
        assert_ne!(restored.0, 100_000); // Truncated
    }

    #[test]
    fn test_builder_with_u32_ref_size() {
        let mut builder = StringTableBuilder::with_ref_size(StringRefSize::U32);
        assert_eq!(builder.ref_size(), StringRefSize::U32);

        builder.add("test").unwrap();
        let table = builder.build().unwrap();
        assert_eq!(table.ref_size(), StringRefSize::U32);
    }
}
