# Ingestion Flow: Cardano Collections to Binary Format

## Overview

This document describes the pipeline for importing existing Cardano NFT collections into our optimized binary format. The system supports multiple data sources and generates HCF bundles for efficient image delivery.

```
+-----------------+     +-----------------+     +-----------------+
|   Data Source   | --> |    Sync CLI     | --> |  R2 Storage     |
|  (configurable) |     |  (transforms)   |     |  (output)       |
+-----------------+     +-----------------+     +-----------------+
         |                      |                       |
         v                      v                       v
  - CNFT.tools API       IngestionConfig         collection.bin
  - Maestro API          + HCF bundle gen        sprites_*.webp
  - (future sources)     + trait processing      images_*.hcf
                         + PHF building
```

## Data Sources

### Source Selection

The ingestion source is defined in the collection config file:

```toml
# configs/cardano/{policy_id}.toml

[source]
type = "cnft_tools"  # or "maestro"

# Source-specific options
[source.maestro]
api_key_env = "MAESTRO_API_KEY"
network = "mainnet"
```

### CNFT.tools (Default)

The `cnft-tools` crate provides:

```rust
pub struct CnftAsset {
    pub asset_id: String,           // Full asset ID (policy_id + asset_name)
    pub encoded_name: String,       // Hex-encoded asset name (our lookup key)
    pub name: String,               // Display name
    pub icon_url: Option<String>,   // IPFS/HTTP URL to image
    pub rarity_rank: u32,           // Pre-calculated rarity rank
    pub trait_count: Option<u32>,
    pub traits: HashMap<String, Vec<String>>,
    pub owner_stake_key: String,
    pub on_sale: Option<bool>,
}
```

**Pros**: Single API call returns everything including rarity ranks
**Cons**: Rate limits, not all collections indexed

### Maestro (Alternative)

For collections not on CNFT.tools or requiring fresher data:

```rust
// Via maestro indexer crate
pub struct MaestroAsset {
    pub asset_name: String,
    pub policy_id: String,
    pub fingerprint: String,
    pub metadata: serde_json::Value,  // On-chain metadata
    // ... other fields
}
```

**Pros**: Direct chain data, more complete coverage
**Cons**: Requires API key, no pre-calculated rarity, pagination needed

### Normalized Asset Type

Both sources normalize to a common type for processing:

```rust
pub struct NormalizedAsset {
    pub encoded_name: String,       // Hex asset name (lookup key)
    pub display_name: String,       // Human-readable name
    pub traits: HashMap<String, Vec<String>>,
    pub rarity_rank: Option<u32>,   // From source if available
    pub image_url: Option<String>,  // IPFS or HTTP URL to original image
}
```

## Ingestion Config

Full config format at `configs/cardano/{policy_id}.toml`:

```toml
# Display name override (optional)
name = "Toolheads"

# Data source configuration
[source]
type = "cnft_tools"  # "cnft_tools" | "maestro"

# Maestro-specific options (if type = "maestro")
[source.maestro]
api_key_env = "MAESTRO_API_KEY"
network = "mainnet"

# Trait configuration
[traits]
# Traits to exclude from filtering/indexing (unique identifiers, etc.)
ignore = [
    "Call Sign",
    "Serial Number",
]

# Normalize trait names for display
[traits.aliases]
"BACKGROUND" = "Background"
"BODY_TYPE" = "Body Type"

# Rarity configuration
[rarity]
# Use source rarity if available (default: true)
use_source = true

# If recalculating, exclude these traits from rarity score
exclude = ["Call Sign"]

# Sprite generation settings (optional, has defaults)
[sprites]
thumb_size = 150      # pixels
grid_size = 10        # 10x10 = 100 per sheet
quality = 80          # WebP quality 0-100

# HCF bundle settings (optional, has defaults)
[hcf]
max_dimension = 2048  # Max width/height for full images
shard_size_mb = 250   # Size of each HCF shard file
quality = 85          # WebP quality for full images
```

## Pipeline Stages

### Stage 1: Load Config & Initialize Source

