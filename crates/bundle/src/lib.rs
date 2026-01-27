//! Bundle format for NFT viewer.
//!
//! This crate provides types and utilities for creating and reading bundles,
//! which consist of:
//! - `index.json` - Token ID to image offset/length mapping + sprite info
//! - `asset_details.json` - Collection metadata, per-token attributes, and rarity
//! - `images_XXX.bin` - Sharded concatenated image data
//! - `sprites_XXX.webp` - Thumbnail sprite sheets
//!
//! Designed for use with Cloudflare KV (index/asset_details) and R2 (images/sprites)
//! for fast random-access serving.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

mod error;
pub use error::BundleError;

/// Current bundle format version
pub const FORMAT_VERSION: u32 = 1;

// ============================================================================
// Index Types (stored in KV)
// ============================================================================

/// Index file mapping token IDs to image locations in the binary shards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleIndex {
    /// Format version for compatibility
    pub version: u32,
    /// Default image format: "png", "jpg", or "webp"
    pub image_format: String,
    /// Total number of images
    pub image_count: u32,
    /// Number of shard files (images_000.bin through images_NNN.bin)
    #[serde(default = "default_shard_count")]
    pub shard_count: u32,
    /// Sprite sheet configuration (if generated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprites: Option<SpriteConfig>,
    /// Map of token ID to index entry
    pub entries: HashMap<String, IndexEntry>,
}

fn default_shard_count() -> u32 {
    1
}

/// Configuration for sprite sheets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteConfig {
    /// Thumbnail size (width and height, square)
    pub thumb_size: u32,
    /// Number of columns per sprite sheet
    pub columns: u32,
    /// Number of rows per sprite sheet
    pub rows: u32,
    /// Number of sprite sheet files
    pub sheet_count: u32,
    /// Image format for sprites (typically "webp")
    pub format: String,
}

impl SpriteConfig {
    /// Create a standard 10x10 sprite config
    pub fn standard(sheet_count: u32) -> Self {
        Self {
            thumb_size: 300,
            columns: 10,
            rows: 10,
            sheet_count,
            format: "webp".to_string(),
        }
    }

    /// Tokens per sprite sheet
    pub fn tokens_per_sheet(&self) -> u32 {
        self.columns * self.rows
    }
}

/// Location of a single image in the sharded binary files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Byte offset from start of the shard file
    pub offset: u64,
    /// Byte length of image data
    pub length: u32,
    /// Short hash for cache busting (8 hex chars)
    pub hash: String,
    /// Shard index (0 = images_000.bin, 1 = images_001.bin, etc.)
    #[serde(default)]
    pub shard: u32,
    /// Sprite sheet index
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_sheet: Option<u32>,
    /// Column position in sprite sheet (0-indexed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_x: Option<u32>,
    /// Row position in sprite sheet (0-indexed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_y: Option<u32>,
}

impl BundleIndex {
    /// Create a new empty index
    pub fn new(image_format: &str) -> Self {
        Self {
            version: FORMAT_VERSION,
            image_format: image_format.to_string(),
            image_count: 0,
            shard_count: 1,
            sprites: None,
            entries: HashMap::new(),
        }
    }

    /// Set sprite configuration
    pub fn set_sprites(&mut self, config: SpriteConfig) {
        self.sprites = Some(config);
    }

    /// Set the shard count
    pub fn set_shard_count(&mut self, count: u32) {
        self.shard_count = count;
    }

    /// Add an entry to the index
    pub fn add_entry(&mut self, token_id: &str, entry: IndexEntry) {
        self.entries.insert(token_id.to_string(), entry);
        self.image_count = self.entries.len() as u32;
    }

    /// Get entry for a token ID
    pub fn get(&self, token_id: &str) -> Option<&IndexEntry> {
        self.entries.get(token_id)
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, BundleError> {
        serde_json::to_string_pretty(self).map_err(BundleError::Serialize)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, BundleError> {
        serde_json::from_str(json).map_err(BundleError::Deserialize)
    }

    /// Write to file
    pub fn write_to_file(&self, path: &Path) -> Result<(), BundleError> {
        let json = self.to_json()?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Read from file
    pub fn read_from_file(path: &Path) -> Result<Self, BundleError> {
        let json = std::fs::read_to_string(path)?;
        Self::from_json(&json)
    }
}

// ============================================================================
// Utilities
// ============================================================================

/// Compute a short hash (8 hex chars) of data using FNV-1a
pub fn compute_short_hash(data: &[u8]) -> String {
    // FNV-1a hash
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:08x}", hash as u32)
}

/// Format a shard filename
pub fn shard_filename(index: u32) -> String {
    format!("images_{:03}.bin", index)
}

/// Format a sprite sheet filename
pub fn sprite_filename(index: u32) -> String {
    format!("sprites_{:03}.webp", index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_hash() {
        let hash1 = compute_short_hash(b"hello world");
        let hash2 = compute_short_hash(b"hello world");
        let hash3 = compute_short_hash(b"different data");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 8);
    }

    #[test]
    fn test_index_roundtrip() {
        let mut index = BundleIndex::new("webp");
        index.add_entry(
            "000001",
            IndexEntry {
                offset: 0,
                length: 1000,
                hash: "abc12345".to_string(),
                shard: 0,
                sprite_sheet: Some(0),
                sprite_x: Some(0),
                sprite_y: Some(0),
            },
        );

        let json = index.to_json().unwrap();
        let parsed = BundleIndex::from_json(&json).unwrap();

        assert_eq!(parsed.image_count, 1);
        assert_eq!(parsed.get("000001").unwrap().length, 1000);
    }

    #[test]
    fn test_filenames() {
        assert_eq!(shard_filename(0), "images_000.bin");
        assert_eq!(shard_filename(5), "images_005.bin");
        assert_eq!(sprite_filename(0), "sprites_000.webp");
    }
}
