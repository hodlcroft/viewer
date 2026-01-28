# Collection Binary Format

## Overview

A single binary file (`collection.bin`) containing everything needed for fast trait filtering, sprite lookups, and direct HCF image access, designed to run efficiently in WASM.

**Goals:**
- Single cacheable file per collection
- O(1) asset lookup via perfect hash
- Instant trait filtering via bitmasks
- Direct HCF bundle access without loading HCF index
- Fast autocomplete via sorted string table
- ~350 KB for 10K collection (vs 4.5 MB JSON)

## File Structure

```
+----------------------------------------------------------------+
| Header (40 bytes)                                              |
+----------------------------------------------------------------+
| String Table                                                   |
+----------------------------------------------------------------+
| Trait Schema                                                   |
+----------------------------------------------------------------+
| Trait Index (inverted)                                         |
+----------------------------------------------------------------+
| Token Table                                                    |
+----------------------------------------------------------------+
| Asset ID PHF                                                   |
+----------------------------------------------------------------+
| Sprite Sheets Metadata                                         |
+----------------------------------------------------------------+
| HCF Bundle Metadata                                            |
+----------------------------------------------------------------+
```

## Section Details

### Header (40 bytes)

```rust
struct Header {
    magic: [u8; 4],          // "COLL"
    version: u16,            // Format version
    flags: u16,              // Feature flags
    token_count: u32,        // Number of tokens
    trait_count: u8,         // Number of traits
    bitmap_size: u8,         // BitmapSize enum
    hcf_index_size: u8,      // HcfIndexSize enum
    _reserved: u8,
    
    // Section offsets (from start of file)
    string_table_offset: u32,
    trait_schema_offset: u32,
    trait_index_offset: u32,
    token_table_offset: u32,
    phf_offset: u32,
    sprites_offset: u32,
    hcf_metadata_offset: u32,
}
```

### String Table

Deduplicated strings for trait names, trait values, and collection name. Referenced by 16-bit offsets.

```
+------------------------------------------+
| count: u16                               |
+------------------------------------------+
| offsets: [u16; count]                    | --+
+------------------------------------------+   | offset into data
| data: [u8; ...]                          | --+
|   "Background\0Blue\0Red\0Eyes\0..."     |
+------------------------------------------+
```

Lookup: `strings.get(StringRef(42))` -> pointer arithmetic, zero-copy.

### Trait Schema

Defines the trait structure and maps trait:value pairs to bitmap positions.

```rust
struct TraitSchema {
    traits: [TraitDef; trait_count],
}

struct TraitDef {
    name: StringRef,           // Offset into string table
    value_count: u8,           // Number of possible values
    bitmap_offset: u16,        // Starting bit position in attribute bitmap
    values: [ValueDef; value_count],
}

struct ValueDef {
    name: StringRef,           // Offset into string table
    count: u16,                // Number of tokens with this value
    // percentage derivable: count / token_count * 100
}
```

Example for a collection with 3 traits:
```
Trait 0: "Background" (5 values, bits 0-4)
Trait 1: "Eyes" (8 values, bits 5-12)  
Trait 2: "Clothing" (12 values, bits 13-24)
```

### Trait Index (Inverted)

For each trait:value combination, a list of token indices that have it.

```
+------------------------------------------+
| bucket_offsets: [u32; total_values]      |
+------------------------------------------+
| bucket_lengths: [u16; total_values]      |
+------------------------------------------+
| token_indices: [u16; ...]                |
|   Bucket 0: [0, 15, 42, 99, ...]         |
|   Bucket 1: [1, 2, 50, ...]              |
|   ...                                    |
+------------------------------------------+
```

Filtering flow:
```rust
fn filter_by_trait_value(index: &TraitIndex, trait_id: u8, value_id: u8) -> &[u16] {
    let bucket_id = schema.traits[trait_id].bitmap_offset + value_id;
    let offset = index.bucket_offsets[bucket_id];
    let length = index.bucket_lengths[bucket_id];
    &index.token_indices[offset..offset + length]
}
```