```rust
async fn initialize_ingestion(policy_id: &str) -> Result<IngestionContext> {
    let config = load_config(policy_id)?;
    
    let source: Box<dyn AssetSource> = match config.source.source_type {
        SourceType::CnftTools => Box::new(CnftToolsSource::new()),
        SourceType::Maestro => {
            let api_key = std::env::var(&config.source.maestro.api_key_env)?;
            Box::new(MaestroSource::new(&api_key))
        }
    };
    
    Ok(IngestionContext { config, source, policy_id: policy_id.to_string() })
}
```

### Stage 2: Fetch & Normalize Assets

```rust
#[async_trait]
trait AssetSource {
    async fn fetch_collection(&self, policy_id: &str) -> Result<Vec<NormalizedAsset>>;
}

impl AssetSource for CnftToolsSource {
    async fn fetch_collection(&self, policy_id: &str) -> Result<Vec<NormalizedAsset>> {
        let api = CnftApi::default();
        let assets = api.get_for_policy(policy_id).await?;
        
        Ok(assets.into_iter().map(|a| NormalizedAsset {
            encoded_name: a.encoded_name,
            display_name: a.name,
            traits: a.traits,
            rarity_rank: Some(a.rarity_rank),  // CNFT.tools provides this
            image_url: a.icon_url,
        }).collect())
    }
}
```

### Stage 3: Calculate Rarity (if needed)

```rust
fn process_rarity(
    assets: &mut [NormalizedAsset],
    config: &IngestionConfig,
) {
    // Check if we should use source rarity
    if config.rarity.use_source {
        let has_source_rarity = assets.iter().all(|a| a.rarity_rank.is_some());
        if has_source_rarity {
            tracing::info!("Using rarity ranks from source");
            return;
        }
        tracing::info!("Source rarity not available, calculating...");
    }
    
    // Calculate rarity scores using trait frequency
    let excluded_traits: HashSet<_> = config.rarity.exclude.iter().collect();
    
    // ... frequency-based rarity calculation ...
}
```

### Stage 4: Analyze & Build Trait Schema

```rust
struct TraitAnalysis {
    schema: TraitSchema,
    bitmap_size: BitmapSize,
    total_trait_values: usize,
}

fn analyze_traits(
    assets: &[NormalizedAsset],
    config: &IngestionConfig,
) -> Result<TraitAnalysis> {
    // Collect all trait:value pairs (excluding ignored)
    let mut trait_values: HashMap<String, HashSet<String>> = HashMap::new();
    
    for asset in assets {
        for (trait_name, values) in &asset.traits {
            if config.traits.ignore.contains(trait_name) {
                continue;
            }
            // ... collect values ...
        }
    }
    
    let total_combinations: usize = trait_values.values().map(|v| v.len()).sum();
    
    // Select smallest bitmap size that fits
    let bitmap_size = match total_combinations {
        0..=64 => BitmapSize::U64,
        65..=128 => BitmapSize::U128,
        129..=256 => BitmapSize::U256,
        257..=512 => BitmapSize::U512,
        _ => return Err(anyhow!(
            "Collection has {} trait:value combinations (max 512). \
             Add traits to ignore list in config.",
            total_combinations
        )),
    };
    
    tracing::info!(
        "Trait analysis: {} values, using {:?} bitmap ({} bytes/token)",
        total_combinations,
        bitmap_size,
        bitmap_size.byte_size()
    );
    
    // Build schema with bitmap offsets...
    Ok(TraitAnalysis { schema, bitmap_size, total_trait_values: total_combinations })
}
```

### Stage 5: Build Token Table

```rust
fn build_token_table(
    assets: &[NormalizedAsset],
    analysis: &TraitAnalysis,
    config: &IngestionConfig,
) -> Vec<TokenEntry> {
    let mut tokens = Vec::with_capacity(assets.len());
    
    for (idx, asset) in assets.iter().enumerate() {
        // Build attribute bitmap based on bitmap_size
        let attributes = build_attribute_bitmap(asset, analysis);
        
        tokens.push(TokenEntry {
            index: idx as u16,
            encoded_name: asset.encoded_name.clone(),
            display_name: asset.display_name.clone(),
            attributes,
            rarity_rank: asset.rarity_rank.unwrap_or(0) as u16,
            sprite_sheet: (idx / 100) as u16,
            sprite_x: (idx % 10) as u8,
            sprite_y: ((idx % 100) / 10) as u8,
            // HCF location filled in during bundle generation
            hcf_offset: 0,
            hcf_length: 0,
        });
    }
    
    tokens
}
```

