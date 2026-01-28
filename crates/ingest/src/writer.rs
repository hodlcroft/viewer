//! Binary format writer for collection.bin.
//!
//! Assembles all the pieces into the final binary format:
//! - Header with offsets to all sections
//! - String table
//! - Trait schema
//! - Trait index (inverted)
//! - Token table
//! - PHF data (for asset ID lookup)
//! - Sprite metadata
//! - HCF metadata
//! - Sources section
//!
//! Supports two-pass construction:
//! 1. Pass 1: Build everything except HCF locations (traits, sprites, rarity)
//! 2. Pass 2: Add HCF locations and finalize

use std::io::{self, Write};
use std::path::Path;

use thiserror::Error;
use viewer_binary::{
    BinaryFormatError, BitmapSize, HEADER_SIZE, HcfIndexSize, HcfMetadata, Header, SourcesSection,
    StringTableBuilder, TokenEntry, TraitSchemaBuilder,
};

use crate::bundle::ImageLocation;
use crate::sprites::SpriteLocation;

/// Binary format writer errors.
#[derive(Debug, Error)]
pub enum WriterError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Binary format error: {0}")]
    BinaryFormat(#[from] BinaryFormatError),

    #[error("Too many traits: {count} (max 255)")]
    TooManyTraits { count: usize },

    #[error("Too many trait values: {count} (max 512)")]
    TooManyValues { count: usize },

    #[error("Mismatched token count: expected {expected}, got {actual}")]
    TokenCountMismatch { expected: usize, actual: usize },

    #[error("HCF locations not set - call finalize_with_hcf first")]
    HcfNotFinalized,
}

/// Token data for writing to binary format.
///
/// Note: HCF locations are stored in a separate index section, not with tokens.
/// This allows the token table to be built before HCF bundling is complete.
#[derive(Debug, Clone)]
pub struct TokenData {
    /// Token name (for string table)
    pub name: String,
    /// Asset ID (for PHF)
    pub asset_id: String,
    /// Encoded asset name (hex) - used for deterministic ordering
    pub encoded_name: String,
    /// Trait values as (trait_index, value_index) pairs
    pub traits: Vec<(u8, u8)>,
    /// Rarity rank (1-based)
    pub rarity_rank: u16,
    /// Rarity score (multiplied by 100 for fixed-point)
    pub rarity_score: u16,
    /// Sprite location
    pub sprite: SpriteLocation,
    /// Source index (for multi-source collections)
    pub source_index: Option<u8>,
}

/// Internal token data with resolved string references.
#[derive(Debug, Clone)]
struct ResolvedToken {
    /// Name string reference
    name_ref: u16,
    /// Asset ID stored as raw bytes (not in string table - they're unique anyway)
    asset_id: String,
    /// Trait values as (trait_index, value_index) pairs
    traits: Vec<(u8, u8)>,
    /// Rarity rank (1-based)
    rarity_rank: u16,
    /// Rarity score (multiplied by 100 for fixed-point)
    rarity_score: u16,
    /// Sprite location
    sprite: SpriteLocation,
    /// Source index (for multi-source collections)
    source_index: Option<u8>,
}

/// Builder for collection.bin files.
pub struct CollectionWriter {
    /// String table builder
    strings: StringTableBuilder,
    /// Trait schema builder
    traits: TraitSchemaBuilder,
    /// Token data with resolved string references
    tokens: Vec<ResolvedToken>,
    /// Sources section
    sources: SourcesSection,
    /// HCF metadata
    hcf_metadata: HcfMetadata,
    /// Bitmap size for traits
    bitmap_size: BitmapSize,
    /// HCF index size
    hcf_index_size: HcfIndexSize,
    /// Hide rarity rankings in the UI
    hide_rarity: bool,
}

impl CollectionWriter {
    /// Create a new collection writer.
    ///
    /// Returns None if total_trait_values exceeds 512.
    pub fn new(
        sources: SourcesSection,
        hcf_metadata: HcfMetadata,
        total_trait_values: usize,
        total_hcf_size: u64,
        max_image_size: u32,
    ) -> Option<Self> {
        Self::with_options(
            sources,
            hcf_metadata,
            total_trait_values,
            total_hcf_size,
            max_image_size,
            false,
        )
    }