For multi-trait filters, intersect the token index sets.

### Token Table

Fixed-size entries for each token, indexed by token_index (0 to token_count-1). Entry size varies based on `bitmap_size` and `hcf_index_size` in header.

```rust
struct TokenEntry {
    // Sprite location (4 bytes)
    sprite_sheet: u16,
    sprite_x: u8,
    sprite_y: u8,
    
    // Rarity (4 bytes)
    rarity_rank: u16,          // 1 = rarest
    rarity_score: u16,         // Fixed-point (score * 100)
    
    // Name reference (2 bytes)
    // If high bit set: index into custom names table
    // Otherwise: token number for "{Collection} #{n}" pattern
    name_ref: u16,
    
    // Attributes as bitmask (variable size based on bitmap_size)
    attributes: [u8; bitmap_bytes],
    
    // HCF bundle location (variable size based on hcf_index_size)
    hcf_location: [u8; hcf_bytes],
}
```

#### Attribute Bitmap

Each bit represents a trait:value combination:
```
bit 0:  Background = Blue
bit 1:  Background = Red
bit 2:  Background = Green
bit 3:  Eyes = Happy
bit 4:  Eyes = Sad
...
```

Filtering with AND:
```rust
fn matches_filter(token: &TokenEntry, filter_mask: u64) -> bool {
    token.attributes & filter_mask == filter_mask
}

// "Background:Blue AND Eyes:Happy"
let filter = (1 << 0) | (1 << 3);
let matches: Vec<_> = tokens.iter()
    .enumerate()
    .filter(|(_, t)| matches_filter(t, filter))
    .collect();
```

#### Bitmap Sizing

Header specifies bitmap type, chosen during ingestion based on trait:value count:

```rust
#[repr(u8)]
enum BitmapSize {
    U64  = 0,  // <= 64 values,  8 bytes
    U128 = 1,  // <= 128 values, 16 bytes
    U256 = 2,  // <= 256 values, 32 bytes
    U512 = 3,  // <= 512 values, 64 bytes
}
```

The sync CLI analyzes the collection and selects the smallest size that fits. Collections exceeding 512 trait:value combinations must use the `traits.ignore` config to exclude high-cardinality traits.

#### HCF Index Sizing

Header specifies HCF location size, chosen during ingestion based on total bundle size and max image size:

```rust
#[repr(u8)]
enum HcfIndexSize {
    U32_U16 = 0,  // offset: u32, length: u16 - 6 bytes (up to 4GB total, 64KB per image)
    U32_U24 = 1,  // offset: u32, length: u24 - 7 bytes (up to 4GB total, 16MB per image)
    U40_U24 = 2,  // offset: u40, length: u24 - 8 bytes (up to 1TB total, 16MB per image)
}
```

| Collection Profile | Total HCF Size | Max Image | Index Size | Per Token |
|--------------------|----------------|-----------|------------|-----------|
| 10K @ 50KB WebP | 500 MB | ~100 KB | U32_U16 | 6 bytes |
| 10K @ 100KB WebP | 1 GB | ~200 KB | U32_U24 | 7 bytes |
| 25K @ 150KB WebP | 3.75 GB | ~300 KB | U32_U24 | 7 bytes |
| 50K+ large images | > 4 GB | > 64 KB | U40_U24 | 8 bytes |

Most 10K WebP collections fit in **U32_U16 (6 bytes per token)**.

#### Token Entry Size Examples

| Bitmap | HCF Index | Fixed Fields | Total | 10K Tokens |
|--------|-----------|--------------|-------|------------|
| U64 (8) | U32_U16 (6) | 10 | 24 bytes | 240 KB |
| U64 (8) | U32_U24 (7) | 10 | 25 bytes | 250 KB |
| U128 (16) | U32_U16 (6) | 10 | 32 bytes | 320 KB |
| U256 (32) | U32_U24 (7) | 10 | 49 bytes | 490 KB |

