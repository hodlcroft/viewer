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

    /// Pinata operations
    Pinata {
        #[command(subcommand)]
        action: PinataAction,
    },

    /// CID analysis and computation
    Cid {
        #[command(subcommand)]
        action: CidAction,
    },
}

#[derive(Subcommand)]
enum PinataAction {
    /// Cancel all pending pin requests in the queue
    PurgeQueue,

    /// Show status of pending pin requests
    QueueStatus,
}

#[derive(Subcommand)]
enum CidAction {
    /// Analyze CIDs from a collection and compare with local files
    Check {
        /// Policy ID of the collection
        policy_id: String,

        /// Directory containing local image files to compare
        #[arg(short, long)]
        local_dir: Option<PathBuf>,
    },

    /// Compute CID for a local file
    Compute {
        /// Path to the file
        path: PathBuf,

        /// Try different settings to match a target CID
        #[arg(short, long)]
        target: Option<String>,
    },

    /// Parse and display information about a CID
    Info {
        /// CID string to analyze
        cid: String,
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
    // Load .env file if present
    dotenvy::dotenv().ok();

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
        Commands::Pinata { action } => match action {
            PinataAction::PurgeQueue => cmd_pinata_purge_queue().await,
            PinataAction::QueueStatus => cmd_pinata_queue_status().await,
        },
        Commands::Cid { action } => match action {
            CidAction::Check {
                policy_id,
                local_dir,
            } => cmd_cid_check(&policy_id, local_dir).await,
            CidAction::Compute { path, target } => cmd_cid_compute(&path, target),
            CidAction::Info { cid } => cmd_cid_info(&cid),
        },
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

/// A MakeWriter that opens, writes, and syncs for each log event.
/// This is slower but guarantees immediate visibility.
#[derive(Clone)]
struct ImmediateFileWriter {
    path: std::path::PathBuf,
}

impl ImmediateFileWriter {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ImmediateFileWriter {
    type Writer = ImmediateWriter;

    fn make_writer(&'a self) -> Self::Writer {
        // Open file in append mode for each write
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .expect("Failed to open log file");
        ImmediateWriter { file }
    }
}

/// Writer that syncs after being dropped (when the log line is complete).
struct ImmediateWriter {
    file: std::fs::File,
}

impl std::io::Write for ImmediateWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Drop for ImmediateWriter {
    fn drop(&mut self) {
        // Sync when the writer is dropped (after each log line)
        let _ = self.file.sync_data();
    }
}

fn setup_build_logging(log_path: &std::path::Path) -> anyhow::Result<()> {
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
    file.sync_all()?;
    drop(file);

    // Use immediate writer that syncs after each log event
    let writer = ImmediateFileWriter::new(log_path.to_path_buf());

    // Initialize tracing with file layer (info level and above)
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_filter(EnvFilter::new("info")),
        )
        .init();

    Ok(())
}

/// Sync a Cardano collection: fetch, analyze, generate sprites, HCF bundles, and collection.bin.
async fn cmd_sync_cardano(
    policy_id: &str,
    output: &PathBuf,
    config_path: Option<PathBuf>,
    skip_images: bool,
) -> anyhow::Result<()> {
    use viewer_binary::{
        HcfMetadata, ImageFormat, SourceMetadata, SourcesSection, SpriteIndexBuilder, StringRef,
    };
    use viewer_ingest::{
        AssetSource, CnftToolsSource, CollectionWriter, HcfBundleResult, HcfBundler, HcfConfig,
        NftcdnClient, PinataClient, Pipeline, PipelineConfig, SpriteConfig, SpriteGenerator,
        TraitAnalysis, fetch_images, fetch_images_iiif, fetch_images_nftcdn,
        fetch_thumbnails_pinata,
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
    setup_build_logging(&log_path)?;
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
    println!("\n[2/6] Analyzing traits...");
    let analysis = TraitAnalysis::from_assets(&assets, &ignore_traits)?;
    println!("  {}", analysis.summary());
    tracing::info!("Trait analysis: {}", analysis.summary());

    println!("  Build directory: {}", pipeline.dirs.root.display());

    // Write collection.bin pass 1 (without HCF locations)
    // This establishes deterministic ordering and can be uploaded for testing
    println!("\n[3/6] Writing collection.bin (pass 1 - no HCF)...");
    let collection_bin_path = pipeline.dirs.root.join("collection.bin");
    let _sprite_config_placeholder = (); // Sprite config determined after sprite generation
    {
        use viewer_binary::{HcfMetadata, ImageFormat, SourceMetadata, SourcesSection, StringRef};

        let sources = SourcesSection::new(vec![SourceMetadata {
            chain: StringRef(0),
            id: StringRef(1),
            token_count: assets.len() as u32,
            synced_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
        }]);

        // Placeholder HCF metadata (will be updated in pass 2)
        let hcf_metadata = HcfMetadata {
            shard_size: pipeline.config.hcf_shard_size as u32,
            shard_count: 1,
            image_format: ImageFormat::WebP,
            max_dimension: 2048,
        };

        let mut writer = CollectionWriter::with_options(
            sources,
            hcf_metadata,
            analysis.total_values(),
            0, // total_hcf_size unknown yet
            0, // max_image_size unknown yet
            config.rarity.hide,
        )
        .ok_or_else(|| anyhow::anyhow!("Too many trait values for binary format"))?;

        // Add trait definitions
        for (trait_name, values) in analysis.trait_values() {
            let value_counts: Vec<(&str, u16)> =
                values.iter().map(|(v, c)| (v.as_str(), *c)).collect();
            writer.add_trait(trait_name, &value_counts)?;
        }

        // Add tokens
        for asset in &assets {
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
                rarity_score: 0,
                source_index: None,
            };

            writer.add_token(token)?;
        }

        // Write pass 1 (without HCF locations)
        writer.write_to_file_without_hcf(&collection_bin_path)?;

        let file_size = std::fs::metadata(&collection_bin_path)?.len();
        println!(
            "  Written {} ({:.2} KB, {} tokens) - ready for upload",
            collection_bin_path.display(),
            file_size as f64 / 1024.0,
            assets.len()
        );
        tracing::info!(
            "Wrote collection.bin pass 1: {} bytes, {} tokens (no HCF)",
            file_size,
            assets.len()
        );
    }

    // Pinata mode: pin CIDs and fetch thumbnails
    // Standard mode: fetch raw images from IPFS/IIIF
    let use_pinata = config.pinata.is_enabled();
    let pinata_client = if use_pinata {
        Some(PinataClient::from_env()?)
    } else {
        None
    };

    if use_pinata {
        let pinata = pinata_client.as_ref().unwrap();
        let group_id = config.pinata.group_id.as_ref().unwrap();

        // Check if all thumbnails already exist locally - if so, skip Pinata sync
        let thumbnails_dir = pipeline.dirs.root.join("thumbnails");
        let all_thumbnails_present = assets.iter().all(|asset| {
            ["png", "jpg", "webp", "gif"].iter().any(|ext| {
                thumbnails_dir
                    .join(format!("{}.{}", asset.encoded_name, ext))
                    .exists()
            })
        });

        if all_thumbnails_present {
            println!(
                "\n[4/6] All {} thumbnails present, skipping Pinata sync",
                assets.len()
            );
        } else {
            // Use pre-configured group ID
            println!("\n[4a/6] Setting up Pinata group...");
            println!("  Using group ID: {}", group_id);

            // Extract (name, CID) pairs from assets
            let pin_items: Vec<(String, String)> = assets
                .iter()
                .filter_map(|a| {
                    a.image_url
                        .as_ref()
                        .and_then(|url| viewer_ingest::extract_cid(url))
                        .map(|cid| (a.display_name.clone(), cid))
                })
                .collect();

            println!(
                "  Ensuring {} CIDs are pinned (rate limited to ~150/min)...",
                pin_items.len()
            );
            let pinned = pinata
                .ensure_cids_pinned(
                    group_id,
                    &pin_items,
                    Some(&|done, total| {
                        print!("\r  Queueing pins: {}/{}    ", done, total);
                        use std::io::Write;
                        std::io::stdout().flush().ok();
                    }),
                )
                .await?;
            println!("\r  Queued {} new CIDs for pinning    ", pinned);

            // Extract just CIDs for wait_for_pins
            let cids: Vec<String> = pin_items.into_iter().map(|(_, cid)| cid).collect();

            // Wait for pins to complete if we queued any
            if pinned > 0 {
                println!("  Waiting for pins to complete...");
                let failed_pins = pinata
                    .wait_for_pins(
                        &cids,
                        Some(&|completed, total, status| {
                            print!("\r  Pin status: {}/{} ({})    ", completed, total, status);
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }),
                    )
                    .await?;

                if !failed_pins.is_empty() {
                    println!("\r  Warning: {} CIDs failed to pin    ", failed_pins.len());
                    for cid in failed_pins.iter().take(5) {
                        println!("    - {}", cid);
                    }
                    if failed_pins.len() > 5 {
                        println!("    ... and {} more", failed_pins.len() - 5);
                    }
                } else {
                    println!("\r  All pins completed successfully    ");
                }
            }

            // Fetch thumbnails
            if !skip_images {
                println!("\n[4b/6] Fetching thumbnails from Pinata...");
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

                let result = fetch_thumbnails_pinata(
                    &mut pipeline,
                    &assets,
                    pinata,
                    config.pinata.thumbnail_size,
                    Some(progress_cb),
                )
                .await?;

                println!(
                    "\r  Complete: {} fetched, {} skipped, {} failed    ",
                    result.fetched,
                    result.skipped,
                    result.failed.len()
                );

                if !result.failed.is_empty() {
                    tracing::error!("Failed to fetch {} thumbnails", result.failed.len());
                    println!("\nFailed to fetch {} thumbnails:", result.failed.len());
                    for id in result.failed.iter().take(10) {
                        tracing::error!("  Failed thumbnail: {}", id);
                        println!("  - {}", id);
                    }
                    if result.failed.len() > 10 {
                        println!("  ... and {} more", result.failed.len() - 10);
                    }
                    anyhow::bail!(
                        "Cannot continue with {} failed thumbnails. Fix the issues and retry.",
                        result.failed.len()
                    );
                }
            } else {
                println!("\n[4b/6] Skipping thumbnail fetch (--skip-images)");
            }
        } // end if !all_thumbnails_present
    } else {
        // Standard IPFS/IIIF fetch
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

            let result = if config.images.is_nftcdn() {
                println!("\n[4/6] Fetching images from NFTCDN...");
                let nftcdn = NftcdnClient::from_env()?;
                fetch_images_nftcdn(
                    &mut pipeline,
                    &assets,
                    policy_id,
                    &nftcdn,
                    Some(progress_cb),
                )
                .await?
            } else if config.images.is_iiif() {
                println!("\n[4/6] Fetching images from IIIF...");
                fetch_images_iiif(
                    &mut pipeline,
                    &assets,
                    policy_id,
                    &config.images,
                    Some(progress_cb),
                )
                .await?
            } else {
                println!("\n[4/6] Fetching images from IPFS...");
                fetch_images(&mut pipeline, &assets, &config.images, Some(progress_cb)).await?
            };

            println!(
                "\r  Complete: {} fetched, {} skipped, {} failed    ",
                result.fetched,
                result.skipped,
                result.failed.len()
            );

            // Abort if any images failed
            if !result.failed.is_empty() {
                tracing::error!("Failed to fetch {} images", result.failed.len());
                println!("\nFailed to fetch {} images:", result.failed.len());
                for id in &result.failed {
                    tracing::error!("  Failed image: {}", id);
                    println!("  - {}", id);
                }
                anyhow::bail!(
                    "Cannot continue with {} failed images. Fix the issues and retry.",
                    result.failed.len()
                );
            }
        } else {
            println!("\n[4/6] Skipping image fetch (--skip-images)");
        }
    }

    // Generate sprites from images
    // Pinata mode: uses thumbnails/ directory (already sized PNGs)
    // Standard mode: uses raw/ directory (auto-detects aspect ratio)
    println!("\n[5/6] Generating sprites...");
    let sprite_config_actual: SpriteConfig;
    {
        // Collect image paths in asset order
        let mut image_paths: Vec<std::path::PathBuf> = Vec::with_capacity(assets.len());
        let mut missing = Vec::new();

        if use_pinata {
            // Use thumbnails directory for Pinata mode (may be png, jpg, webp, or gif)
            let thumbnails_dir = pipeline.dirs.root.join("thumbnails");
            for asset in &assets {
                let found = ["png", "jpg", "webp", "gif"]
                    .iter()
                    .map(|ext| thumbnails_dir.join(format!("{}.{}", asset.encoded_name, ext)))
                    .find(|p| p.exists());
                if let Some(path) = found {
                    image_paths.push(path);
                } else {
                    missing.push(asset.encoded_name.clone());
                }
            }
        } else {
            // Use raw directory for standard mode
            for asset in &assets {
                if let Some(path) = pipeline.raw_exists(&asset.encoded_name) {
                    image_paths.push(path);
                } else {
                    missing.push(asset.encoded_name.clone());
                }
            }
        }

        if !missing.is_empty() {
            let image_type = if use_pinata {
                "thumbnails"
            } else {
                "raw images"
            };
            anyhow::bail!(
                "Cannot generate sprites: {} {} missing. Run without --skip-images first.",
                missing.len(),
                image_type
            );
        }

        let total = image_paths.len();
        let (sheets, _locations, detected_config) = SpriteGenerator::generate_batch_auto(
            pipeline.config.sprite_max_sheet_size,
            &image_paths,
            &pipeline.dirs.sprites,
            |done, _| {
                if done % 64 == 0 || done == total {
                    print!("\r  Progress: {}/{}    ", done, total);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            },
        )?;

        sprite_config_actual = detected_config;

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

        // Generate sprites.bin index file
        let sprites_bin_path = pipeline.dirs.root.join("sprites.bin");
        let thumbs_per_sheet = sprite_config_actual.thumbs_per_sheet();
        let mut sprite_builder = SpriteIndexBuilder::new(
            sheets.len() as u16,
            sprite_config_actual.thumb_width as u16,
            sprite_config_actual.thumb_height as u16,
            sprite_config_actual.grid_columns as u8,
            sprite_config_actual.grid_rows as u8,
        );

        for (idx, asset) in assets.iter().enumerate() {
            let sheet = (idx as u32) / thumbs_per_sheet;
            let pos_in_sheet = (idx as u32) % thumbs_per_sheet;
            let col = pos_in_sheet % sprite_config_actual.grid_columns;
            let row = pos_in_sheet / sprite_config_actual.grid_columns;

            sprite_builder.add(&asset.encoded_name, sheet as u16, col as u8, row as u8);
        }

        let sprite_index_data = sprite_builder.build();
        std::fs::write(&sprites_bin_path, &sprite_index_data)?;
        println!(
            "  Written {} ({:.2} KB, {} entries)",
            sprites_bin_path.display(),
            sprite_index_data.len() as f64 / 1024.0,
            assets.len()
        );
        tracing::info!(
            "Wrote sprites.bin: {} bytes, {} entries",
            sprite_index_data.len(),
            assets.len()
        );

        pipeline.state.sprites_complete = true;
        pipeline.save_state().ok();
    }

    // Generate HCF bundles from raw images (skip in Pinata mode)
    let hcf_result = if use_pinata {
        println!("\n[6/6] Skipping HCF generation (Pinata mode - viewer fetches from gateway)");
        tracing::info!("Skipping HCF generation in Pinata mode");

        // Return empty result - no HCF files generated
        HcfBundleResult {
            shards: vec![],
            locations: vec![],
            total_size: 0,
            max_image_size: 0,
        }
    } else {
        println!("\n[6/6] Generating HCF bundles...");

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

    // Write collection.bin pass 2 (with actual HCF locations and sprite config)
    let pass2_label = if use_pinata {
        "final, no HCF"
    } else {
        "final with HCF"
    };
    println!("\nWriting collection.bin (pass 2 - {})...", pass2_label);
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

        // Detect image format
        // Pinata mode: use WebP (gateway will convert)
        // Standard mode: detect from first raw image
        let image_format = if use_pinata {
            ImageFormat::WebP
        } else {
            assets
                .first()
                .and_then(|a| pipeline.raw_exists(&a.encoded_name))
                .and_then(|p| p.extension().map(|e| e.to_owned()))
                .and_then(|ext| ext.to_str().map(|s| s.to_lowercase()))
                .map(|ext| match ext.as_str() {
                    "jpg" | "jpeg" => ImageFormat::Jpeg,
                    "png" => ImageFormat::Png,
                    "webp" => ImageFormat::WebP,
                    "avif" => ImageFormat::Avif,
                    _ => ImageFormat::WebP,
                })
                .unwrap_or(ImageFormat::WebP)
        };

        // HCF metadata
        // shard_count = 0 indicates Pinata mode (no HCF files)
        let hcf_metadata = HcfMetadata {
            shard_size: pipeline.config.hcf_shard_size as u32,
            shard_count: hcf_result.shards.len() as u16,
            image_format,
            max_dimension: 2048,
        };

        let mut writer = CollectionWriter::with_options(
            sources,
            hcf_metadata,
            analysis.total_values(),
            hcf_result.total_size,
            hcf_result.max_image_size,
            config.rarity.hide,
        )
        .ok_or_else(|| anyhow::anyhow!("Too many trait values for binary format"))?;

        // Add trait definitions
        for (trait_name, values) in analysis.trait_values() {
            let value_counts: Vec<(&str, u16)> =
                values.iter().map(|(v, c)| (v.as_str(), *c)).collect();
            writer.add_trait(trait_name, &value_counts)?;
        }

        // Add tokens
        for asset in &assets {
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
                source_index: None,
            };

            writer.add_token(token)?;
        }

        // Write collection.bin
        if use_pinata {
            // Pinata mode: no HCF locations
            writer.write_to_file_without_hcf(&collection_bin_path)?;
        } else {
            // Standard mode: with HCF locations
            writer.write_to_file(&collection_bin_path, &hcf_result.locations)?;
        }

        let file_size = std::fs::metadata(&collection_bin_path)?.len();
        println!(
            "  Written {} ({:.2} KB, {} tokens)",
            collection_bin_path.display(),
            file_size as f64 / 1024.0,
            assets.len()
        );
        println!(
            "  Sprites: {}x{} cells, {}x{} grid, {} per sheet",
            sprite_config_actual.thumb_width,
            sprite_config_actual.thumb_height,
            sprite_config_actual.grid_columns,
            sprite_config_actual.grid_rows,
            sprite_config_actual.thumbs_per_sheet()
        );
        if use_pinata {
            println!("  Mode: Pinata (viewer fetches images from gateway)");
        }
        tracing::info!(
            "Wrote collection.bin pass 2: {} bytes, {} tokens, sprite config: {}x{} @ {}x{}, pinata_mode: {}",
            file_size,
            assets.len(),
            sprite_config_actual.thumb_width,
            sprite_config_actual.thumb_height,
            sprite_config_actual.grid_columns,
            sprite_config_actual.grid_rows,
            use_pinata
        );
    }

    // Write pinata.json config if in Pinata mode
    if use_pinata {
        let pinata_config_path = pipeline.dirs.root.join("pinata.json");
        let gateway_host = pinata_client
            .as_ref()
            .and_then(|p| p.gateway_host().map(|s| s.to_string()))
            .unwrap_or_default();

        let pinata_json = serde_json::json!({
            "enabled": true,
            "gateway_host": gateway_host,
            "group_id": config.pinata.group_id,
        });

        std::fs::write(
            &pinata_config_path,
            serde_json::to_string_pretty(&pinata_json)?,
        )?;
        println!(
            "  Written {} (gateway: {})",
            pinata_config_path.display(),
            gateway_host
        );
        tracing::info!("Wrote pinata.json with gateway: {}", gateway_host);
    }

    // Copy final output
    println!("\nOutput: {}", output.display());
    std::fs::create_dir_all(output)?;
    // TODO: Copy collection.bin, sprites, HCF to output directory

    Ok(())
}

/// Cancel all pending pin requests in Pinata queue.
async fn cmd_pinata_purge_queue() -> anyhow::Result<()> {
    use viewer_ingest::PinataClient;

    println!("Connecting to Pinata...");
    let pinata = PinataClient::from_env()?;

    // First show current queue status
    let jobs = pinata.query_pin_requests(None).await?;

    if jobs.is_empty() {
        println!("Pin queue is empty, nothing to cancel.");
        return Ok(());
    }

    // Count by status
    let mut status_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for job in &jobs {
        *status_counts.entry(&job.status).or_insert(0) += 1;
    }

    println!("Found {} pending pin requests:", jobs.len());
    for (status, count) in &status_counts {
        println!("  {}: {}", status, count);
    }

    println!("\nCancelling all requests...");
    let cancelled = pinata.cancel_all_pins().await?;

    println!("Cancelled {} pin requests.", cancelled);

    Ok(())
}

/// Show status of pending pin requests in Pinata queue.
async fn cmd_pinata_queue_status() -> anyhow::Result<()> {
    use viewer_ingest::PinataClient;

    println!("Connecting to Pinata...");
    let pinata = PinataClient::from_env()?;

    let jobs = pinata.query_pin_requests(None).await?;

    if jobs.is_empty() {
        println!("Pin queue is empty.");
        return Ok(());
    }

    // Count by status
    let mut status_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for job in &jobs {
        *status_counts.entry(&job.status).or_insert(0) += 1;
    }

    println!("Pin queue status ({} total):", jobs.len());
    for (status, count) in &status_counts {
        println!("  {}: {}", status, count);
    }

    // Show a few sample entries
    if jobs.len() <= 10 {
        println!("\nAll requests:");
        for job in &jobs {
            println!(
                "  {} - {} [{}]",
                job.cid,
                job.name.as_deref().unwrap_or("(unnamed)"),
                job.status
            );
        }
    } else {
        println!("\nFirst 10 requests:");
        for job in jobs.iter().take(10) {
            println!(
                "  {} - {} [{}]",
                job.cid,
                job.name.as_deref().unwrap_or("(unnamed)"),
                job.status
            );
        }
        println!("  ... and {} more", jobs.len() - 10);
    }

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

            // Parse and display binary header
            let data = std::fs::read(&bin_path)?;
            if data.len() >= viewer_binary::HEADER_SIZE {
                let header_bytes: [u8; viewer_binary::HEADER_SIZE] =
                    data[..viewer_binary::HEADER_SIZE].try_into().unwrap();
                let header = viewer_binary::Header::from_bytes(&header_bytes);

                println!("  Version: {}", header.version);
                println!("  Flags: 0x{:04x}", header.flags);
                println!("    - hide_rarity: {}", header.hide_rarity());
                println!("    - multi_source: {}", header.is_multi_source());
                println!("  Token count: {}", header.token_count);
                println!("  Trait count: {}", header.trait_count);
                if let Some(bitmap_size) = header.bitmap_size() {
                    println!("  Bitmap size: {}", bitmap_size);
                }
                if let Some(hcf_index_size) = header.hcf_index_size() {
                    println!("  HCF index size: {:?}", hcf_index_size);
                }
                println!("  Source count: {}", header.source_count);

                // Show section offsets
                println!("  Sections:");
                println!("    string_table: 0x{:x}", header.string_table_offset);
                println!("    trait_schema: 0x{:x}", header.trait_schema_offset);
                println!("    token_table: 0x{:x}", header.token_table_offset);
                println!("    hcf_metadata: 0x{:x}", header.hcf_metadata_offset);
                println!("    hcf_index: 0x{:x}", header.hcf_index_offset);
                println!("    asset_id_index: 0x{:x}", header.asset_id_index_offset);

                // Show first few tokens' rarity data
                if let Some(bitmap_size) = header.bitmap_size() {
                    let token_fixed_size = viewer_binary::TOKEN_FIXED_SIZE;
                    let bitmap_bytes = bitmap_size.byte_size();
                    let token_entry_size = token_fixed_size + bitmap_bytes;
                    let token_table_start = header.token_table_offset as usize;

                    println!("  Sample tokens (first 5):");
                    for i in 0..5.min(header.token_count as usize) {
                        let offset = token_table_start + i * token_entry_size;
                        if offset + token_fixed_size <= data.len() {
                            let entry = viewer_binary::TokenEntry::read_fixed(
                                &data[offset..],
                                header.is_multi_source(),
                            );
                            println!(
                                "    Token {}: rarity_rank={}, rarity_score={}, name_ref=0x{:x}",
                                i, entry.rarity_rank, entry.rarity_score, entry.name_ref
                            );
                        }
                    }
                }
            } else {
                println!("  (File too small to parse header)");
            }
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

/// Check CIDs from a collection against local files.
async fn cmd_cid_check(policy_id: &str, local_dir: Option<PathBuf>) -> anyhow::Result<()> {
    use viewer_ingest::{AssetSource, CnftToolsSource, extract_cid, parse_cid_info};

    println!("Fetching collection metadata for: {}", policy_id);

    // Fetch from CNFT.tools
    let source = CnftToolsSource::new();
    let assets = source.fetch_collection(policy_id).await?;

    println!("  Found {} assets\n", assets.len());

    // Analyze CIDs from image URLs
    let mut cid_stats = std::collections::HashMap::new();
    let mut assets_with_cids = Vec::new();

    for asset in &assets {
        if let Some(ref image_url) = asset.image_url {
            if let Some(cid) = extract_cid(image_url) {
                if let Some(info) = parse_cid_info(&cid) {
                    let key = (info.version, info.codec.clone());
                    *cid_stats.entry(key).or_insert(0usize) += 1;
                    assets_with_cids.push((
                        asset.encoded_name.clone(),
                        asset.display_name.clone(),
                        cid,
                        info,
                    ));
                }
            }
        }
    }

    println!("CID Analysis:");
    println!("  Assets with valid CIDs: {}", assets_with_cids.len());
    println!("  CID types:");
    for ((version, codec), count) in &cid_stats {
        println!("    CIDv{} ({}): {}", version, codec, count);
    }

    // Show sample CIDs
    println!("\n  Sample CIDs (first 5):");
    for (encoded_name, display_name, cid, info) in assets_with_cids.iter().take(5) {
        println!("    {} ({})", display_name, encoded_name);
        println!("      CID: {}", cid);
        println!(
            "      Version: {}, Codec: {}, Hash: {}",
            info.version, info.codec, info.hash_algo
        );
    }

    // If local directory provided, try to match files
    if let Some(dir) = local_dir {
        println!("\nComparing with local files in: {}", dir.display());

        if !dir.exists() {
            anyhow::bail!("Directory does not exist: {}", dir.display());
        }

        let mut matched = 0;
        let mut unmatched = 0;
        let mut missing = 0;

        for (encoded_name, display_name, target_cid, _info) in &assets_with_cids {
            // Try to find matching local file
            let possible_paths: Vec<PathBuf> = ["png", "jpg", "jpeg", "webp", "gif"]
                .iter()
                .flat_map(|ext| {
                    vec![
                        dir.join(format!("{}.{}", encoded_name, ext)),
                        dir.join(format!("{}.{}", display_name, ext)),
                    ]
                })
                .collect();

            let local_file = possible_paths.iter().find(|p| p.exists());

            match local_file {
                Some(path) => {
                    // Try to compute matching CID
                    let data = std::fs::read(path)?;
                    if let Some((computed, strategy)) =
                        viewer_ingest::find_matching_cid(&data, target_cid)
                    {
                        println!("  ✓ {} - matched using {}", display_name, strategy);
                        println!("    Local: {}", path.display());
                        println!("    CID: {}", computed.cid_v1);
                        matched += 1;
                    } else {
                        // Show what CIDs we computed
                        let raw = viewer_ingest::compute_cid_bytes(&data);
                        let legacy = viewer_ingest::cid_compute::compute_cid_bytes_with_settings(
                            &data,
                            &viewer_ingest::CidSettings::legacy(),
                        );
                        println!("  ✗ {} - no match", display_name);
                        println!("    Local: {}", path.display());
                        println!("    Target:  {}", target_cid);
                        println!("    Raw:     {}", raw.cid_v1);
                        if let Some(v0) = &legacy.cid_v0 {
                            println!("    Legacy:  {} / {}", legacy.cid_v1, v0);
                        }
                        unmatched += 1;
                    }
                }
                None => {
                    missing += 1;
                }
            }
        }

        println!("\nSummary:");
        println!("  Matched: {}", matched);
        println!("  Unmatched: {}", unmatched);
        println!("  Missing local files: {}", missing);
    }

    Ok(())
}

/// Compute CID for a local file.
fn cmd_cid_compute(path: &PathBuf, target: Option<String>) -> anyhow::Result<()> {
    use viewer_ingest::{CidSettings, compute_cid, find_matching_cid};

    if !path.exists() {
        anyhow::bail!("File does not exist: {}", path.display());
    }

    let data = std::fs::read(path)?;
    let file_size = data.len();

    println!("File: {}", path.display());
    println!(
        "Size: {} bytes ({:.2} KB)",
        file_size,
        file_size as f64 / 1024.0
    );

    if let Some(ref target_cid) = target {
        println!("\nTrying to match target CID: {}", target_cid);

        if let Some((computed, strategy)) = find_matching_cid(&data, target_cid) {
            println!("  ✓ Match found using: {}", strategy);
            println!("  CIDv1: {}", computed.cid_v1);
            if let Some(v0) = &computed.cid_v0 {
                println!("  CIDv0: {}", v0);
            }
            println!("  Chunks: {}", computed.chunk_count);
            println!("  Raw codec: {}", computed.is_raw);
        } else {
            println!("  ✗ No matching configuration found");
            println!("\n  Computed CIDs with different settings:");

            // Show all computed variants
            let raw = viewer_ingest::compute_cid_bytes(&data);
            println!("    raw-leaves: {}", raw.cid_v1);

            let legacy = viewer_ingest::cid_compute::compute_cid_bytes_with_settings(
                &data,
                &CidSettings::legacy(),
            );
            println!("    legacy:     {}", legacy.cid_v1);
            if let Some(v0) = &legacy.cid_v0 {
                println!("    legacy v0:  {}", v0);
            }
        }
    } else {
        // Just compute and show CIDs
        let result = compute_cid(path)?;

        println!("\nComputed CIDs (raw-leaves mode):");
        println!("  CIDv1: {}", result.cid_v1);
        if let Some(v0) = &result.cid_v0 {
            println!("  CIDv0: {}", v0);
        }
        println!("  Chunks: {}", result.chunk_count);
        println!("  Raw codec: {}", result.is_raw);

        // Also show legacy mode
        let legacy = viewer_ingest::cid_compute::compute_cid_bytes_with_settings(
            &data,
            &CidSettings::legacy(),
        );
        println!("\nComputed CIDs (legacy mode):");
        println!("  CIDv1: {}", legacy.cid_v1);
        if let Some(v0) = &legacy.cid_v0 {
            println!("  CIDv0: {}", v0);
        }
    }

    Ok(())
}

/// Display information about a CID.
fn cmd_cid_info(cid_str: &str) -> anyhow::Result<()> {
    use viewer_ingest::parse_cid_info;

    match parse_cid_info(cid_str) {
        Some(info) => {
            println!("CID: {}", info.original);
            println!("  Version: {}", info.version);
            println!("  Codec: {} (0x{:x})", info.codec, info.codec_code);
            println!("  Hash algorithm: {}", info.hash_algo);
            println!("  Digest: {}", info.digest_hex);

            // Additional interpretation
            match info.codec.as_str() {
                "raw" => println!("\n  This is a raw block CID (single chunk, no UnixFS wrapper)"),
                "dag-pb" => {
                    println!("\n  This is a dag-pb CID (UnixFS wrapped, may be multi-block)")
                }
                _ => {}
            }

            if info.version == 0 {
                println!("  Note: CIDv0 can be converted to CIDv1 for compatibility");
            }
        }
        None => {
            anyhow::bail!("Invalid CID: {}", cid_str);
        }
    }

    Ok(())
}
