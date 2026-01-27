# Collection Binary Format

## Overview

A single binary file (`collection.bin`) containing everything needed for fast trait filtering and sprite lookups, designed to run efficiently in WASM.

**Goals:**
- Single cacheable file per collection
- O(1) asset lookup via perfect hash
- Instant trait filtering via bitmasks
- Fast autocomplete via sorted string table
- ~190 KB for 10K collection (vs 4.5 MB JSON)

## File Structure

```
┌────────────────────────────────────────────────────────────────┐
│ Header (32 bytes)                                              │
├────────────────────────────────────────────────────────────────┤
│ String Table                                                   │
├────────────────────────────────────────────────────────────────┤
│ Trait Schema                                                   │
├────────────────────────────────────────────────────────────────┤
│ Trait Index (inverted)                                         │
├────────────────────────────────────────────────────────────────┤
│ Token Table                                                    │
├────────────────────────────────────────────────────────────────┤
│ Asset ID PHF                                                   │
├────────────────────────────────────────────────────────────────┤
│ Sprite Sheets Metadata                                         │
└────────────────────────────────────────────────────────────────┘
```

## Section Details

### Header (32 bytes)

```rust
struct Header {
    magic: [u8; 4],          // "COLL"
    version: u16,            // Format version
    flags: u16,              // Feature flags
    token_count: u32,        // Number of tokens
    trait_count: u8,         // Number of traits
    max_values_per_trait: u8,// For bitmap sizing
    _reserved: [u8; 2],
    
    // Section offsets (from start of file)
    string_table_offset: u32,
    trait_schema_offset: u32,
    trait_index_offset: u32,
    token_table_offset: u32,
    phf_offset: u32,
    sprites_offset: u32,
}
```

### String Table

Deduplicated strings for trait names, trait values, and collection name. Referenced by 16-bit offsets.

```
┌─────────────────────────────────────────┐
│ count: u16                              │
├─────────────────────────────────────────┤
│ offsets: [u16; count]                   │  ─┐
├─────────────────────────────────────────┤   │ offset into data
│ data: [u8; ...]                         │  ─┘
│   "Background\0Blue\0Red\0Eyes\0..."    │
└─────────────────────────────────────────┘
```

Lookup: `strings.get(StringRef(42))` → pointer arithmetic, zero-copy.

### Trait Schema

Defines the trait structure and maps trait:value pairs to bitmap positions.

```rust
struct TraitSchema {
    traits: [TraitDef; trait_count],
}

struct TraitDef {
    name: StringRef,           // Offset into string table
    value_count: u8,           // Number of possible values
    bitmap_offset: u8,         // Starting bit position in attribute bitmap
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
┌─────────────────────────────────────────┐
│ bucket_offsets: [u32; total_values]     │
├─────────────────────────────────────────┤
│ bucket_lengths: [u16; total_values]     │
├─────────────────────────────────────────┤
│ token_indices: [u16; ...]               │
│   Bucket 0: [0, 15, 42, 99, ...]        │
│   Bucket 1: [1, 2, 50, ...]             │
│   ...                                   │
└─────────────────────────────────────────┘
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

Fixed-size entries for each token, indexed by token_index (0 to token_count-1).

```rust
struct TokenEntry {
    // Sprite location (4 bytes)
    sprite_sheet: u16,
    sprite_x: u8,
    sprite_y: u8,
    
    // Rarity (4 bytes)
    rarity_rank: u16,          // 1 = rarest
    rarity_score: u16,         // Fixed-point (score * 100)
    
    // Attributes as bitmask (8 bytes for ≤64 trait:value combos)
    attributes: u64,
    
