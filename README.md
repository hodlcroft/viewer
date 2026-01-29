# NFT Collection Viewer

A high-performance viewer for NFT collections, optimized for fast loading of 10K+ token collections with trait filtering, rarity rankings, and efficient image delivery via range requests.

## Features

- **Fast Gallery Loading** - Sprite sheet thumbnails load entire grids in single requests
- **Trait Filtering** - Bitmap-based filtering for instant results on any trait combination
- **Rarity Rankings** - Percentile-based rarity with tier coloring (Legendary, Epic, Rare, etc.)
- **HCF Range Requests** - Full-resolution images loaded on-demand via HTTP range requests
- **Compact Binary Format** - ~350KB for 10K tokens (vs 4.5MB JSON equivalent)
- **Token Protection** - Access tokens for private preview collections
- **Self-Hostable** - Deploys to Cloudflare Workers + R2

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Frontend                              │
│     Leptos WASM app with infinite scroll & filtering        │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         Worker                               │
│        Cloudflare Worker serving assets from R2             │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
         ┌────────┐     ┌──────────┐    ┌──────────┐
         │   R2   │     │    R2    │    │    R2    │
         │ binary │     │   HCF    │    │ sprites  │
         │ index  │     │  shards  │    │  sheets  │
         └────────┘     └──────────┘    └──────────┘
```

## Quick Start

```bash
# Build frontend
cd frontend && trunk build --release

# Build CLI
cargo build --release -p viewer-cli

# Ingest a collection
viewer-cli ingest --source ./images --output ./bundle

# Serve locally
cd worker && npm run dev
```

## Crates

| Crate | Description |
|-------|-------------|
| `viewer-binary` | Binary format types: Header, TraitSchema, HcfMetadata, BitmapSize |
| `viewer-format` | High-level types: AssetDetails, TokenDetails, CollectionSource |
| `viewer-bundle` | Bundle I/O: reading/writing index.json and sharded images |
| `viewer-ingest` | Ingestion pipeline: sprite generation, HCF bundling, binary encoding |
| `viewer-cli` | CLI tool for syncing and bundling collections |

## File Structure

A complete collection bundle consists of:

```
{collection-slug}/
├── collection.bin        # Binary index with traits, tokens, HCF locations
├── sprites.bin           # Sprite index for fast thumbnail lookups
├── sprites/
│   ├── 0000.webp         # Sprite sheet 0 (4x4 = 16 thumbnails)
│   ├── 0001.webp         # Sprite sheet 1
│   └── ...
└── hcf/
    ├── images_000.hcf    # Image shard 0 (up to 250MB)
    ├── images_001.hcf    # Image shard 1
    └── ...
```

## Binary Format (collection.bin)

The binary format is designed for efficient WASM parsing with zero-copy access where possible.

### Structure

```
┌──────────────────────────┐
│ Header (128 bytes)       │  Magic, version, counts, section offsets
├──────────────────────────┤
│ String Table             │  Deduplicated trait names and values
├──────────────────────────┤
│ Trait Schema             │  Trait definitions with bitmap bit positions
├──────────────────────────┤
│ Trait Index              │  Inverted index: trait:value → token list
├──────────────────────────┤
│ Token Table              │  Fixed-size entries with rarity, name, bitmap
├──────────────────────────┤
│ Asset ID Index           │  Asset ID strings for URL routing
├──────────────────────────┤
│ HCF Metadata             │  Shard size, count, image format
├──────────────────────────┤
│ HCF Index                │  Per-token image offset/length
└──────────────────────────┘
```

Note: Sprite data is stored separately in `sprites.bin` for fast lookups without loading the full collection.

### Header (128 bytes)

```rust
struct Header {
    magic: [u8; 4],              // "COLL"
    version: u16,                // Format version (currently 1)
    flags: u16,                  // Feature flags (bit 0 = multi-source, bit 1 = hide-rarity)
    token_count: u32,            // Number of tokens
    trait_count: u8,             // Number of traits
    bitmap_size: u8,             // BitmapSize enum (0=U64, 1=U128, 2=U256, 3=U512, 4=U1024)
    hcf_index_size: u8,          // HcfIndexSize enum
    source_count: u8,            // Number of sources (1 = single chain)
    
    // Section offsets (from start of file)
    string_table_offset: u32,
    trait_schema_offset: u32,
    trait_index_offset: u32,
    token_table_offset: u32,
    phf_offset: u32,
    reserved_sprites: u32,       // Reserved (sprite data now in sprites.bin)
    hcf_metadata_offset: u32,
    hcf_index_offset: u32,
    sources_offset: u32,
    asset_id_index_offset: u32,
    string_ref_size: u8,         // StringRefSize enum (0=U16, 1=U32)
    