    /// Create a new collection writer with additional options.
    ///
    /// Returns None if total_trait_values exceeds 512.
    pub fn with_options(
        sources: SourcesSection,
        hcf_metadata: HcfMetadata,
        total_trait_values: usize,
        total_hcf_size: u64,
        max_image_size: u32,
        hide_rarity: bool,
    ) -> Option<Self> {
        let bitmap_size = BitmapSize::for_count(total_trait_values)?;
        let hcf_index_size = HcfIndexSize::for_sizes(total_hcf_size, max_image_size);

        Some(Self {
            strings: StringTableBuilder::new(),
            traits: TraitSchemaBuilder::new(),
            tokens: Vec::new(),
            sources,
            hcf_metadata,
            bitmap_size,
            hcf_index_size,
            hide_rarity,
        })
    }

    /// Add a trait definition with values.
    ///
    /// Values are provided as (name, count) pairs.
    pub fn add_trait(&mut self, name: &str, values: &[(&str, u16)]) -> Result<(), WriterError> {
        let name_ref = self.strings.add(name)?;

        let mut value_refs = Vec::with_capacity(values.len());
        for (v, count) in values {
            let v_ref = self.strings.add(v)?;
            value_refs.push((v_ref, *count));
        }

        self.traits.add_trait(name_ref, value_refs)?;
        Ok(())
    }

    /// Add a token.
    ///
    /// Returns an error if the name cannot be added to the string table.
    pub fn add_token(&mut self, token: TokenData) -> Result<(), WriterError> {
        // Add name to string table (with high bit set to indicate custom name)
        let name_ref = self.strings.add(&token.name)?.0 | 0x8000;

        // Asset ID is stored separately (not in string table - they're unique anyway)
        self.tokens.push(ResolvedToken {
            name_ref,
            asset_id: token.asset_id,
            traits: token.traits,
            rarity_rank: token.rarity_rank,
            rarity_score: token.rarity_score,
            sprite: token.sprite,
            source_index: token.source_index,
        });

        Ok(())
    }

    /// Write the collection to a file.
    ///
    /// `hcf_locations` must be in the same order as tokens were added.
    pub fn write_to_file(
        &self,
        path: &Path,
        hcf_locations: &[ImageLocation],
    ) -> Result<(), WriterError> {
        if hcf_locations.len() != self.tokens.len() {
            return Err(WriterError::TokenCountMismatch {
                expected: self.tokens.len(),
                actual: hcf_locations.len(),
            });
        }

        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        self.write(&mut writer, hcf_locations)?;
        writer.flush()?;
        Ok(())
    }

    /// Write the collection to a file without HCF locations (pass 1).
    ///
    /// This writes a valid collection.bin with placeholder HCF locations (0, 0).
    /// Use this to establish deterministic ordering before sprite/HCF generation.
    /// Call `write_to_file` later with actual HCF locations for the final version.
    pub fn write_to_file_without_hcf(&self, path: &Path) -> Result<(), WriterError> {
        // Create placeholder locations with zeros
        let placeholder_locations: Vec<ImageLocation> = (0..self.tokens.len())
            .map(|_| ImageLocation {
                global_offset: 0,
                length: 0,
                shard_index: 0,
                shard_offset: 0,
            })
            .collect();

        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        self.write(&mut writer, &placeholder_locations)?;
        writer.flush()?;
        Ok(())
    }