    // Name reference (2 bytes)
    // If high bit set: index into custom names table
    // Otherwise: token number for "{Collection} #{n}" pattern
    name_ref: u16,
}
// Total: 18 bytes per token
```

For 10K tokens: 180 KB

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

#### Handling Large Trait:Value Spaces

Collections vary wildly in trait complexity:

| Scenario | Trait:Value Combos | Bitmap Size |
|----------|-------------------|-------------|
| Simple PFP | 30-50 | u64 (8 bytes) |
| Complex PFP | 100-200 | u128 or [u64; 2] (16 bytes) |
| Generative art | 200-500 | [u64; 4] to [u64; 8] (32-64 bytes) |
| With unique traits | 10,000+ | **Must filter traits** |

**The problem**: Some collections have traits like "Call Sign" or "Serial Number" that are unique per token. Including these would require 10K+ bits per token, destroying our size advantage.

**Solution**: Ingestion configs specify which traits to ignore.

#### Bitmap Sizing

Header specifies bitmap type:

```rust
enum BitmapSize {
    U64,           // ≤64 values
    U128,          // ≤128 values  
    U256,          // ≤256 values (4 × u64)
    U512,          // ≤512 values (8 × u64)
    // Beyond 512: must use trait filtering in config
}
```

The sync CLI will error if a collection exceeds the bitmap capacity after applying ignore rules.

### Asset ID PHF

Perfect hash function mapping asset_id strings to token indices.

```
┌─────────────────────────────────────────┐
│ phf_seed: u64                           │
│ phf_data: [u8; ...]                     │  Algorithm-specific
├─────────────────────────────────────────┤
│ verification: [u64; token_count]        │  Hash of each asset_id
└─────────────────────────────────────────┘
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
    thumb_size: u16,           // e.g., 300
    columns: u8,               // e.g., 10
    rows: u8,                  // e.g., 10
    sheet_count: u16,          // e.g., 100 for 10K tokens
    format: u8,                // 0 = webp, 1 = png, 2 = avif
}
```

## Size Budget (10K Collection, 10 traits, 200 total values)

| Section | Calculation | Size |
|---------|-------------|------|
| Header | Fixed | 32 B |
| String table | ~200 strings × 12 chars + overhead | ~3 KB |
| Trait schema | 10 traits × 20 values × 6 bytes | ~1.2 KB |
| Trait index | 200 buckets, avg 50 tokens each × 2 bytes | ~20 KB |
| Token table | 10K × 18 bytes | 180 KB |
| PHF + verification | ~3KB + 10K × 8 bytes | ~83 KB |
| Sprites metadata | Fixed | 8 B |
| **Total** | | **~290 KB** |

With compression (zstd): **~100-150 KB**

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
    
    /// Filter tokens by trait:value pairs (AND logic)
    pub fn filter(&self, filters: &[(u8, u8)]) -> FilteredTokens;
    
    /// Search trait values by prefix (for autocomplete)
    pub fn search_values(&self, prefix: &str) -> Vec<TraitValueMatch>;
    
    /// Get all trait definitions (for filter UI)
    pub fn traits(&self) -> &[TraitDef];
    
    /// Collection metadata
    pub fn name(&self) -> &str;
    pub fn token_count(&self) -> u32;
}

/// Zero-copy view into a token
pub struct TokenView<'a> {
    pub sprite: SpriteCoords,
    pub rarity_rank: u16,
    pub rarity_percentile: f32,
    pub attributes: AttributeIter<'a>,
    pub name: &'a str,
}

/// Filtered result with efficient iteration
pub struct FilteredTokens<'a> {
    collection: &'a Collection,
    indices: Vec<u16>,  // or iterator for large results
}

impl<'a> FilteredTokens<'a> {
    pub fn count(&self) -> usize;
    pub fn iter(&self) -> impl Iterator<Item = TokenView<'a>>;
}
```

## File Naming & Caching

```
/{chain}/{collection_id}/collection.bin   → immutable, cache forever
/{chain}/{collection_id}/sprites_000.webp → immutable, cache forever
```

When a collection is resynced, generate new files with content-hash in path or use versioning:
```
/cardano/{policy_id}/v3/collection.bin
```

## Ingestion Configs

Per-collection configuration files control how traits are processed during sync.

### File Location

```
configs/
└── cardano/
    └── {policy_id}.toml
```

### Config Format

```toml
# Display name (optional, defaults to on-chain name)
name = "Toolheads"

# Traits to exclude from filtering/indexing
# Use for unique identifiers that would bloat the index
[traits]
ignore = [
    "Call Sign",      # Unique per token
    "Serial Number",  # Unique per token
]

# Trait name aliases for cleaner display
[traits.aliases]
"BACKGROUND" = "Background"
"BODY_TYPE" = "Body Type"

# Optional: override rarity calculation
# [rarity]
# exclude = ["Call Sign"]  # Don't count toward rarity score
```

### Behavior

1. **No config file**: All traits included, sync fails if >512 trait:value combinations
2. **With ignore list**: Excluded traits still stored in token details but not indexed for filtering
3. **With aliases**: Display names normalized in UI, original names preserved in data

## Build Pipeline

```
configs/{chain}/{id}.toml ─────────────────────┐
                                               ▼
cnft.tools API → sync CLI → collection.bin + sprites_*.webp
     ↓                ↓
[CnftAsset]    Apply config:
                 - Filter ignored traits from bitmap
                 - Apply aliases
                 - Validate bitmap size
                      ↓
               Compute rarity
               Build trait schema
               Build inverted index
               Build PHF
               Generate sprites
               Pack into collection.bin
```

## WASM Considerations

1. **Memory mapping**: In WASM, load entire file into linear memory. Use typed array views for zero-copy access.

2. **Alignment**: Ensure struct layouts are naturally aligned for direct casting.

3. **Endianness**: Use little-endian (native for WASM).

4. **String handling**: UTF-8 strings with null terminators for easy slicing.

## Future Extensions

- **Delta updates**: For live collections, support incremental syncs
- **Multiple bitmaps**: Support collections with 256+ trait:value combinations
- **Compressed sprites**: Embed low-res sprite data directly (for offline/preview)
- **Provenance data**: On-chain tx hashes, mint dates, etc.