### HCF Bundle Metadata

Information needed to construct HCF bundle URLs and perform range requests.

```rust
struct HcfMetadata {
    shard_size: u32,           // Fixed shard size in bytes (e.g., 250 MB)
    shard_count: u16,          // Number of HCF bundle shards
    image_format: u8,          // 0 = webp, 1 = png, 2 = avif
    max_dimension: u16,        // Max width/height (e.g., 2048)
    _reserved: [u8; 3],
}
```

#### HCF Shard Calculation

HCF bundles are fixed-size shards (e.g., 250 MB each), zero-padded. Given a global byte offset:

```rust
fn locate_in_hcf(global_offset: u64, shard_size: u32) -> (shard_index: u32, offset_in_shard: u32) {
    let shard_index = (global_offset / shard_size as u64) as u32;
    let offset_in_shard = (global_offset % shard_size as u64) as u32;
    (shard_index, offset_in_shard)
}

// Construct URL: /cardano/{policy_id}/images_{shard_index:03}.hcf
// Range request: bytes={offset_in_shard}-{offset_in_shard + length - 1}
```

### Asset ID PHF

Perfect hash function mapping asset_id strings to token indices.

```
+------------------------------------------+
| phf_seed: u64                            |
| phf_data: [u8; ...]                      |  Algorithm-specific
+------------------------------------------+
| verification: [u64; token_count]         |  Hash of each asset_id
+------------------------------------------+
```

Lookup:
```rust
fn get_token_index(phf: &AssetIdPhf, asset_id: &str) -> Option<u16> {
    let idx = phf.hash(asset_id);
    let expected_hash = phf.verification[idx];
    if hash64(asset_id) == expected_hash {
        Some(idx as u16)
    } else {
        None  // asset_id not in collection
    }
}
```

### Sprite Sheets Metadata

```rust
struct SpriteMetadata {
    thumb_size: u16,           // e.g., 150
    columns: u8,               // e.g., 10
    rows: u8,                  // e.g., 10
    sheet_count: u16,          // e.g., 100 for 10K tokens
    format: u8,                // 0 = webp, 1 = png, 2 = avif
}
```

## Size Budget (10K Collection, typical PFP)

Assuming: 10 traits, 100 total trait:values (U64 bitmap), WebP images averaging 80KB (U32_U16 HCF index)

| Section | Calculation | Size |
|---------|-------------|------|
| Header | Fixed | 40 B |
| String table | ~100 strings x 12 chars + overhead | ~2 KB |
| Trait schema | 10 traits x 10 values x 6 bytes | ~600 B |
| Trait index | 100 buckets, avg 100 tokens each x 2 bytes | ~20 KB |
| Token table | 10K x 24 bytes | 240 KB |
| PHF + verification | ~3KB + 10K x 8 bytes | ~83 KB |
| Sprites metadata | Fixed | 8 B |
| HCF metadata | Fixed | 12 B |
| **Total** | | **~345 KB** |

With compression (zstd): **~120-180 KB**

## API

