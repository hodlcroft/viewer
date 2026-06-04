//! Sprite index format for fast sprite lookups.
//!
//! A lightweight index file (sprites.bin) mapping asset IDs to sprite sheet positions.
//! Designed for standalone use without loading the full collection.bin.
//!
//! # Format
//!
//! ```text
//! Header (18 bytes):
//!   magic: [u8; 4] = "SPRT"
//!   version: u16
//!   entry_count: u32
//!   sheet_count: u16
//!   thumb_width: u16
//!   thumb_height: u16
//!   grid_columns: u8
//!   grid_rows: u8
//!
//! Entries (12 bytes each, sorted by asset_id_hash):
//!   asset_id_hash: u64
//!   sheet: u16
//!   x: u8
//!   y: u8
//! ```

use xxhash_rust::xxh64::xxh64;

/// Magic bytes for sprites.bin
pub const SPRITE_INDEX_MAGIC: [u8; 4] = *b"SPRT";

/// Current format version
pub const SPRITE_INDEX_VERSION: u16 = 1;

/// Header size in bytes
pub const SPRITE_INDEX_HEADER_SIZE: usize = 18;

/// Entry size in bytes
pub const SPRITE_INDEX_ENTRY_SIZE: usize = 12;

/// Hash an asset ID string to u64 using xxHash64.
///
/// This produces a stable, fast hash suitable for binary search lookups.
pub fn hash_asset_id(asset_id: &str) -> u64 {
    xxh64(asset_id.as_bytes(), 0)
}

/// Sprite index header.
#[derive(Debug, Clone, Copy)]
pub struct SpriteIndexHeader {
    pub version: u16,
    pub entry_count: u32,
    pub sheet_count: u16,
    pub thumb_width: u16,
    pub thumb_height: u16,
    pub grid_columns: u8,
    pub grid_rows: u8,
}

impl SpriteIndexHeader {
    /// Serialize header to bytes.
    pub fn to_bytes(&self) -> [u8; SPRITE_INDEX_HEADER_SIZE] {
        let mut buf = [0u8; SPRITE_INDEX_HEADER_SIZE];
        buf[0..4].copy_from_slice(&SPRITE_INDEX_MAGIC);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..10].copy_from_slice(&self.entry_count.to_le_bytes());
        buf[10..12].copy_from_slice(&self.sheet_count.to_le_bytes());
        buf[12..14].copy_from_slice(&self.thumb_width.to_le_bytes());
        buf[14..16].copy_from_slice(&self.thumb_height.to_le_bytes());
        buf[16] = self.grid_columns;
        buf[17] = self.grid_rows;
        buf
    }

    /// Deserialize header from bytes.
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < SPRITE_INDEX_HEADER_SIZE {
            return None;
        }
        if buf[0..4] != SPRITE_INDEX_MAGIC {
            return None;
        }
        Some(Self {
            version: u16::from_le_bytes([buf[4], buf[5]]),
            entry_count: u32::from_le_bytes([buf[6], buf[7], buf[8], buf[9]]),
            sheet_count: u16::from_le_bytes([buf[10], buf[11]]),
            thumb_width: u16::from_le_bytes([buf[12], buf[13]]),
            thumb_height: u16::from_le_bytes([buf[14], buf[15]]),
            grid_columns: buf[16],
            grid_rows: buf[17],
        })
    }
}

/// A single sprite index entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteIndexEntry {
    pub asset_id_hash: u64,
    pub sheet: u16,
    pub x: u8,
    pub y: u8,
}

impl SpriteIndexEntry {
    /// Create a new entry from an asset ID string.
    pub fn new(asset_id: &str, sheet: u16, x: u8, y: u8) -> Self {
        Self {
            asset_id_hash: hash_asset_id(asset_id),
            sheet,
            x,
            y,
        }
    }

