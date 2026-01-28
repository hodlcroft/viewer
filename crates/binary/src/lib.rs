//! Binary collection format for NFT viewer.
//!
//! This crate defines the `collection.bin` format - a compact binary representation
//! optimized for fast trait filtering, sprite lookups, and HCF image access in WASM.
//!
//! # Format Overview
//!
//! ```text
//! +------------------+
//! | Header (40 bytes)|
//! +------------------+
//! | String Table     |
//! +------------------+
//! | Trait Schema     |
//! +------------------+
//! | Trait Index      |
//! +------------------+
//! | Token Table      |
//! +------------------+
//! | Asset ID PHF     |
//! +------------------+
//! | Sprite Metadata  |
//! +------------------+
//! | HCF Metadata     |
//! +------------------+
//! ```

mod bitmap;
mod error;
mod hcf;
mod header;
mod sources;
mod sprites;
mod string_ref_size;
mod strings;
mod tokens;
mod traits;

pub use bitmap::BitmapSize;
pub use error::BinaryFormatError;
pub use hcf::{HcfIndexSize, HcfMetadata, ImageFormat};
pub use header::{FLAG_HIDE_RARITY, FLAG_MULTI_SOURCE, HEADER_SIZE, Header};
pub use sources::{SourceMetadata, SourcesSection};
pub use sprites::{SPRITE_METADATA_SIZE, SpriteFormat, SpriteMetadata};
pub use string_ref_size::StringRefSize;
pub use strings::{StringRef, StringTable, StringTableBuilder};
pub use tokens::{
    TOKEN_FIXED_SIZE, TOKEN_FIXED_SIZE_MULTI_SOURCE, TokenEntry, read_hcf_location,
    write_hcf_location,
};
pub use traits::{TraitDef, TraitSchema, TraitSchemaBuilder, ValueDef};

/// Magic bytes identifying a collection.bin file
pub const MAGIC: [u8; 4] = *b"COLL";

/// Current format version
pub const VERSION: u16 = 1;