### Stage 6: Build Inverted Index & PHF

Same as before - create inverted index for trait filtering and PHF for O(1) asset lookups.

### Stage 7: Generate Sprites

Fetch thumbnails and composite into sprite sheets:

```rust
async fn generate_sprites(
    policy_id: &str,
    tokens: &[TokenEntry],
    assets: &[NormalizedAsset],
    output_dir: &Path,
    config: &SpriteConfig,
) -> Result<SpriteMetadata> {
    let thumb_size = config.thumb_size;  // e.g., 150
    let grid_size = config.grid_size;    // e.g., 10
    let tokens_per_sheet = grid_size * grid_size;  // 100
    
    let semaphore = Arc::new(Semaphore::new(50));  // Concurrency limit
    
    for sheet_idx in 0..(tokens.len() + tokens_per_sheet - 1) / tokens_per_sheet {
        let start = sheet_idx * tokens_per_sheet;
        let end = (start + tokens_per_sheet).min(tokens.len());
        
        // Fetch and resize images concurrently
        let futures: Vec<_> = assets[start..end]
            .iter()
            .map(|asset| fetch_and_resize(&asset.image_url, thumb_size, &semaphore))
            .collect();
        
        let images = futures::future::join_all(futures).await;
        
        // Composite into sprite sheet
        let sprite_sheet = composite_sprite_sheet(&images, grid_size, thumb_size);
        
        // Save as WebP
        save_webp(&sprite_sheet, output_dir.join(format!("sprites_{:03}.webp", sheet_idx)))?;
    }
    
    Ok(SpriteMetadata { thumb_size, columns: grid_size, rows: grid_size, ... })
}
```

### Stage 8: Generate HCF Bundles

Fetch full-size images and pack into HCF bundle shards:

```rust
async fn generate_hcf_bundles(
    tokens: &mut [TokenEntry],
    assets: &[NormalizedAsset],
    output_dir: &Path,
    config: &HcfConfig,
) -> Result<(HcfMetadata, HcfIndexSize)> {
    let shard_size = config.shard_size_mb * 1024 * 1024;  // 250 MB
    let max_dim = config.max_dimension;  // 2048
    
    let mut current_shard = 0u32;
    let mut current_offset = 0u64;  // Global offset across all shards
    let mut shard_buffer = Vec::with_capacity(shard_size);
    
    let mut max_image_size = 0u32;
    let semaphore = Arc::new(Semaphore::new(20));  // Lower concurrency for larger images
    
    for (idx, asset) in assets.iter().enumerate() {
        // Fetch and process image
        let image_bytes = fetch_and_encode_webp(
            &asset.image_url,
            max_dim,
            config.quality,
            &semaphore,
        ).await?;
        
        let image_len = image_bytes.len() as u32;
        max_image_size = max_image_size.max(image_len);
        
        // Check if we need a new shard
        if shard_buffer.len() + image_bytes.len() > shard_size {
            // Pad current shard to fixed size and write
            shard_buffer.resize(shard_size, 0);
            write_shard(&shard_buffer, output_dir, current_shard)?;
            
            current_shard += 1;
            shard_buffer.clear();
        }
        
        // Record HCF location in token
        tokens[idx].hcf_offset = current_offset;
        tokens[idx].hcf_length = image_len;
        
        // Append to shard buffer
        shard_buffer.extend_from_slice(&image_bytes);
        current_offset += image_len as u64;
        
        if idx % 100 == 0 {
            tracing::info!("Processed {}/{} images", idx, assets.len());
        }
    }
    
    // Write final shard (padded)
    if !shard_buffer.is_empty() {
        shard_buffer.resize(shard_size, 0);
        write_shard(&shard_buffer, output_dir, current_shard)?;
    }
    
    // Determine HCF index size based on actual data
    let total_size = current_offset;
    let hcf_index_size = select_hcf_index_size(total_size, max_image_size);
    
    tracing::info!(
        "HCF bundles: {} shards, {} total, max image {}, using {:?} ({} bytes/token)",
        current_shard + 1,
        format_bytes(total_size),
        format_bytes(max_image_size as u64),
        hcf_index_size,
        hcf_index_size.byte_size()
    );
    
    Ok((
        HcfMetadata {
            shard_size: shard_size as u32,
            shard_count: current_shard + 1,
            image_format: ImageFormat::WebP,
            max_dimension: max_dim as u16,
        },
        hcf_index_size,
    ))
}

fn select_hcf_index_size(total_size: u64, max_image_size: u32) -> HcfIndexSize {
    let needs_large_offset = total_size > u32::MAX as u64;
    let needs_large_length = max_image_size > u16::MAX as u32;
    
    match (needs_large_offset, needs_large_length) {
        (false, false) => HcfIndexSize::U32_U16,  // 6 bytes
        (false, true) => HcfIndexSize::U32_U24,   // 7 bytes
        (true, _) => HcfIndexSize::U40_U24,       // 8 bytes
    }
}

fn write_shard(data: &[u8], output_dir: &Path, shard_index: u32) -> Result<()> {
    let path = output_dir.join(format!("images_{:03}.hcf", shard_index));
    std::fs::write(&path, data)?;
    tracing::info!("Wrote shard {} ({} bytes)", shard_index, data.len());
    Ok(())
}
```

