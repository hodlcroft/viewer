# NFT Viewer

A high-performance viewer for NFT collections, supporting both live on-chain collections and generated previews.

## Features

- View collections of 10K+ tokens with smooth scrolling
- Trait-based filtering with autocomplete
- Rarity rankings and percentiles
- Sprite sheet thumbnails for fast loading
- Token protection for private previews
- Self-hostable on Cloudflare Workers + R2

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Frontend                              │
│  Leptos CSR app with infinite scroll, filtering, rarity UI  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         Worker                               │
│  Cloudflare Worker serving assets from KV + R2              │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
         ┌────────┐     ┌──────────┐    ┌──────────┐
         │   KV   │     │    R2    │    │    R2    │
         │ index  │     │  images  │    │ sprites  │
         │ + meta │     │  shards  │    │  sheets  │
         └────────┘     └──────────┘    └──────────┘
```

## Crates

| Crate | Description |
|-------|-------------|
| `viewer-format` | Core types: `AssetDetails`, `TokenDetails`, `CollectionSource` |
| `viewer-bundle` | Bundle format: `BundleIndex`, `SpriteConfig`, I/O utilities |
| `viewer-cli` | CLI for syncing collections and managing bundles |

## Bundle Format

A bundle consists of:

```
bundle/
├── index.json          # Token ID → image location + sprite coords
├── asset_details.json  # Collection metadata + per-token attributes
├── images_000.bin      # Sharded image data (byte ranges)
├── images_001.bin
├── sprites_000.webp    # 10x10 thumbnail grids (100 tokens each)
├── sprites_001.webp
└── ...
```

### index.json

Maps token IDs to image locations and sprite positions:

```json
{
  "version": 1,
  "image_format": "webp",
  "image_count": 1000,
  "shard_count": 5,
  "sprites": {
    "thumb_size": 300,
    "columns": 10,
    "rows": 10,
    "sheet_count": 10,
    "format": "webp"
  },
  "entries": {
    "SquashuaChicken0001": {
      "offset": 0,
      "length": 45000,
      "hash": "a1b2c3d4",
      "shard": 0,
      "sprite_sheet": 0,
      "sprite_x": 0,
      "sprite_y": 0
    }
  }
}
```

### asset_details.json

Collection metadata and per-token attributes:

```json
{
  "collection": {
    "name": "Squashua Chicken",
    "total_tokens": 5000,
    "source": {
      "type": "live",
      "policy_id": "abc123...",
      "synced_at": "2025-01-28T12:00:00Z"
    },
    "created_at": "2025-01-28T12:00:00Z"
  },
  "tokens": [
    {
      "id": "SquashuaChicken0001",
      "name": "Squashua Chicken #1",
      "attributes": {
        "Background": "Blue",
        "Body": "White",
        "Eyes": "Happy"
      },
      "rarity": {
        "score": 45.2,
        "rank": 150,
        "percentile": 85.0
      }
    }
  ],
  "trait_summary": {
    "Background": {
      "values": {
        "Blue": { "count": 250, "percentage": 25.0 },
        "Red": { "count": 150, "percentage": 15.0 }
      }
    }
  }
}
```

### Collection Sources

**Preview** - Generated collection not yet minted:
```json
{
  "type": "preview",
  "seed": 12345,
  "distribution": "weighted"
}
```

**Live** - On-chain collection synced from indexer:
```json
{
  "type": "live",
  "policy_id": "abc123def456...",
  "synced_at": "2025-01-28T12:00:00Z"
}
```

## Token ID Convention

| Collection Type | Token ID Format | Example |
|-----------------|-----------------|---------|
| Preview | Zero-padded numeric | `"000001"` |
| Live | `encoded_name` / `raw_asset_hex` | `"SquashuaChicken0001"` |

For live collections, the token ID equals the raw asset hex from the blockchain, enabling direct lookups by on-chain asset identifier.

## Docs

- [Collection Binary Format](docs/collection-binary-format.md) - Core binary format spec (~290KB for 10K tokens)
- [Fast Thumbnail Lookup Design](docs/fast-thumbnail-lookup.md) - URL design and caching strategy
- [PHF Index Design](docs/phf-index-design.md) - Perfect hash function details

## License

MIT OR Apache-2.0