    reserved: [u8; 71],          // Reserved for future use
}
```

### Trait Bitmap

Each token has a bitmap where each bit represents a trait:value combination:

```
bit 0:  Background = Blue
bit 1:  Background = Red
bit 2:  Background = Green
bit 3:  Eyes = Happy
bit 4:  Eyes = Sad
...
```

Filtering is a simple AND operation:
```rust
fn matches_filter(token_bitmap: u64, filter_bitmap: u64) -> bool {
    (token_bitmap & filter_bitmap) == filter_bitmap
}
```

### Bitmap Sizing

The bitmap size is chosen during ingestion based on total trait:value count:

| Enum Value | Type | Max Values | Bytes per Token |
|------------|------|------------|-----------------|
| 0 | U64 | 64 | 8 |
| 1 | U128 | 128 | 16 |
| 2 | U256 | 256 | 32 |
| 3 | U512 | 512 | 64 |

### Token Entry

Fixed-size per token (8 bytes + bitmap, or 9 bytes for multi-source):

```rust
struct TokenEntry {
    // For multi-source only:
    source_index: u8,      // Which source (only if FLAG_MULTI_SOURCE set)
    
    // Always present (8 bytes):
    rarity_rank: u16,      // 1 = rarest
    rarity_score: u16,     // Fixed-point (score * 100)
    name_ref: u32,         // High bit = custom name flag, lower 31 bits = string table offset
    bitmap: [u8; N],       // Trait bitmap (N = bitmap_size.byte_size())
}
```

Note: Sprite coordinates are stored in `sprites.bin`, not in the token entry. This keeps the collection.bin smaller and allows sprite lookups without loading the full collection.

### Size Budget

For a typical 10K PFP collection with 10 traits and ~100 total values:

| Section | Size |
|---------|------|
| Header | 128 B |
| String table | ~2 KB |
| Trait schema | ~600 B |
| Trait index | ~20 KB |
| Token table | ~160 KB |
| Asset ID index | ~80 KB |
| HCF metadata | 12 B |
| HCF index | ~60 KB |
| **Total** | **~325 KB** |

Plus sprites.bin: ~120 KB for 10K tokens (12 bytes per entry + 18 byte header).

## Sprite Index (sprites.bin)

The sprite index is a separate lightweight file for fast thumbnail lookups by asset ID hash.

### Format

```
┌──────────────────────────┐
│ Header (18 bytes)        │
├──────────────────────────┤
│ Entries (12 bytes each)  │  Sorted by asset_id_hash for binary search
└──────────────────────────┘
```

### Header (18 bytes)

```rust
struct SpriteIndexHeader {
    magic: [u8; 4],        // "SPRT"
    version: u16,          // Format version (currently 1)
    entry_count: u32,      // Number of entries
    sheet_count: u16,      // Number of sprite sheets
    thumb_width: u16,      // Thumbnail width in pixels
    thumb_height: u16,     // Thumbnail height in pixels
    grid_columns: u8,      // Columns per sheet
    grid_rows: u8,         // Rows per sheet
}
```

### Entry (12 bytes)

```rust
struct SpriteIndexEntry {
    asset_id_hash: u64,    // xxHash64 of asset ID string
    sheet: u16,            // Which sprite sheet (0000.webp, 0001.webp, etc.)
    x: u8,                 // Column in sprite grid (0 to grid_columns-1)
    y: u8,                 // Row in sprite grid (0 to grid_rows-1)
}
```

### Lookup

Entries are sorted by `asset_id_hash` for O(log n) binary search lookup:

```rust
fn lookup_sprite(sprites_bin: &[u8], asset_id: &str) -> Option<(u16, u8, u8)> {
    let hash = xxhash64(asset_id.as_bytes());
    // Binary search entries by hash
    // Returns (sheet, x, y) if found
}
```

### Benefits

- **Fast lookups**: Binary search by hash, no need to scan all tokens
- **Standalone**: Can display thumbnails without loading collection.bin
- **Compact**: 12 bytes per token vs 4 bytes saved in collection.bin token entries
- **Parallel loading**: Frontend loads collection.bin and sprites.bin simultaneously

## Sprite Sheets

Thumbnails are pre-rendered into sprite sheets for efficient gallery loading.

### Configuration

```rust
const SPRITE_THUMB_SIZE: u32 = 256;  // 256x256 pixels per thumbnail
const SPRITE_COLUMNS: u32 = 4;        // 4 columns per sheet
const SPRITE_ROWS: u32 = 4;           // 4 rows per sheet
const SPRITES_PER_SHEET: u32 = 16;    // 16 tokens per sheet
```

### Lookup

Given a token index:
```rust
let sheet_index = token_index / SPRITES_PER_SHEET;  // Which .webp file
let position = token_index % SPRITES_PER_SHEET;
let sprite_x = position % SPRITE_COLUMNS;           // Column (0-3)
let sprite_y = position / SPRITE_COLUMNS;           // Row (0-3)

