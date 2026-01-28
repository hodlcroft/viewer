use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "viewer")]
#[command(about = "CLI for syncing and managing NFT collection bundles")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Sync a collection from a blockchain
    Sync {
        #[command(subcommand)]
        chain: SyncChain,
    },

    /// Fetch and analyze a collection (dry run, no images)
    Fetch {
        #[command(subcommand)]
        chain: FetchChain,
    },

    /// Verify a bundle's integrity
    Verify {
        /// Path to the bundle directory
        path: PathBuf,
    },

    /// Show bundle info
    Info {
        /// Path to the bundle directory
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum SyncChain {
    /// Sync a Cardano collection by policy ID
    Cardano {
        /// Policy ID of the collection
        policy_id: String,

        /// Output directory for the bundle
        #[arg(short, long, default_value = "./output")]
        output: PathBuf,

        /// Config file path (default: configs/cardano/{policy_id}.toml)
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Skip image fetching (metadata only)
        #[arg(long)]
        skip_images: bool,
    },
}

#[derive(Subcommand)]
enum FetchChain {
    /// Fetch a Cardano collection by policy ID
    Cardano {
        /// Policy ID of the collection
        policy_id: String,

        /// Config file path (default: configs/cardano/{policy_id}.toml)
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Sync { chain } => match chain {
            SyncChain::Cardano {
                policy_id,
                output,
                config,
                skip_images,
            } => cmd_sync_cardano(&policy_id, &output, config, skip_images).await,
        },
        Commands::Fetch { chain } => match chain {
            FetchChain::Cardano { policy_id, config } => {
                cmd_fetch_cardano(&policy_id, config).await
            }
        },
        Commands::Verify { path } => {
            println!("Verifying bundle at {}", path.display());
            todo!("Verify not yet implemented")
        }
        Commands::Info { path } => cmd_info(&path),
    }
}

/// Fetch Cardano collection data and analyze traits without generating output files.
async fn cmd_fetch_cardano(policy_id: &str, config_path: Option<PathBuf>) -> anyhow::Result<()> {
    use viewer_ingest::{AssetSource, CnftToolsSource, TraitAnalysis};

    println!("Fetching Cardano collection: {}", policy_id);

    // Load config if provided
    let config = load_cardano_config(policy_id, config_path)?;
    let ignore_traits = config.traits.ignore.clone();

    // Fetch from CNFT.tools
    let source = CnftToolsSource::new();
    let assets = source.fetch_collection(policy_id).await?;

    println!("  Fetched {} assets", assets.len());

    // Analyze traits
    let analysis = TraitAnalysis::from_assets(&assets, &ignore_traits)?;

    println!("\nTrait Analysis:");
    println!("  {}", analysis.summary());

    // Show rarity distribution if available
    let with_rarity = assets.iter().filter(|a| a.rarity_rank.is_some()).count();
    if with_rarity > 0 {
        println!("\nRarity: {} assets have source rarity ranks", with_rarity);
    }

    // Show image URL stats
    let with_images = assets.iter().filter(|a| a.image_url.is_some()).count();
    let ipfs_images = assets
        .iter()
        .filter(|a| {
            a.image_url
                .as_ref()
                .is_some_and(|u| u.starts_with("ipfs://") || u.starts_with("Qm"))
        })
        .count();

    println!("\nImages:");
    println!("  {} assets have image URLs", with_images);
    println!("  {} are IPFS URLs", ipfs_images);

    // Estimate binary format size (assuming single source)
    let token_entry_size = viewer_binary::TokenEntry::entry_size(
        analysis.bitmap_size,
        false, // single source
    );
    // HCF index is separate: 6 bytes per token for U32U16
    let hcf_entry_size = viewer_binary::HcfIndexSize::U32U16.byte_size();
    let estimated_token_table = assets.len() * (token_entry_size + hcf_entry_size);
    println!("\nEstimated collection.bin:");
    println!(
        "  Token table: {} bytes ({} bytes/token)",
        estimated_token_table, token_entry_size
    );

    Ok(())
}

/// Set up file logging for the build process.
fn setup_build_logging(
    log_path: &std::path::Path,
) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    use std::io::Write;

    // Truncate and write header
    let mut file = std::fs::File::create(log_path)?;
    writeln!(
        file,
        "================================================================================\n\
         Build started: {}\n\
         ================================================================================\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    )?;
    drop(file);

    // Set up non-blocking file appender
    let file = std::fs::OpenOptions::new().append(true).open(log_path)?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file);

    // Initialize tracing with file layer (info level and above)
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_filter(EnvFilter::new("info")),
        )
        .init();

    Ok(guard)
}

