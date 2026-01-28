# Fast Thumbnail Lookup Design

## Goal

Provide fast, cacheable thumbnail lookups given a `policy_id` and `raw_asset_hex` (AssetId).

## Key Insight

For live collections synced from cnft.tools:
- `encoded_name` == `raw_asset_hex`
- `TokenDetails.id` stores the `encoded_name`
- Therefore: `raw_asset_hex` IS the token ID used in index lookups

## Current Flow

```
Client Request: GET /{policy_id}/thumb/{raw_asset_hex}
                              ↓
Worker: Load index.json from KV (cached)
                              ↓
Lookup: index.entries[raw_asset_hex] → IndexEntry
                              ↓
Response: sprite sheet URL + coordinates
```

## URL Design

### Thumbnail Metadata Endpoint
```
GET /{policy_id}/thumb/{raw_asset_hex}

Response (JSON, cacheable):
{
  "sprite": {
    "sheet": "sprites_003.webp",
    "x": 4,
    "y": 7,
    "size": 300
  },
  "token": {
    "id": "SquashuaChicken0042",
    "name": "Squashua Chicken #42",
    "rarity": { "rank": 150, "percentile": 85.0 }
  }
}
```

### Sprite Sheet (Static Asset)
```
GET /{policy_id}/sprites/sprites_003.webp

Response: WebP image (immutable, long cache)
```

## Client Usage

```javascript
// 1. Fetch thumbnail metadata
const meta = await fetch(`/${policyId}/thumb/${assetHex}`).then(r => r.json());

// 2. Sprite sheet is cached/shared across 100 tokens
const sheetUrl = `/${policyId}/sprites/${meta.sprite.sheet}`;

// 3. CSS to display specific thumbnail
const style = {
  backgroundImage: `url(${sheetUrl})`,
  backgroundPosition: `${meta.sprite.x * -300}px ${meta.sprite.y * -300}px`,
  backgroundSize: '3000px 3000px', // 10x10 grid of 300px thumbs
  width: '300px',
  height: '300px'
};
```

## Performance Considerations

### Current: JSON Index

- `index.json` for 10K collection: ~1.8 MB
- Loaded into KV, cached at edge
- HashMap lookup is O(1) in memory
- **Bottleneck**: Initial parse of 1.8 MB JSON per cold start

### Future: Binary Index

If JSON parsing becomes a bottleneck, consider:

1. **Sorted array + binary search**
   - Sort entries by token ID (raw_asset_hex)
   - Store as fixed-width records: `[token_id_hash: u64, sheet: u16, x: u8, y: u8, ...]`
   - Binary search: O(log n)
   - 10K entries × 16 bytes = 160 KB

2. **Perfect hash table**
   - Pre-compute minimal perfect hash for token IDs
   - Direct array indexing: O(1)
   - Requires rebuild when collection changes

3. **Separate sprite-only index**
   - Tiny index with just sprite coords (no full image offsets)
   - `sprite_index.bin`: token_id_hash → (sheet, x, y)
   - Keep full `index.json` for image serving

### Recommendation

Start with JSON. Optimize to binary only if:
- Cold start latency is measurable problem
- Profile shows JSON parsing as the bottleneck

## Caching Strategy

| Resource | Cache-Control | Notes |
|----------|---------------|-------|
| `sprites_XXX.webp` | `immutable, max-age=31536000` | Content-addressed, never changes |
| `/thumb/{asset}` | `max-age=3600` | Metadata rarely changes |
| `index.json` | KV cache, 24h TTL | Worker-side only |
| `asset_details.json` | KV cache, 1h TTL | Worker-side only |

## Open Questions

1. Should `/thumb/` return minimal data (just sprite coords) or full token metadata?
2. Do we need batch lookups? `POST /thumbs` with array of asset IDs?
3. For preview collections, token ID is numeric ("000001") not hex - handle both?