### Stage 9: Pack Binary Format

```rust
fn pack_collection(
    analysis: &TraitAnalysis,
    tokens: &[TokenEntry],
    inverted_index: &InvertedIndex,
    phf: &AssetIdPhf,
    sprite_meta: &SpriteMetadata,
    hcf_meta: &HcfMetadata,
    hcf_index_size: HcfIndexSize,
) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    
    // Reserve header space
    buffer.extend_from_slice(&[0u8; HEADER_SIZE]);
    
    // Write sections, recording offsets
    let string_table_offset = write_string_table(&mut buffer, ...)?;
    let trait_schema_offset = write_trait_schema(&mut buffer, &analysis.schema)?;
    let trait_index_offset = write_inverted_index(&mut buffer, inverted_index)?;
    let token_table_offset = write_token_table(
        &mut buffer,
        tokens,
        analysis.bitmap_size,
        hcf_index_size,
    )?;
    let phf_offset = write_phf(&mut buffer, phf)?;
    let sprites_offset = write_sprite_metadata(&mut buffer, sprite_meta)?;
    let hcf_offset = write_hcf_metadata(&mut buffer, hcf_meta)?;
    
    // Write header
    let header = Header {
        magic: *b"COLL",
        version: 1,
        token_count: tokens.len() as u32,
        bitmap_size: analysis.bitmap_size,
        hcf_index_size,
        string_table_offset,
        trait_schema_offset,
        trait_index_offset,
        token_table_offset,
        phf_offset,
        sprites_offset,
        hcf_metadata_offset: hcf_offset,
        ..Default::default()
    };
    
    buffer[..HEADER_SIZE].copy_from_slice(&header.to_bytes());
    
    Ok(buffer)
}
```

### Stage 10: Upload to R2

```rust
async fn upload_to_r2(
    bucket: &Bucket,
    policy_id: &str,
    output_dir: &Path,
) -> Result<()> {
    let base_path = format!("collections/cardano/{}", policy_id);
    
    // Upload collection.bin
    let collection_data = std::fs::read(output_dir.join("collection.bin"))?;
    bucket.put(&format!("{}/collection.bin", base_path), collection_data).await?;
    
    // Upload sprite sheets
    for entry in glob::glob(&output_dir.join("sprites_*.webp").to_string_lossy())? {
        let path = entry?;
        let filename = path.file_name().unwrap().to_string_lossy();
        let data = std::fs::read(&path)?;
        bucket.put(&format!("{}/{}", base_path, filename), data).await?;
    }
    
    // Upload HCF bundles
    for entry in glob::glob(&output_dir.join("images_*.hcf").to_string_lossy())? {
        let path = entry?;
        let filename = path.file_name().unwrap().to_string_lossy();
        let data = std::fs::read(&path)?;
        bucket.put(&format!("{}/{}", base_path, filename), data).await?;
        tracing::info!("Uploaded {}", filename);
    }
    
    tracing::info!("Upload complete: {}", base_path);
    Ok(())
}
```

## CLI Interface