    /// Write the collection to a writer.
    ///
    /// `hcf_locations` must be in the same order as tokens were added.
    pub fn write(
        &self,
        writer: &mut impl Write,
        hcf_locations: &[ImageLocation],
    ) -> Result<(), WriterError> {
        // Build string table (clone since build consumes)
        let string_table = self.strings.clone().build()?;
        let string_table_bytes = string_table.to_bytes();

        // Build trait schema (clone since build consumes)
        let trait_schema = self.traits.clone().build();
        let trait_schema_bytes = trait_schema.to_bytes();

        // Calculate section offsets
        let string_table_offset = HEADER_SIZE as u32;
        let trait_schema_offset = string_table_offset + string_table_bytes.len() as u32;
        let trait_index_offset = trait_schema_offset + trait_schema_bytes.len() as u32;

        // For now, trait index is empty (would contain inverted index)
        let trait_index_bytes: Vec<u8> = Vec::new();

        let token_table_offset = trait_index_offset + trait_index_bytes.len() as u32;

        // Build token table (without HCF - that's separate now)
        let multi_source = self.sources.is_multi_source();
        let token_table_bytes = self.build_token_table(multi_source)?;

        let phf_offset = token_table_offset + token_table_bytes.len() as u32;

        // PHF data (placeholder - would contain perfect hash function data)
        let phf_bytes: Vec<u8> = Vec::new();

        let sprites_offset = phf_offset + phf_bytes.len() as u32;

        // Sprite metadata (placeholder)
        let sprites_bytes: Vec<u8> = Vec::new();

        let hcf_metadata_offset = sprites_offset + sprites_bytes.len() as u32;
        let hcf_metadata_bytes = self.hcf_metadata.to_bytes();

        // HCF index section (array of offset/length per token)
        let hcf_index_offset = hcf_metadata_offset + hcf_metadata_bytes.len() as u32;
        let hcf_index_bytes = self.build_hcf_index(hcf_locations);

        let sources_offset = hcf_index_offset + hcf_index_bytes.len() as u32;
        let sources_bytes = self.sources.to_bytes();

        // Asset ID index section (array of u16 string refs, one per token)
        let asset_id_index_offset = sources_offset + sources_bytes.len() as u32;
        let asset_id_index_bytes = self.build_asset_id_index();

        // Build header
        let mut header = Header::new(
            self.tokens.len() as u32,
            self.traits.trait_count() as u8,
            self.bitmap_size,
            self.hcf_index_size,
            self.sources.count() as u8,
        );
        if self.hide_rarity {
            header.flags |= viewer_binary::FLAG_HIDE_RARITY;
        }
        header.string_table_offset = string_table_offset;
        header.trait_schema_offset = trait_schema_offset;
        header.trait_index_offset = trait_index_offset;
        header.token_table_offset = token_table_offset;
        header.phf_offset = phf_offset;
        header.sprites_offset = sprites_offset;
        header.hcf_metadata_offset = hcf_metadata_offset;
        header.hcf_index_offset = hcf_index_offset;
        header.sources_offset = sources_offset;
        header.asset_id_index_offset = asset_id_index_offset;

        // Write all sections
        writer.write_all(&header.to_bytes())?;
        writer.write_all(&string_table_bytes)?;
        writer.write_all(&trait_schema_bytes)?;
        writer.write_all(&trait_index_bytes)?;
        writer.write_all(&token_table_bytes)?;
        writer.write_all(&phf_bytes)?;
        writer.write_all(&sprites_bytes)?;
        writer.write_all(&hcf_metadata_bytes)?;
        writer.write_all(&hcf_index_bytes)?;
        writer.write_all(&sources_bytes)?;
        writer.write_all(&asset_id_index_bytes)?;

        Ok(())
    }

    /// Build the token table bytes (without HCF locations).
    fn build_token_table(&self, multi_source: bool) -> Result<Vec<u8>, WriterError> {
        let entry_size = TokenEntry::entry_size(self.bitmap_size, multi_source);
        let mut bytes = Vec::with_capacity(self.tokens.len() * entry_size);

        for token in &self.tokens {
            let entry = TokenEntry {
                source_index: token.source_index,
                sprite_sheet: token.sprite.sheet,
                sprite_x: token.sprite.x,
                sprite_y: token.sprite.y,
                rarity_rank: token.rarity_rank,
                rarity_score: token.rarity_score,
                name_ref: token.name_ref,
            };

            // Write fixed fields
            let fixed_size = if multi_source {
                viewer_binary::TOKEN_FIXED_SIZE_MULTI_SOURCE
            } else {
                viewer_binary::TOKEN_FIXED_SIZE
            };
            let mut entry_bytes = vec![0u8; entry_size];
            entry.write_fixed(&mut entry_bytes[..fixed_size], multi_source);

            // Write bitmap (trait values encoded as bits)
            let bitmap_offset = fixed_size;
            let bitmap = self.encode_traits_bitmap(&token.traits);
            entry_bytes[bitmap_offset..bitmap_offset + self.bitmap_size.byte_size()]
                .copy_from_slice(&bitmap[..self.bitmap_size.byte_size()]);

            bytes.extend_from_slice(&entry_bytes);
        }

        Ok(bytes)
    }