// CSS background-position
let bg_x = sprite_x * SPRITE_THUMB_SIZE;  // e.g., 512px
let bg_y = sprite_y * SPRITE_THUMB_SIZE;  // e.g., 256px
```

### CSS Usage

```scss
.thumbnail {
    width: 256px;
    height: 256px;
    background-image: url('/sprites/0000.webp');
    background-size: 400% 400%;  // 4x4 grid
    background-position: calc(var(--sprite-col) * 33.3333%)
                         calc(var(--sprite-row) * 33.3333%);
}
```

## HCF (High-Compression Format) Bundles

Full-resolution images are stored in sharded binary files, accessed via HTTP range requests.

### Sharding

- **Shard Size**: 250 MB (configurable, stays under Cloudflare's 300MB limit)
- **Naming**: `images_000.hcf`, `images_001.hcf`, etc.
- **Padding**: Intermediate shards are zero-padded to exactly shard_size for consistent offset math

### HCF Metadata

```rust
struct HcfMetadata {
    shard_size: u32,       // 250_000_000 (250 MB)
    shard_count: u16,      // Number of shard files
    image_format: u8,      // 0=webp, 1=png, 2=avif, 3=jpeg
}
```

### Token HCF Location

Each token entry includes its image location as a global byte offset and length:

```rust
// Stored in token entry (6-8 bytes depending on HcfIndexSize)
global_offset: u64,  // Absolute position across all shards
length: u32,         // Image size in bytes
```

### Range Request Flow

```rust
// 1. Calculate shard and offset
let shard_index = global_offset / shard_size;
let offset_in_shard = global_offset % shard_size;

// 2. Construct URL
let url = format!("/hcf/images_{:03}.hcf", shard_index);

// 3. Fetch with range header
let range = format!("bytes={}-{}", offset_in_shard, offset_in_shard + length - 1);
fetch(url, { headers: { "Range": range } });

// 4. Response is raw image bytes (webp/png/etc)
```

### Benefits

- **On-demand loading**: Only fetch images user actually views
- **Parallel downloads**: Multiple shards can be fetched simultaneously
- **CDN-friendly**: Range requests work with standard CDN caching
- **No index lookup**: HCF location embedded in token entry

## Frontend

The frontend is a Leptos CSR (Client-Side Rendered) application compiled to WASM.

### Pages

| Route | Description |
|-------|-------------|
| `/` | Home page with collection list |
| `/:slug` | Gallery view with infinite scroll and filtering |
| `/:slug/:id` | Detail view with full image and traits |
| `/debug/:slug` | Debug view showing binary format details |

### Gallery Features

- **Infinite Scroll**: Loads tokens in batches as user scrolls
- **Sprite Thumbnails**: Single request loads 16 thumbnails
- **Trait Filtering**: URL params like `?filter=Background:Blue`
- **Keyboard Navigation**: Arrow keys in detail view

### Detail View Features

- **Lazy HCF Loading**: Sprite shown immediately, full image loaded via range request
- **Request Cancellation**: Navigation cancels in-flight requests
- **Smooth Transitions**: CSS opacity transitions when image loads
- **Rarity Display**: Rank, percentile, and tier coloring

### State Management

```rust
// Collection data cached globally
let cache = expect_context::<CollectionCache>();

// Loading state tracked with request IDs
struct LoadingTracker {
    current_request_id: Arc<AtomicU64>,
    completed_request_id: RwSignal<u64>,
}
```

## Worker API

The Cloudflare Worker serves assets from R2 with optional access token protection.

### Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check |
| `GET /:slug/collection.bin` | Binary collection data |
| `GET /:slug/sprites/:sheet.webp` | Sprite sheet image |
| `GET /:slug/hcf/images_:shard.hcf` | HCF shard (supports range requests) |

### Access Tokens

For private preview collections, requests require a token:

```
GET /my-collection/collection.bin?token=abc123
```

Or via header:
```
Authorization: Bearer abc123
```

## Development

### Prerequisites

- Rust 1.75+
- [Trunk](https://trunkrs.dev/) for WASM builds
- Node.js 18+ for worker development
- Wrangler CLI for Cloudflare deployment

### Building

```bash
# Frontend (WASM)
cd frontend
trunk build          # Development
trunk build --release  # Production
trunk serve          # Dev server with hot reload

# CLI
cargo build -p viewer-cli

# Worker
cd worker
npm install
npm run dev          # Local development
npm run deploy       # Deploy to Cloudflare
```

### Testing

```bash
cargo test --workspace
```

## Configuration

### Collection Config (TOML)

```toml
# configs/my-collection.toml

[collection]
name = "My Collection"
slug = "my-collection"

[sprites]
thumb_size = 256
columns = 4
rows = 4

[hcf]
shard_size = 262144000  # 250 MB
image_format = "webp"

[traits]
# Exclude high-cardinality traits from filtering
ignore = ["Serial Number", "Unique ID"]

# Rename traits for display
[traits.aliases]
"BG" = "Background"
"EYES_TYPE" = "Eyes"
```

## Documentation

- [Collection Binary Format](docs/collection-binary-format.md) - Detailed binary format specification
- [Fast Thumbnail Lookup](docs/fast-thumbnail-lookup.md) - URL design and caching strategy
- [PHF Index Design](docs/phf-index-design.md) - Perfect hash function details

## License

MIT OR Apache-2.0