    /// Serialize entry to bytes.
    pub fn to_bytes(&self) -> [u8; SPRITE_INDEX_ENTRY_SIZE] {
        let mut buf = [0u8; SPRITE_INDEX_ENTRY_SIZE];
        buf[0..8].copy_from_slice(&self.asset_id_hash.to_le_bytes());
        buf[8..10].copy_from_slice(&self.sheet.to_le_bytes());
        buf[10] = self.x;
        buf[11] = self.y;
        buf
    }

    /// Deserialize entry from bytes.
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < SPRITE_INDEX_ENTRY_SIZE {
            return None;
        }
        Some(Self {
            asset_id_hash: u64::from_le_bytes([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
            ]),
            sheet: u16::from_le_bytes([buf[8], buf[9]]),
            x: buf[10],
            y: buf[11],
        })
    }
}

/// Reader for sprite index files.
pub struct SpriteIndex<'a> {
    header: SpriteIndexHeader,
    data: &'a [u8],
}

impl<'a> SpriteIndex<'a> {
    /// Parse a sprite index from bytes.
    pub fn from_bytes(data: &'a [u8]) -> Option<Self> {
        let header = SpriteIndexHeader::from_bytes(data)?;
        Some(Self { header, data })
    }

    /// Get the header.
    pub fn header(&self) -> &SpriteIndexHeader {
        &self.header
    }

    /// Look up a sprite position by asset ID.
    ///
    /// Returns (sheet, x, y) if found.
    pub fn lookup(&self, asset_id: &str) -> Option<(u16, u8, u8)> {
        let hash = hash_asset_id(asset_id);
        self.lookup_by_hash(hash)
    }