```rust
pub struct Collection {
    data: Vec<u8>,  // or mmap
}

impl Collection {
    /// Load from bytes
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, Error>;
    
    /// Get sprite coordinates for an asset
    pub fn get_sprite(&self, asset_id: &str) -> Option<SpriteCoords>;
    
    /// Get full token details
    pub fn get_token(&self, asset_id: &str) -> Option<TokenView>;
    
    /// Get token by index (after filtering)
    pub fn get_token_by_index(&self, index: u16) -> TokenView;
    
    /// Get HCF location for direct image fetch
    pub fn get_hcf_location(&self, asset_id: &str) -> Option<HcfLocation>;
    
    /// Filter tokens by trait:value pairs (AND logic)
    pub fn filter(&self, filters: &[(u8, u8)]) -> FilteredTokens;
    
    /// Search trait values by prefix (for autocomplete)
    pub fn search_values(&self, prefix: &str) -> Vec<TraitValueMatch>;
    
    /// Get all trait definitions (for filter UI)
    pub fn traits(&self) -> &[TraitDef];
    
    /// Collection metadata
    pub fn name(&self) -> &str;
    pub fn token_count(&self) -> u32;
    
    /// HCF bundle info
    pub fn hcf_metadata(&self) -> &HcfMetadata;
}

/// HCF bundle location for direct range requests
pub struct HcfLocation {
    pub shard_index: u32,
    pub offset: u32,
    pub length: u32,
}

impl HcfLocation {
    /// Construct the HCF shard URL
    pub fn shard_url(&self, base_url: &str, policy_id: &str) -> String {
        format!("{}/cardano/{}/images_{:03}.hcf", base_url, policy_id, self.shard_index)
    }
    
    /// HTTP Range header value
    pub fn range_header(&self) -> String {
        format!("bytes={}-{}", self.offset, self.offset + self.length - 1)
    }
}

/// Zero-copy view into a token
pub struct TokenView<'a> {
    pub index: u16,
    pub sprite: SpriteCoords,
    pub rarity_rank: u16,
    pub rarity_percentile: f32,
    pub attributes: AttributeIter<'a>,
    pub name: &'a str,
    pub hcf: HcfLocation,
}

/// Filtered result with efficient iteration
pub struct FilteredTokens<'a> {
    collection: &'a Collection,
    indices: Vec<u16>,
}

impl<'a> FilteredTokens<'a> {
    pub fn count(&self) -> usize;
    pub fn iter(&self) -> impl Iterator<Item = TokenView<'a>>;
}
```

## File Naming & Storage

```
/{chain}/{policy_id}/
  collection.bin          -> trait filtering, lookups
  sprites_000.webp        -> thumbnail grid (100 tokens each)
  sprites_001.webp
  ...
  images_000.hcf          -> full-size images (250 MB shards)
  images_001.hcf
  ...
```

All files are immutable and cache-forever. Re-sync generates new version:
```
/cardano/{policy_id}/v3/collection.bin
```

## Ingestion Configs

Per-collection configuration files control how traits are processed during sync.

### File Location

```
configs/
  cardano/
    {policy_id}.toml
```

### Config Format

```toml
# Display name (optional, defaults to on-chain name)
name = "Toolheads"

# Data source
[source]
type = "cnft_tools"  # or "maestro"

# Traits to exclude from filtering/indexing
[traits]
ignore = [
    "Call Sign",      # Unique per token
    "Serial Number",  # Unique per token
]

# Trait name aliases for cleaner display
[traits.aliases]
"BACKGROUND" = "Background"
"BODY_TYPE" = "Body Type"

# Rarity configuration
[rarity]
use_source = true     # Use CNFT.tools rarity if available
exclude = ["Call Sign"]  # Exclude from rarity calculation if recalculating
```

### Behavior

1. **No config file**: All traits included, sync fails if >512 trait:value combinations
2. **With ignore list**: Excluded traits still stored in token details but not indexed for filtering
3. **With aliases**: Display names normalized in UI, original names preserved in data

## WASM Considerations

1. **Memory mapping**: In WASM, load entire file into linear memory. Use typed array views for zero-copy access.

2. **Alignment**: Ensure struct layouts are naturally aligned for direct casting.

3. **Endianness**: Use little-endian (native for WASM).

4. **String handling**: UTF-8 strings with null terminators for easy slicing.

5. **Variable-size entries**: Read `bitmap_size` and `hcf_index_size` from header to calculate token entry stride.

## Future Extensions

- **Delta updates**: For live collections, support incremental syncs
- **Compressed sprites**: Embed low-res sprite data directly (for offline/preview)
- **Provenance data**: On-chain tx hashes, mint dates, etc.
- **Multi-chain**: Extend to Ethereum, Solana with chain-specific PHF keys