/// Sync a Cardano collection: fetch, analyze, generate sprites, HCF bundles, and collection.bin.
async fn cmd_sync_cardano(
    policy_id: &str,
    output: &PathBuf,
    config_path: Option<PathBuf>,
    skip_images: bool,
) -> anyhow::Result<()> {
    use viewer_binary::{HcfMetadata, ImageFormat, SourceMetadata, SourcesSection, StringRef};
    use viewer_ingest::{
        AssetSource, CnftToolsSource, CollectionWriter, HcfBundler, HcfConfig, Pipeline,
        PipelineConfig, SpriteConfig, SpriteGenerator, SpriteLocation, TraitAnalysis, fetch_images,
        fetch_images_iiif,
    };

    println!("Syncing Cardano collection: {}", policy_id);

    // Load config
    let config = load_cardano_config(policy_id, config_path)?;
    let ignore_traits = config.traits.ignore.clone();

    // Initialize pipeline first so we can set up logging
    let pipeline_config = PipelineConfig {
        build_dir: PathBuf::from(".build"),
        ..Default::default()
    };
    let mut pipeline = Pipeline::new(policy_id, 0, pipeline_config)?;

    // Set up file logging
    let log_path = pipeline.dirs.build_log();
    let _log_guard = setup_build_logging(&log_path)?;
    println!("  Build log: {}", log_path.display());

    tracing::info!("Starting sync for policy_id={}", policy_id);

    // Fetch metadata from CNFT.tools
    println!("\n[1/5] Fetching collection metadata...");
    let source = CnftToolsSource::new();
    let mut assets = source.fetch_collection(policy_id).await?;

    // Sort by encoded_name for deterministic ordering
    assets.sort_by(|a, b| a.encoded_name.cmp(&b.encoded_name));

    println!("  Found {} assets", assets.len());
    tracing::info!(
        "Fetched {} assets from CNFT.tools (sorted by encoded_name)",
        assets.len()
    );

    // Update pipeline state with actual asset count
    pipeline.state.total_assets = assets.len();

    // Analyze traits
    println!("\n[2/5] Analyzing traits...");
    let analysis = TraitAnalysis::from_assets(&assets, &ignore_traits)?;
    println!("  {}", analysis.summary());
    tracing::info!("Trait analysis: {}", analysis.summary());

    println!("  Build directory: {}", pipeline.dirs.root.display());

    // Fetch images
    if !skip_images {
        let progress_cb = Box::new(
            |processed: usize, total: usize, fetched: usize, failed: usize| {
                print!(
                    "\r  Progress: {}/{} ({} new, {} failed)    ",
                    processed, total, fetched, failed
                );
                use std::io::Write;
                std::io::stdout().flush().ok();
            },
        );

        let result = if config.images.is_iiif() {
            println!("\n[3/5] Fetching images from IIIF...");
            fetch_images_iiif(
                &mut pipeline,
                &assets,
                policy_id,
                &config.images,
                Some(progress_cb),
            )
            .await?
        } else {
            println!("\n[3/5] Fetching images from IPFS...");
            fetch_images(&mut pipeline, &assets, Some(progress_cb)).await?
        };

        println!(
            "\r  Complete: {} fetched, {} skipped, {} failed    ",
            result.fetched,
            result.skipped,
            result.failed.len()
        );

        // Abort if any images failed
        if !result.failed.is_empty() {
            println!("\nFailed to fetch {} images:", result.failed.len());
            for id in &result.failed {
                println!("  - {}", id);
            }
            anyhow::bail!(
                "Cannot continue with {} failed images. Fix the issues and retry.",
                result.failed.len()
            );
        }
    } else {
        println!("\n[3/5] Skipping image fetch (--skip-images)");
    }

    // Generate sprites from raw images (auto-detects aspect ratio)
    println!("\n[4/5] Generating sprites...");
    let sprite_config: SpriteConfig;
    {
        // Collect raw image paths in asset order
        let mut raw_paths: Vec<std::path::PathBuf> = Vec::with_capacity(assets.len());
        let mut missing = Vec::new();

        for asset in &assets {
            if let Some(path) = pipeline.raw_exists(&asset.encoded_name) {
                raw_paths.push(path);
            } else {
                missing.push(asset.encoded_name.clone());
            }
        }

        if !missing.is_empty() {
            anyhow::bail!(
                "Cannot generate sprites: {} raw images missing. Run without --skip-images first.",
                missing.len()
            );
        }

        let total = raw_paths.len();
        let (sheets, _locations, detected_config) = SpriteGenerator::generate_batch_auto(
            pipeline.config.sprite_max_sheet_size,
            &raw_paths,
            &pipeline.dirs.sprites,
            |done, _| {
                if done % 64 == 0 || done == total {
                    print!("\r  Progress: {}/{}    ", done, total);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            },
        )?;

        sprite_config = detected_config;

        let total_size: u64 = sheets.iter().map(|s| s.file_size).sum();
        println!(
            "\r  Generated {} sprite sheets ({:.2} MB)    ",
            sheets.len(),
            total_size as f64 / 1024.0 / 1024.0
        );
        tracing::info!(
            "Generated {} sprite sheets, total size: {} bytes",
            sheets.len(),
            total_size
        );

        pipeline.state.sprites_complete = true;
        pipeline.save_state().ok();
    }

    // Generate HCF bundles from raw images
    println!("\n[5/5] Generating HCF bundles...");
    let hcf_result = {
        // Collect raw image paths in asset order (same as sprites)
        let raw_paths: Vec<std::path::PathBuf> = assets
            .iter()
            .filter_map(|a| pipeline.raw_exists(&a.encoded_name))
            .collect();

        let hcf_config = HcfConfig {
            shard_size: pipeline.config.hcf_shard_size,
            ..Default::default()
        };

        let total = raw_paths.len();
        let result =
            HcfBundler::bundle_batch(hcf_config, &raw_paths, &pipeline.dirs.hcf, |done, _| {
                if done % 100 == 0 || done == total {
                    print!("\r  Progress: {}/{}    ", done, total);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            })?;

        println!(
            "\r  Generated {} HCF shards ({:.2} MB, max image: {} KB)    ",
            result.shards.len(),
            result.total_size as f64 / 1024.0 / 1024.0,
            result.max_image_size / 1024
        );
        tracing::info!(
            "Generated {} HCF shards, total size: {} bytes, max image: {} bytes",
            result.shards.len(),
            result.total_size,
            result.max_image_size
        );

        pipeline.state.hcf_complete = true;
        pipeline.save_state().ok();

        result
    };

    // Write collection.bin with all metadata
    println!("\nWriting collection.bin...");
    let collection_bin_path = pipeline.dirs.root.join("collection.bin");
    {
        // Create sources section
        let sources = SourcesSection::new(vec![SourceMetadata {
            chain: StringRef(0),
            id: StringRef(1),
            token_count: assets.len() as u32,
            synced_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
        }]);

        // HCF metadata
        let hcf_metadata = HcfMetadata {
            shard_size: pipeline.config.hcf_shard_size as u32,
            shard_count: hcf_result.shards.len() as u16,
            image_format: ImageFormat::WebP,
            max_dimension: 2048,
        };

        let mut writer = CollectionWriter::new(
            sources,
            hcf_metadata,
            analysis.total_values(),
            hcf_result.total_size,
            hcf_result.max_image_size,
        )
        .ok_or_else(|| anyhow::anyhow!("Too many trait values for binary format"))?;

        // Add trait definitions
        for (trait_name, values) in analysis.trait_values() {
            let value_counts: Vec<(&str, u16)> =
                values.iter().map(|(v, c)| (v.as_str(), *c)).collect();
            writer.add_trait(trait_name, &value_counts)?;
        }

        // Calculate sprite locations using detected config
        let thumbs_per_sheet = sprite_config.thumbs_per_sheet();

        // Add tokens
        for (idx, asset) in assets.iter().enumerate() {
            let sheet = (idx as u32) / thumbs_per_sheet;
            let pos_in_sheet = (idx as u32) % thumbs_per_sheet;
            let col = pos_in_sheet % sprite_config.grid_columns;
            let row = pos_in_sheet / sprite_config.grid_columns;

            let sprite = SpriteLocation {
                sheet: sheet as u16,
                x: col as u8,
                y: row as u8,
            };

            let traits: Vec<(u8, u8)> = asset
                .traits
                .iter()
                .filter_map(|(name, values)| analysis.encode_trait(name, values))
                .collect();

            let token = viewer_ingest::TokenData {
                name: asset.display_name.clone(),
                asset_id: asset.encoded_name.clone(),
                encoded_name: asset.encoded_name.clone(),
                traits,
                rarity_rank: asset.rarity_rank.unwrap_or(0) as u16,
                rarity_score: 0, // TODO: Calculate rarity score
                sprite,
                source_index: None,
            };

            writer.add_token(token);
        }

        // Write with HCF locations
        writer.write_to_file(&collection_bin_path, &hcf_result.locations)?;

        let file_size = std::fs::metadata(&collection_bin_path)?.len();
        println!(
            "  Written {} ({:.2} KB, {} tokens)",
            collection_bin_path.display(),
            file_size as f64 / 1024.0,
            assets.len()
        );
        println!(
            "  Sprites: {}x{} cells, {}x{} grid, {} per sheet",
            sprite_config.thumb_width,
            sprite_config.thumb_height,
            sprite_config.grid_columns,
            sprite_config.grid_rows,
            sprite_config.thumbs_per_sheet()
        );
        tracing::info!(
            "Wrote collection.bin: {} bytes, {} tokens, sprite config: {}x{} @ {}x{}",
            file_size,
            assets.len(),
            sprite_config.thumb_width,
            sprite_config.thumb_height,
            sprite_config.grid_columns,
            sprite_config.grid_rows
        );
    }

    // Copy final output
    println!("\nOutput: {}", output.display());
    std::fs::create_dir_all(output)?;
    // TODO: Copy collection.bin, sprites, HCF to output directory

    Ok(())
}

/// Show info about an existing bundle.
fn cmd_info(path: &PathBuf) -> anyhow::Result<()> {
    let index_path = path.join("index.json");

    if index_path.exists() {
        // JSON bundle format
        let index = viewer_bundle::BundleIndex::read_from_file(&index_path)?;
        println!("Bundle info for {}", path.display());
        println!("  Format: JSON bundle (v{})", index.version);
        println!("  Image format: {}", index.image_format);
        println!("  Image count: {}", index.image_count);
        println!("  Shard count: {}", index.shard_count);
        if let Some(sprites) = &index.sprites {
            println!(
                "  Sprites: {}x{} grid, {} sheets",
                sprites.columns, sprites.rows, sprites.sheet_count
            );
        }
    } else {
        // Check for binary format
        let bin_path = path.join("collection.bin");
        if bin_path.exists() {
            println!("Bundle info for {}", path.display());
            println!("  Format: Binary (collection.bin)");
            // TODO: Read and display binary header info
            println!("  (Binary format parsing not yet implemented)");
        } else {
            anyhow::bail!(
                "No bundle found at {} (missing index.json and collection.bin)",
                path.display()
            );
        }
    }

    Ok(())
}

/// Load Cardano ingestion config from file or create default.
fn load_cardano_config(
    policy_id: &str,
    config_path: Option<PathBuf>,
) -> anyhow::Result<viewer_format::IngestionConfig> {
    let path =
        config_path.unwrap_or_else(|| PathBuf::from(format!("configs/cardano/{}.toml", policy_id)));

    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        let config: viewer_format::IngestionConfig = toml::from_str(&content)?;
        println!("  Loaded config: {}", path.display());
        Ok(config)
    } else {
        println!("  No config file found, using defaults");
        Ok(viewer_format::IngestionConfig::default())
    }
}