    /// Look up a sprite position by pre-computed hash.
    pub fn lookup_by_hash(&self, hash: u64) -> Option<(u16, u8, u8)> {
        let entry_count = self.header.entry_count as usize;
        if entry_count == 0 {
            return None;
        }

        // Binary search
        let entries_start = SPRITE_INDEX_HEADER_SIZE;
        let mut left = 0;
        let mut right = entry_count;

        while left < right {
            let mid = left + (right - left) / 2;
            let offset = entries_start + mid * SPRITE_INDEX_ENTRY_SIZE;
            let entry_hash = u64::from_le_bytes([
                self.data[offset],
                self.data[offset + 1],
                self.data[offset + 2],
                self.data[offset + 3],
                self.data[offset + 4],
                self.data[offset + 5],
                self.data[offset + 6],
                self.data[offset + 7],
            ]);

            if entry_hash == hash {
                let sheet = u16::from_le_bytes([self.data[offset + 8], self.data[offset + 9]]);
                let x = self.data[offset + 10];
                let y = self.data[offset + 11];
                return Some((sheet, x, y));
            } else if entry_hash < hash {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        None
    }

    /// Get the number of entries.
    pub fn entry_count(&self) -> u32 {
        self.header.entry_count
    }
}

/// Builder for creating sprite index files.
pub struct SpriteIndexBuilder {
    entries: Vec<SpriteIndexEntry>,
    sheet_count: u16,
    thumb_width: u16,
    thumb_height: u16,
    grid_columns: u8,
    grid_rows: u8,
}

impl SpriteIndexBuilder {
    /// Create a new builder.
    pub fn new(
        sheet_count: u16,
        thumb_width: u16,
        thumb_height: u16,
        grid_columns: u8,
        grid_rows: u8,
    ) -> Self {
        Self {
            entries: Vec::new(),
            sheet_count,
            thumb_width,
            thumb_height,
            grid_columns,
            grid_rows,
        }
    }

    /// Add an entry.
    pub fn add(&mut self, asset_id: &str, sheet: u16, x: u8, y: u8) {
        self.entries
            .push(SpriteIndexEntry::new(asset_id, sheet, x, y));
    }

    /// Add an entry with pre-computed hash.
    pub fn add_with_hash(&mut self, asset_id_hash: u64, sheet: u16, x: u8, y: u8) {
        self.entries.push(SpriteIndexEntry {
            asset_id_hash,
            sheet,
            x,
            y,
        });
    }

    /// Build the index, returning the serialized bytes.
    ///
    /// Entries are sorted by hash for binary search.
    pub fn build(mut self) -> Vec<u8> {
        // Sort by hash for binary search
        self.entries.sort_by_key(|e| e.asset_id_hash);

        let header = SpriteIndexHeader {
            version: SPRITE_INDEX_VERSION,
            entry_count: self.entries.len() as u32,
            sheet_count: self.sheet_count,
            thumb_width: self.thumb_width,
            thumb_height: self.thumb_height,
            grid_columns: self.grid_columns,
            grid_rows: self.grid_rows,
        };

        let mut buf = Vec::with_capacity(
            SPRITE_INDEX_HEADER_SIZE + self.entries.len() * SPRITE_INDEX_ENTRY_SIZE,
        );
        buf.extend_from_slice(&header.to_bytes());
        for entry in &self.entries {
            buf.extend_from_slice(&entry.to_bytes());
        }
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_asset_id() {
        let hash1 = hash_asset_id("abc123");
        let hash2 = hash_asset_id("abc123");
        let hash3 = hash_asset_id("def456");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_header_roundtrip() {
        let header = SpriteIndexHeader {
            version: 1,
            entry_count: 1000,
            sheet_count: 5,
            thumb_width: 64,
            thumb_height: 64,
            grid_columns: 4,
            grid_rows: 4,
        };

        let bytes = header.to_bytes();
        let restored = SpriteIndexHeader::from_bytes(&bytes).unwrap();

        assert_eq!(restored.version, 1);
        assert_eq!(restored.entry_count, 1000);
        assert_eq!(restored.sheet_count, 5);
        assert_eq!(restored.thumb_width, 64);
        assert_eq!(restored.thumb_height, 64);
        assert_eq!(restored.grid_columns, 4);
        assert_eq!(restored.grid_rows, 4);
    }

    #[test]
    fn test_entry_roundtrip() {
        let entry = SpriteIndexEntry::new("test_asset_123", 3, 5, 7);
        let bytes = entry.to_bytes();
        let restored = SpriteIndexEntry::from_bytes(&bytes).unwrap();

        assert_eq!(restored, entry);
    }

    #[test]
    fn test_builder_and_lookup() {
        let mut builder = SpriteIndexBuilder::new(2, 64, 64, 4, 4);
        builder.add("asset_a", 0, 1, 2);
        builder.add("asset_b", 0, 3, 4);
        builder.add("asset_c", 1, 0, 0);

        let data = builder.build();
        let index = SpriteIndex::from_bytes(&data).unwrap();

        assert_eq!(index.entry_count(), 3);
        assert_eq!(index.lookup("asset_a"), Some((0, 1, 2)));
        assert_eq!(index.lookup("asset_b"), Some((0, 3, 4)));
        assert_eq!(index.lookup("asset_c"), Some((1, 0, 0)));
        assert_eq!(index.lookup("nonexistent"), None);
    }

    #[test]
    fn test_binary_search_many_entries() {
        let mut builder = SpriteIndexBuilder::new(10, 64, 64, 10, 10);

        // Add 1000 entries
        for i in 0..1000u32 {
            let asset_id = format!("asset_{i:04}");
            let sheet = (i / 100) as u16;
            let x = (i % 10) as u8;
            let y = ((i / 10) % 10) as u8;
            builder.add(&asset_id, sheet, x, y);
        }

        let data = builder.build();
        let index = SpriteIndex::from_bytes(&data).unwrap();

        // Verify lookups
        assert_eq!(index.lookup("asset_0000"), Some((0, 0, 0)));
        assert_eq!(index.lookup("asset_0123"), Some((1, 3, 2)));
        assert_eq!(index.lookup("asset_0999"), Some((9, 9, 9)));
        assert_eq!(index.lookup("not_found"), None);
    }
}