```
viewer-cli sync <POLICY_ID> [OPTIONS]

Arguments:
  <POLICY_ID>  The Cardano policy ID to sync

Options:
  --config <PATH>       Config file path (default: configs/cardano/{policy_id}.toml)
  --output <DIR>        Local output directory (default: ./output/{policy_id})
  --source <TYPE>       Override source type: cnft_tools, maestro
  --upload              Upload to R2 after generation
  --skip-sprites        Skip sprite generation (use existing)
  --skip-hcf            Skip HCF bundle generation (use existing)
  --recalc-rarity       Force rarity recalculation even if source provides it
  --dry-run             Analyze only, don't generate files
  -v, --verbose         Verbose output

Examples:
  # Full sync with upload
  viewer-cli sync 285c0b8e91ba323da4ca083c9db837e111dafbf3143ece4d03eba8f4 --upload

  # Dry run to check trait analysis and sizing
  viewer-cli sync <policy_id> --dry-run

  # Re-sync just collection.bin (sprites/HCF unchanged)
  viewer-cli sync <policy_id> --skip-sprites --skip-hcf
```

## Output Structure

```
output/{policy_id}/
  collection.bin          # ~350 KB - trait filtering, lookups, HCF index
  sprites_000.webp        # ~1.5 MB each - thumbnail grids (100 tokens)
  sprites_001.webp
  ...
  images_000.hcf          # 250 MB each - full-size WebP images (zero-padded)
  images_001.hcf
  ...
```

For a 10K collection with 100KB average images:
- `collection.bin`: ~350 KB
- Sprites: 100 sheets x 1.5 MB = 150 MB
- HCF bundles: 4 shards x 250 MB = 1 GB

## Image Delivery Architecture

```
+---------------------------------------------------------------------+
|                        Frontend (Browser)                           |
+---------------------------------------------------------------------+
                |                              |
                | Grid view (thumbnails)       | Detail view (full image)
                v                              v
+---------------------------+    +-----------------------------------+
|  R2: Sprite Sheets        |    |  R2: HCF Bundles                  |
|  /sprites_000.webp        |    |  /images_000.hcf                  |
|  /sprites_001.webp        |    |  /images_001.hcf                  |
|  ...                      |    |                                   |
|                           |    |  Direct range requests using      |
|  Pre-composited during    |    |  offset/length from collection.bin|
|  sync, 150px thumbnails   |    |  No HCF index fetch needed!       |
+---------------------------+    +-----------------------------------+
```

**Grid View**: Load sprite sheets (fast, single request per 100 tokens)
**Detail View**: Range request directly into HCF bundle using offset/length from collection.bin

## Error Handling

### Recoverable Errors

| Error | Recovery |
|-------|----------|
| Image fetch 404 | Use placeholder, log warning |
| Image decode failure | Use placeholder, log warning |
| IPFS timeout | Retry with backoff, try alternate gateway |

### Fatal Errors

| Error | Action |
|-------|--------|
| Source API rate limited | Exit with retry-after suggestion |
| >512 trait:value combos | Exit with high-cardinality traits to ignore |
| Policy ID not found | Exit with error |
| Invalid config file | Exit with parse error details |
| Disk full during HCF gen | Exit with space requirements |

## Performance Considerations

### Memory Usage

For a 10K collection:
- Normalized assets: ~20 MB
- Token table: ~2 MB
- Sprite processing: ~100 MB peak (batched)
- HCF generation: ~50 MB peak (streaming)

### Network

- CNFT.tools: Single request, ~5-10 MB response
- Image fetching: 10K images x 100KB = 1 GB
  - Sprite thumbnails: 50 concurrent, smaller images
  - HCF full images: 20 concurrent, larger images
  - Both use retry with exponential backoff

### Disk I/O

- Sprites: 150 MB total
- HCF bundles: 1 GB total (streamed, not all in memory)
- collection.bin: ~350 KB

Total output: ~1.2 GB per 10K collection

## Future Enhancements

1. **Incremental sync**: Track changes, update only modified tokens
2. **Resume support**: Checkpoint progress for interrupted syncs
3. **Parallel shard writes**: Write HCF shards concurrently
4. **Additional sources**: Blockfrost, custom indexers
5. **Multi-chain**: Extend to Ethereum, Solana
6. **Validation mode**: Verify existing files against source
