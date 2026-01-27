//! Deduplicated string table for trait names and values.
//!
//! Strings are stored once and referenced by 16-bit offsets, enabling
//! compact storage and zero-copy access.

use crate::BinaryFormatError;
use std::collections::HashMap;

/// Reference to a string in the string table (16-bit offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StringRef(pub u16);

impl StringRef {
    /// The null/empty string reference.
    pub const EMPTY: StringRef = StringRef(0);
}

/// Builder for constructing a deduplicated string table.
pub struct StringTableBuilder {
    /// Map from string content to offset
    offsets: HashMap<String, u16>,
    /// Concatenated string data (null-terminated)
    data: Vec<u8>,
}

impl StringTableBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        let mut builder = Self {
            offsets: HashMap::new(),
            data: Vec::new(),
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
        if offset > u16::MAX as usize {
            return Err(BinaryFormatError::StringTableOverflow(format!(
                "string table data exceeds 64KB at {} bytes",
                offset
            )));
        }

        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0); // null terminator

        self.offsets.insert(s.to_string(), offset as u16);
        Ok(StringRef(offset as u16))
    }

    /// Get the number of unique strings.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.offsets.len() <= 1 // Only the empty string
    }

    /// Build the final string table.
    pub fn build(self) -> Result<StringTable, BinaryFormatError> {
        if self.offsets.len() > u16::MAX as usize {
            return Err(BinaryFormatError::StringTableOverflow(format!(
                "too many strings: {} (max 65535)",
                self.offsets.len()
            )));
        }

        Ok(StringTable { data: self.data })
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
}

impl StringTable {
    /// Create from raw data bytes.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self { data }
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

    /// Serialize to bytes.
    ///
    /// Format: [data_len: u32][data: bytes]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.data.len());
        buf.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.data);
        buf
    }

    /// Deserialize from bytes.
    pub fn from_serialized(buf: &[u8]) -> Option<Self> {
        if buf.len() < 4 {
            return None;
        }
        let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if buf.len() < 4 + len {
            return None;
        }
        Some(Self {
            data: buf[4..4 + len].to_vec(),
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

        assert_eq!(table.data, restored.data);
    }
}