    /// Build the HCF index section (array of offset/length per token).
    fn build_hcf_index(&self, locations: &[ImageLocation]) -> Vec<u8> {
        let entry_size = self.hcf_index_size.byte_size();
        let mut bytes = Vec::with_capacity(locations.len() * entry_size);

        for loc in locations {
            let mut entry = vec![0u8; entry_size];
            viewer_binary::write_hcf_location(
                loc.global_offset,
                loc.length,
                &mut entry,
                self.hcf_index_size,
            );
            bytes.extend_from_slice(&entry);
        }

        bytes
    }

    /// Encode trait values into a bitmap.
    fn encode_traits_bitmap(&self, traits: &[(u8, u8)]) -> Vec<u8> {
        let mut bitmap = vec![0u8; 64]; // Max U512 size

        for (trait_idx, value_idx) in traits {
            // Calculate bit position: sum of all values before this trait + value_idx
            let bit_pos = self.traits.value_offset(*trait_idx as usize) + *value_idx as usize;
            let byte_idx = bit_pos / 8;
            let bit_idx = bit_pos % 8;
            if byte_idx < bitmap.len() {
                bitmap[byte_idx] |= 1 << bit_idx;
            }
        }

        bitmap
    }

    /// Build the asset ID index section.
    ///
    /// Format: [offset_table: u32 * token_count][string_data]
    /// Each offset points to a null-terminated string in the string_data section.
    /// This avoids bloating the main string table with unique asset IDs.
    fn build_asset_id_index(&self) -> Vec<u8> {
        // First pass: calculate offsets and total size
        let offset_table_size = self.tokens.len() * 4; // u32 per token
        let mut string_data = Vec::new();
        let mut offsets = Vec::with_capacity(self.tokens.len());

        for token in &self.tokens {
            offsets.push((offset_table_size + string_data.len()) as u32);
            string_data.extend_from_slice(token.asset_id.as_bytes());
            string_data.push(0); // null terminator
        }

        // Build final buffer: offsets followed by string data
        let mut bytes = Vec::with_capacity(offset_table_size + string_data.len());
        for offset in offsets {
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        bytes.extend_from_slice(&string_data);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use viewer_binary::{HcfMetadata, SourceMetadata, SourcesSection, StringRef};

    fn create_test_sources() -> SourcesSection {
        SourcesSection::new(vec![SourceMetadata {
            chain: StringRef(0),
            id: StringRef(1),
            token_count: 100,
            synced_at: 1706400000,
        }])
    }

    fn create_test_hcf_metadata() -> HcfMetadata {
        HcfMetadata {
            shard_size: 250_000_000,
            shard_count: 1,
            image_format: viewer_binary::ImageFormat::WebP,
            max_dimension: 2048,
        }
    }

    #[test]
    fn test_writer_basic() {
        let sources = create_test_sources();
        let hcf_meta = create_test_hcf_metadata();

        let mut writer = CollectionWriter::new(sources, hcf_meta, 50, 1_000_000, 50_000).unwrap();

        writer
            .add_trait("Background", &[("Red", 10), ("Blue", 20), ("Green", 15)])
            .unwrap();
        writer
            .add_trait("Eyes", &[("Open", 30), ("Closed", 15)])
            .unwrap();

        let token = TokenData {
            name: "Token #1".to_string(),
            asset_id: "asset123".to_string(),
            encoded_name: "546f6b656e2331".to_string(),
            traits: vec![(0, 1), (1, 0)], // Blue background, Open eyes
            rarity_rank: 1,
            rarity_score: 100,
            sprite: SpriteLocation {
                sheet: 0,
                x: 0,
                y: 0,
            },
            source_index: None,
        };

        writer.add_token(token).unwrap();

        // HCF locations are provided separately
        let hcf_locations = vec![ImageLocation {
            global_offset: 0,
            length: 1000,
            shard_index: 0,
            shard_offset: 0,
        }];

        let mut output = Vec::new();
        writer.write(&mut output, &hcf_locations).unwrap();

        // Verify header magic
        assert_eq!(&output[0..4], b"COLL");

        // Verify header size
        assert!(output.len() >= HEADER_SIZE);
    }
}
