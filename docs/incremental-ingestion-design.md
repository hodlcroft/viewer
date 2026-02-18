# Incremental Ingestion Design

## Overview

This document describes a design for incrementally ingesting NFT collections that are actively minting, without requiring full re-processing of already-ingested tokens.

## Goals

1. **Append-only for images** - Never rewrite existing HCF shards or sprite sheets (except the current/last one)
2. **Stable token indices** - Token #42 stays token #42 forever
3. **Efficient updates** - Only process new tokens, regenerate small index files
4. **Resume-friendly** - Can pick up where we left off after interruption
5. **Updateable NFT support** - Handle metadata/image changes (stretch goal)

## Assumptions

- Tokens are ingested in **mint order** (chronological by on-chain mint timestamp)
- A background worker has access to previously downloaded images (local disk or object storage)
- The indexer provides mint timestamps or block heights for ordering
- Collection metadata (traits) is relatively stable (new values can appear, but trait names don't change)

## Storage Layout

```
{collection}/
├── state.json              # Ingestion state (mutable)
├── collection.bin          # Binary index (regenerated on sync)
├── sprites/
│   ├── 0000.webp           # Complete (16 tokens) - immutable
│   ├── 0001.webp           # Complete - immutable
│   ├── ...
│   └── 0099.webp           # Partial (current) - mutable
├── hcf/
│   ├── images_000.hcf      # Complete (250MB) - immutable
│   ├── images_001.hcf      # Complete - immutable
│   ├── ...
│   └── images_005.hcf      # Partial (current) - mutable
└── images/                 # Source images (local cache)
    ├── 000001.webp
    ├── 000002.webp
    └── ...
```

## Ingestion State

Track progress in `state.json`:

```json
{
  "version": 1,
  "collection_slug": "my-collection",
  "last_sync": "2025-01-28T12:00:00Z",
  
  "tokens": {
    "count": 5432,
    "last_mint_time": "2025-01-28T11:45:00Z",
    "last_block_height": 12345678
  },
  
  "sprites": {
    "complete_sheets": 339,
    "current_sheet_index": 339,
    "current_sheet_count": 8
  },
  
  "hcf": {
    "complete_shards": 2,
    "current_shard_index": 2,
    "current_shard_offset": 156_234_567,
    "image_format": "webp"
  },
  
  "traits": {
    "schema_hash": "abc123",
    "value_counts": {
      "Background:Blue": 542,
      "Background:Red": 487
    }
  }
}
```

## Incremental Sync Flow

### 1. Fetch New Tokens

```
┌─────────────────┐
│ Query indexer   │ "Give me tokens minted after {last_mint_time}"
│ for new tokens  │ Returns: [{id, name, traits, image_url, mint_time}, ...]
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Download new    │ Fetch images not in local cache
│ images          │ Store to images/{index}.webp
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Assign token    │ index = state.tokens.count + position_in_batch
│ indices         │ Maintains mint-order = index-order invariant
└─────────────────┘
```

### 2. Update Sprite Sheets

```
┌─────────────────────────────────────────────────────────┐
│ Current sheet: sprites/0339.webp (8 of 16 slots filled) │
└─────────────────────────────────────────────────────────┘
                         │
         ┌───────────────┼───────────────┐
         │               │               │
         ▼               ▼               ▼
   Add tokens 9-16   Sheet full?    Start new sheet
   to current sheet  Yes → finalize  sprites/0340.webp
                     mark immutable
```

**Algorithm:**

```rust
fn update_sprites(new_tokens: &[Token], state: &mut State) {
    let mut current_sheet = load_or_create_sheet(state.sprites.current_sheet_index);
    
    for token in new_tokens {
        let slot = state.sprites.current_sheet_count;
        
        // Add thumbnail to current sheet
        current_sheet.set_slot(slot, &token.thumbnail);
        state.sprites.current_sheet_count += 1;
        
        // Sheet full? Finalize and start new one
        if state.sprites.current_sheet_count >= SPRITES_PER_SHEET {
            save_sheet(&current_sheet, state.sprites.current_sheet_index);
            state.sprites.complete_sheets += 1;
            state.sprites.current_sheet_index += 1;
            state.sprites.current_sheet_count = 0;
            current_sheet = create_empty_sheet();
        }
    }
    
    // Save partial sheet
    save_sheet(&current_sheet, state.sprites.current_sheet_index);
}
```

### 3. Update HCF Shards

```
┌─────────────────────────────────────────────────────────┐
│ Current shard: images_002.hcf (156MB of 250MB used)     │
└─────────────────────────────────────────────────────────┘
                         │
         ┌───────────────┼───────────────┐
         │               │               │
         ▼               ▼               ▼
   Append images    Shard full?     Pad to 250MB,
   to current shard Yes → finalize  start images_003.hcf
```

**Key insight:** We can append to HCF shards because:
- Token HCF locations use **global offsets** (across all shards)
- Shard boundaries are calculated: `shard = global_offset / shard_size`
- Only need to pad when finalizing a shard

**Algorithm:**

```rust
fn update_hcf(new_tokens: &[Token], state: &mut State) -> Vec<HcfLocation> {
    let mut locations = Vec::new();
    let mut current_shard = open_append(state.hcf.current_shard_index);
    
    for token in new_tokens {
        let image_bytes = read_image(&token.image_path);
        let global_offset = state.hcf.complete_shards as u64 * SHARD_SIZE as u64 
                          + state.hcf.current_shard_offset;
        
        // Would this overflow the shard?
        if state.hcf.current_shard_offset + image_bytes.len() > SHARD_SIZE {
            // Pad current shard to exactly SHARD_SIZE
            pad_to_size(&mut current_shard, SHARD_SIZE);
            finalize_shard(current_shard, state.hcf.current_shard_index);
            
            state.hcf.complete_shards += 1;
            state.hcf.current_shard_index += 1;
            state.hcf.current_shard_offset = 0;
            current_shard = create_shard(state.hcf.current_shard_index);
        }
        
        // Append image to shard
        current_shard.write(&image_bytes);
        
        locations.push(HcfLocation {
            global_offset,
            length: image_bytes.len() as u32,
        });
        
        state.hcf.current_shard_offset += image_bytes.len();
    }
    
    // Save partial shard (not padded yet)
    save_shard(current_shard, state.hcf.current_shard_index);
    
    locations
}
```

### 4. Regenerate collection.bin

The binary index is small (~350KB for 10K tokens) and fast to regenerate:

```rust
fn regenerate_collection_bin(all_tokens: &[Token], state: &State) {
    let mut writer = CollectionBinWriter::new();
    
    // Header
    writer.write_header(all_tokens.len(), state.traits.schema_hash);
    
    // String table (trait names, values, token names)
    writer.write_string_table(&collect_strings(all_tokens));
    
    // Trait schema (includes updated value counts)
    writer.write_trait_schema(&build_schema(all_tokens));
    
    // Trait index (inverted index for filtering)
    writer.write_trait_index(&build_inverted_index(all_tokens));
    
    // Token table (sprite coords, rarity, bitmap, HCF location)
    writer.write_token_table(all_tokens);
    
    // Asset ID index
    writer.write_asset_id_index(all_tokens);
    
    // Sprite metadata
    writer.write_sprite_metadata(state.sprites);
    
    // HCF metadata
    writer.write_hcf_metadata(state.hcf);
    
    writer.finalize("collection.bin");
}
```

### 5. Recalculate Rarity

Rarity rankings change as new tokens are added:

```rust
fn recalculate_rarity(all_tokens: &mut [Token]) {
    // Count trait occurrences
    let mut trait_counts: HashMap<(TraitName, TraitValue), u32> = HashMap::new();
    for token in all_tokens.iter() {
        for (trait_name, value) in &token.traits {
            *trait_counts.entry((trait_name, value)).or_default() += 1;
        }
    }
    
    // Calculate rarity score for each token
    // Score = sum of (1 / trait_percentage) for each trait
    for token in all_tokens.iter_mut() {
        let mut score = 0.0;
        for (trait_name, value) in &token.traits {
            let count = trait_counts[&(trait_name, value)];
            let percentage = count as f64 / all_tokens.len() as f64;
            score += 1.0 / percentage;
        }
        token.rarity_score = score;
    }
    
    // Sort by score and assign ranks
    let mut sorted: Vec<_> = all_tokens.iter_mut().enumerate().collect();
    sorted.sort_by(|a, b| b.1.rarity_score.partial_cmp(&a.1.rarity_score).unwrap());
    
    for (rank, (_, token)) in sorted.into_iter().enumerate() {
        token.rarity_rank = (rank + 1) as u16;
    }
}
```

**Note:** Rarity recalculation requires iterating all tokens, but it's CPU-only and fast (~10ms for 10K tokens).

## Handling Updateable NFTs

Some NFTs allow metadata or image updates after minting. This is harder to handle incrementally.

### Detection

```rust
struct TokenUpdate {
    token_index: u32,
    update_type: UpdateType,
    timestamp: DateTime,
}

enum UpdateType {
    MetadataOnly { old_hash: String, new_hash: String },
    ImageChanged { old_hash: String, new_hash: String },
}
```

### Strategy 1: Metadata-Only Updates (Easy)

If only traits/name changed but image is the same:
- Update token entry in collection.bin
- Recalculate rarity
- No HCF/sprite changes needed

### Strategy 2: Image Updates (Hard)

Options for handling image changes:

#### Option A: Append New Version

- Append new image to HCF (don't touch old location)
- Update token's HCF location to point to new image
- Wastes space but preserves immutability of old shards

```
Token #42:
  v1: global_offset=1000, length=50000  (orphaned)
  v2: global_offset=9999000, length=48000  (current)
```

**Pros:** Simple, no rewriting
**Cons:** Wasted space accumulates

#### Option B: In-Place Update (Current Shard Only)

- If token's image is in the current (partial) shard, can rewrite
- If in a finalized shard, fall back to Option A

```rust
fn update_image(token: &mut Token, new_image: &[u8], state: &State) {
    let current_shard_start = state.hcf.complete_shards as u64 * SHARD_SIZE as u64;
    
    if token.hcf_location.global_offset >= current_shard_start {
        // Token is in current shard - can update in place if same size
        // or append and update offset
    } else {
        // Token is in finalized shard - must append
    }
}
```

#### Option C: Periodic Compaction

- Run compaction job periodically to reclaim orphaned space
- Rewrites all shards, removing orphaned images
- Expensive but keeps storage bounded

```rust
fn compact_hcf(collection: &str) {
    let tokens = load_all_tokens(collection);
    let used_locations: HashSet<_> = tokens.iter()
        .map(|t| t.hcf_location)
        .collect();
    
    // Rewrite shards including only used images
    let mut new_writer = HcfWriter::new();
    for token in &tokens {
        let image = fetch_image_at(token.hcf_location);
        let new_location = new_writer.append(&image);
        // Update token's location
    }
}
```

### Recommended Approach for Updates

1. **Metadata changes:** Handle immediately, regenerate collection.bin
2. **Image changes:** Append new version (Option A)
3. **Compaction:** Run monthly or when orphaned space exceeds threshold (e.g., 20%)

## Caching Considerations

### Immutable Files (Cache Forever)

Once finalized, these never change:
- `sprites/0000.webp` through `sprites/{N-1}.webp` (all but last)
- `hcf/images_000.hcf` through `hcf/images_{N-1}.hcf` (all but last)

```
Cache-Control: public, max-age=31536000, immutable
```

### Mutable Files (Cache with Revalidation)

These change on each sync:
- `collection.bin` - version in query param `?v={hash}`
- `sprites/{last}.webp` - ETag-based revalidation
- `hcf/images_{last}.hcf` - ETag-based revalidation
- `state.json` - no caching (internal use only)

```
Cache-Control: public, max-age=300, must-revalidate
ETag: "{content-hash}"
```

### CDN Purge Strategy

On sync completion:
1. Upload new files to R2
2. Purge CDN cache for:
   - `collection.bin`
   - Current sprite sheet
   - Current HCF shard
3. Don't purge finalized files (they're immutable)

## Background Worker Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Sync Scheduler                          │
│  Cron: every 5 minutes for active collections               │
│  Cron: every hour for stable collections                    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Sync Worker                             │
│  1. Load state.json                                         │
│  2. Query indexer for new tokens                            │
│  3. Download new images (parallel, with retry)              │
│  4. Update sprites (current sheet)                          │
│  5. Update HCF (current shard)                              │
│  6. Regenerate collection.bin                               │
│  7. Upload changed files to R2                              │
│  8. Save state.json                                         │
│  9. Purge CDN cache                                         │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Image Cache                             │
│  Local disk or S3 bucket with all downloaded images         │
│  Key: {collection}/{token_index}.{format}                   │
│  Retained for: compaction, re-processing, debugging         │
└─────────────────────────────────────────────────────────────┘
```

### Worker Configuration

```toml
[sync]
# How often to check for new tokens
active_interval = "5m"      # Collections still minting
stable_interval = "1h"      # Collections that finished minting

# Batch size for processing
batch_size = 100            # Tokens per sync iteration
image_download_parallel = 10

# Thresholds
compaction_threshold = 0.2  # Run compaction when 20% space is orphaned

[storage]
image_cache = "/data/images"  # Local image cache
r2_bucket = "hodlcroft-viewer"

[indexer]
# Cardano indexer
type = "blockfrost"
api_key = "${BLOCKFROST_API_KEY}"
```

## Migration from Full Ingestion

To migrate an existing collection to incremental:

1. **Generate initial state.json** from existing files:
   ```rust
   fn generate_state(collection: &str) -> State {
       let collection_bin = load_collection_bin(collection);
       let sprites = count_sprite_sheets(collection);
       let hcf = analyze_hcf_shards(collection);
       
       State {
           tokens: TokenState {
               count: collection_bin.token_count,
               last_mint_time: fetch_last_mint_time(collection),
               ..
           },
           sprites: SpriteState {
               complete_sheets: sprites.count - 1,
               current_sheet_index: sprites.count - 1,
               current_sheet_count: collection_bin.token_count % SPRITES_PER_SHEET,
           },
           hcf: HcfState {
               complete_shards: hcf.shard_count - 1,
               current_shard_index: hcf.shard_count - 1,
               current_shard_offset: hcf.last_shard_size,
               ..
           },
           ..
       }
   }
   ```

2. **Verify consistency** - ensure token count matches sprite slots and HCF entries

3. **Run incremental sync** - should detect no new tokens and make no changes

## Open Questions

1. **Trait schema changes** - What if a new trait appears mid-mint? Current design assumes trait names are stable. Could version the schema and handle migrations.

2. **Reorg handling** - What if blockchain reorgs? Indexer should handle this, but we might ingest tokens that get rolled back. Could track block heights and re-verify periodically.

3. **Parallel sync** - Can we sync multiple collections in parallel? Yes, they're independent. Just need worker capacity.

4. **Resumable image downloads** - If sync fails mid-download, how to resume? Track downloaded images in state, skip existing ones.

5. **Rate limiting** - Indexer and image CDN rate limits. Need backoff and retry logic.

## Summary

| Operation | Full Ingestion | Incremental |
|-----------|----------------|-------------|
| New tokens | Process all | Process new only |
| Sprite sheets | Generate all | Append to current |
| HCF shards | Generate all | Append to current |
| collection.bin | Generate | Regenerate (fast) |
| Rarity | Calculate | Recalculate (fast) |
| Image downloads | All | New only |
| **Time (10K → 10.1K)** | ~30 min | ~30 sec |

The key insight is that most of the work (image downloading, sprite generation, HCF writing) is **append-only**, and the parts that need regeneration (collection.bin, rarity) are small and fast.
