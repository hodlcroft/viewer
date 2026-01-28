# Perfect Hash Index Design

## Overview

Replace the JSON-based index with a runtime-constructed perfect hash table for O(1) lookups by asset ID across any chain.

## Data Model

```
Chain + Collection ID + Asset ID → SpriteLocation + DetailPointer
```

Where:
- **Chain**: Cardano, Ethereum, Solana, etc.
- **Collection ID**: Policy ID (Cardano), Contract Address (EVM), etc.
- **Asset ID**: raw_asset_hex (Cardano), token_id (EVM), mint address (Solana)

## Index Structure

### Minimal Sprite Index (PHF)

For each collection, a perfect hash table mapping asset IDs to sprite locations:

```rust
struct SpriteIndex {
    /// PHF mapping asset_id → entry index
    phf: PerfectHashFunction,
    /// Packed entries (fixed size, indexed by PHF output)
    entries: Vec<SpriteEntry>,
}

struct SpriteEntry {
    /// Sprite sheet number (u16 supports 65K sheets = 6.5M tokens)
    sheet: u16,
    /// Grid position (u8 each, supports up to 255x255 grid)
    x: u8,
    y: u8,
    /// Offset into asset_details.json tokens array (u32)
    detail_offset: u32,
}
```

Size per entry: 8 bytes
10K collection: ~80 KB (vs 1.8 MB JSON)

### Asset Details (Unchanged)

Keep `asset_details.json` for full token metadata. The `detail_offset` points into the tokens array for O(1) detail lookup after sprite lookup.

```
1. PHF lookup: asset_id → SpriteEntry (O(1), ~80KB in memory)
2. If details needed: tokens[detail_offset] (O(1), lazy load)
```

## File Format

```
bundle/
├── sprite_index.bin    # PHF + packed SpriteEntry array
├── asset_details.json  # Full metadata (lazy loaded for details)
├── sprites_000.webp    # Thumbnail sheets
└── ...
```

### sprite_index.bin Format

```
┌─────────────────────────────────────────┐
│ Header (16 bytes)                       │
│   magic: [u8; 4] = "SPIX"               │
│   version: u16                          │
│   entry_count: u32                      │
│   phf_seed: u64                         │
├─────────────────────────────────────────┤
│ PHF Data (variable)                     │
│   Depends on PHF algorithm              │
│   (~2-3 bits per key typical)           │
├─────────────────────────────────────────┤
│ Entries (entry_count × 8 bytes)         │
│   [SpriteEntry; entry_count]            │
└─────────────────────────────────────────┘
```

## PHF Construction

Build at sync time, not compile time:

```rust
use boomphf::Mphf;

fn build_sprite_index(tokens: &[TokenDetails], sprite_coords: &HashMap<String, (u16, u8, u8)>) -> SpriteIndex {
    let asset_ids: Vec<&str> = tokens.iter().map(|t| t.id.as_str()).collect();
    
    // Build minimal perfect hash (gamma = 2.0 is good tradeoff)
    let phf = Mphf::new(2.0, &asset_ids);
    
    // Build entries array in PHF order
    let mut entries = vec![SpriteEntry::default(); tokens.len()];
    for (idx, token) in tokens.iter().enumerate() {
        let phf_idx = phf.hash(&token.id) as usize;
        let (sheet, x, y) = sprite_coords[&token.id];
        entries[phf_idx] = SpriteEntry {
            sheet,
            x,
            y,
            detail_offset: idx as u32,
        };
    }
    
    SpriteIndex { phf, entries }
}
```

## Lookup Flow

```rust
fn lookup_sprite(index: &SpriteIndex, asset_id: &str) -> Option<&SpriteEntry> {
    let idx = index.phf.hash(asset_id)? as usize;
    // PHF guarantees no collision for known keys
    // For unknown keys, we get garbage - need membership check
    Some(&index.entries[idx])
}
```

### Handling Unknown Keys

PHF returns an index for ANY input, even keys not in the original set. Options:

1. **Store asset_id hash in entry** - verify after lookup (adds 8 bytes/entry)
2. **Bloom filter** - fast "definitely not in set" check
3. **Trust the caller** - if asset_id comes from chain data, it's valid

Recommendation: Option 1 for safety, small overhead:

```rust
struct SpriteEntry {
    asset_id_hash: u64,  // For verification
    sheet: u16,
    x: u8,
    y: u8,
    detail_offset: u32,
}
// Now 16 bytes per entry, 160KB for 10K
```

## Multi-Chain Support

Abstract the collection identifier:

```rust
enum CollectionId {
    Cardano { policy_id: String },
    Ethereum { chain_id: u64, contract: String },
    Solana { collection_mint: String },
}

impl CollectionId {
    fn to_path(&self) -> String {
        match self {
            Self::Cardano { policy_id } => format!("cardano/{policy_id}"),
            Self::Ethereum { chain_id, contract } => format!("evm/{chain_id}/{contract}"),
            Self::Solana { collection_mint } => format!("solana/{collection_mint}"),
        }
    }
}
```

Bundle storage:
```
bundles/
├── cardano/
│   └── {policy_id}/
│       ├── sprite_index.bin
│       ├── asset_details.json
│       └── sprites_*.webp
├── evm/
│   └── {chain_id}/
│       └── {contract}/
│           └── ...
└── solana/
    └── {collection_mint}/
        └── ...
```

## Trait Filtering (Preserving the Magic)

The fast filtering comes from:
1. All token data in memory (`asset_details.json` loaded once)
2. Client-side filtering with reactive UI (no round-trips)
3. Efficient trait index built from `trait_summary`

This architecture preserves that:
- `asset_details.json` still has all attributes
- Trait summary enables instant filter suggestions
- PHF index only optimizes the sprite lookup path

For the **picker component** use case:
```
User types trait filter → instant filter (client-side)
                        → show matching thumbnails (PHF → sprites)
                        → user clicks to select
                        → return asset_id
```

## Implementation Phases

### Phase 1: Current (JSON)
- Keep working system
- Profile to confirm JSON parsing is the bottleneck

### Phase 2: Binary Sprite Index
- Add `sprite_index.bin` generation to sync CLI
- Worker loads binary index, falls back to JSON
- Measure improvement

### Phase 3: Multi-Chain
- Abstract collection identifiers
- Add EVM/Solana sync support
- Unified viewer across chains

## Open Questions

1. Which PHF library? `boomphf` (Rust), `pthash` (C++ with bindings), custom?
2. Store sprite index in KV or R2? (Small enough for KV, ~160KB)
3. Compression? (zstd on the binary could halve size)
4. Incremental updates? (Full rebuild on resync, or delta support?)
