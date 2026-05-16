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

    /// Filebase operations (experimental — DHT republishing trials)
    Filebase {
        #[command(subcommand)]
        action: FilebaseAction,
    },

    /// CID analysis and computation
    Cid {
        #[command(subcommand)]
        action: CidAction,
    },

    /// Dump collection.bin contents for debugging
    Dump {
        /// Path to collection.bin file or directory containing it
        path: PathBuf,

        /// Show a specific token by name (e.g. "#0511")
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Subcommand)]
enum PinataAction {
    /// Cancel all pending pin requests in the queue
    PurgeQueue,

    /// Show status of pending pin requests
    QueueStatus,

    /// Sync validated raw images to Pinata for a collection
    Sync {
        /// Policy ID of the collection
        policy_id: String,

        /// Path to config file (default: configs/cardano/{policy_id}.toml)
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Show what would be uploaded without uploading
        #[arg(long)]
        dry_run: bool,
    },

    /// Remove files from Pinata that don't match on-chain CIDs
    Clean {
        /// Policy ID of the collection
        policy_id: String,

        /// Path to config file (default: configs/cardano/{policy_id}.toml)
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Show what would be deleted without deleting
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum FilebaseAction {
    /// Pin every on-chain CID for a collection to the Filebase bucket
    /// associated with FILEBASE_API_TOKEN.
    Pin {
        /// Policy ID of the collection
        policy_id: String,

        /// Path to config file (default: configs/cardano/{policy_id}.toml)
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// List CIDs that would be pinned without sending requests
        #[arg(long)]
        dry_run: bool,

        /// Pin at most N CIDs (useful for smoke-testing the experiment)
        #[arg(long)]
        limit: Option<usize>,

        /// Override inter-request delay in milliseconds. Default is 50ms
        /// (20 req/s). Use 0 to disable pacing entirely.
        #[arg(long)]
        delay_ms: Option<u64>,
    },
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

    /// Report assets that share an extracted CID with another asset.
    /// Use to verify whether collisions are real duplicates in the source
    /// or an artefact of our CID extraction.
    Duplicates {
        /// Policy ID of the collection
        policy_id: String,
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
            PinataAction::Sync {
                policy_id,
                config,
                dry_run,
            } => cmd_pinata_sync(&policy_id, config, dry_run).await,
            PinataAction::Clean {
                policy_id,
                config,
                dry_run,
            } => cmd_pinata_clean(&policy_id, config, dry_run).await,
        },
        Commands::Filebase { action } => match action {
            FilebaseAction::Pin {
                policy_id,
                config,
                dry_run,
                limit,
                delay_ms,
            } => cmd_filebase_pin(&policy_id, config, dry_run, limit, delay_ms).await,
        },
        Commands::Cid { action } => match action {
            CidAction::Check {
                policy_id,
                local_dir,
            } => cmd_cid_check(&policy_id, local_dir).await,
            CidAction::Compute { path, target } => cmd_cid_compute(&path, target),
            CidAction::Info { cid } => cmd_cid_info(&cid),
            CidAction::Duplicates { policy_id } => cmd_cid_duplicates(&policy_id).await,
        },
        Commands::Dump { path, token } => cmd_dump(&path, token.as_deref()),
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
                .with_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap())),
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
    use viewer_binary::{HcfMetadata, ImageFormat, SourcesSection, SpriteIndexBuilder};
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
        use viewer_binary::{HcfMetadata, ImageFormat, SourcesSection};

        let sources = SourcesSection::new(vec![]); // placeholder, intern_source_strings below

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

        // Intern source chain/policy_id into string table
        writer.intern_source_strings(&[("cardano", policy_id, assets.len() as u32)])?;

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
                .flat_map(|(name, values)| analysis.encode_trait(name, values))
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
        // Placeholder sources section (intern_source_strings called below)
        let sources = SourcesSection::new(vec![]);

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

        // Intern source chain/policy_id into string table
        writer.intern_source_strings(&[("cardano", policy_id, assets.len() as u32)])?;

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
                .flat_map(|(name, values)| analysis.encode_trait(name, values))
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

/// Upload files to Pinata with progress reporting.

/// Sync validated raw images to Pinata for a collection.
///
/// This command:
/// 1. Loads config from configs/cardano/{policy_id}.toml
/// 2. Fetches collection metadata from CNFT.tools
/// 3. Validates local raw files against on-chain CIDs
/// 4. Uploads only valid files to Pinata
/// Sync validated raw images to Pinata for a collection.
///
/// This command uses a streaming approach:
/// 1. Loads config and fetches collection metadata
/// 2. Fetches list of CIDs already in the Pinata group
/// 3. For each asset: skip if already in Pinata, validate CID, upload if valid
/// Sync validated raw images to Pinata for a collection.
///
/// This command efficiently determines which files need uploading:
/// 1. Fetches on-chain CIDs from CNFT.tools
/// 2. Fetches CIDs already in Pinata group
/// 3. Computes the difference (needed = on_chain - pinata)
/// 4. Only validates and uploads the needed files
async fn cmd_pinata_sync(
    policy_id: &str,
    config_path: Option<PathBuf>,
    dry_run: bool,
) -> anyhow::Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::collections::HashMap;
    use viewer_ingest::{
        AssetSource, CnftToolsSource, PinataClient, extract_cid, find_matching_cid, to_cidv1,
    };

    // Initialize tracing with RUST_LOG support
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    // Load config
    let config = load_cardano_config(policy_id, config_path.clone())?;

    // Check Pinata is configured
    if !config.pinata.is_enabled() {
        anyhow::bail!("Pinata is not enabled in config. Add [pinata] section with enabled = true");
    }
    let group_id = config
        .pinata
        .group_id
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Pinata group_id not configured"))?;

    // Determine raw directory
    let raw_dir = PathBuf::from(format!(".build/{}/raw", policy_id));
    if !raw_dir.exists() {
        anyhow::bail!(
            "Raw directory not found: {}\nRun 'viewer sync cardano {}' first to fetch images.",
            raw_dir.display(),
            policy_id
        );
    }

    println!("Pinata sync for: {}", policy_id);
    println!(
        "  Config: {}",
        config_path
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("configs/cardano/{}.toml", policy_id))
    );
    println!("  Raw dir: {}", raw_dir.display());
    println!("  Group ID: {}", group_id);
    println!();

    // Connect to Pinata and verify group exists
    println!("Connecting to Pinata...");
    let pinata = PinataClient::from_env()?;

    let group = pinata.find_group_by_id(group_id).await?;
    if group.is_none() {
        anyhow::bail!("Pinata group not found: {}", group_id);
    }

    // Fetch collection metadata and build CID -> asset map
    println!("Fetching collection metadata...");
    let source = CnftToolsSource::new();
    let assets = source.fetch_collection(policy_id).await?;
    println!("  Found {} assets", assets.len());

    // Build map: CIDv1 -> (encoded_name, display_name, original_cid)
    let mut cid_to_asset: HashMap<String, (String, String, String)> = HashMap::new();
    let mut no_image_url = 0;
    let mut no_cid_extracted = 0;
    let mut no_cid_v1 = 0;
    for (i, asset) in assets.iter().enumerate() {
        if let Some(ref image_url) = asset.image_url {
            if let Some(cid) = extract_cid(image_url) {
                if let Some(cid_v1) = to_cidv1(&cid) {
                    cid_to_asset.insert(
                        cid_v1,
                        (asset.encoded_name.clone(), asset.display_name.clone(), cid),
                    );
                } else {
                    no_cid_v1 += 1;
                    if i < 3 {
                        tracing::warn!("Failed to convert to CIDv1: {}", cid);
                    }
                }
            } else {
                no_cid_extracted += 1;
                if i < 3 {
                    tracing::warn!("Failed to extract CID from: {}", image_url);
                }
            }
        } else {
            no_image_url += 1;
        }
    }
    tracing::info!(count = cid_to_asset.len(), "Assets with CIDs");
    if no_image_url > 0 || no_cid_extracted > 0 || no_cid_v1 > 0 {
        tracing::warn!(
            no_image_url,
            no_cid_extracted,
            no_cid_v1,
            "Some assets missing CID data"
        );
    }

    // Log a sample of on-chain CIDs for debugging
    for (i, (cid_v1, (_encoded, display_name, original))) in cid_to_asset.iter().enumerate() {
        if i >= 3 {
            break;
        }
        tracing::info!(
            name = %display_name,
            %cid_v1,
            %original,
            "On-chain sample"
        );
    }

    // Stream through Pinata pages, removing matched CIDs as we go
    println!("Checking existing files in Pinata group...");
    let mut already_in_pinata = 0usize;

    // Grab a few on-chain CIDs for comparison logging
    let sample_onchain_cids: Vec<_> = cid_to_asset.keys().take(3).cloned().collect();
    let mut logged_samples = 0usize;

    pinata
        .list_files_in_group_paged(group_id, 100, |files, total_fetched| {
            for file in files {
                if let Some(cid_v1) = to_cidv1(&file.cid) {
                    // Log first 5 comparisons for debugging
                    if logged_samples < 5 {
                        tracing::debug!(
                            pinata_v1 = %cid_v1,
                            "Pinata CID"
                        );
                        logged_samples += 1;
                    }

                    if cid_to_asset.remove(&cid_v1).is_some() {
                        already_in_pinata += 1;
                    }
                } else {
                    tracing::warn!("Failed to convert Pinata CID to v1: {}", file.cid);
                }
            }

            // Log on-chain samples on first page
            if total_fetched <= 100 {
                for cid in &sample_onchain_cids {
                    tracing::debug!(onchain_v1 = %cid, "On-chain CID");
                }
            }

            tracing::info!(
                processed = total_fetched,
                matched = already_in_pinata,
                remaining = cid_to_asset.len(),
                "Pinata page processed"
            );
            true // continue pagination
        })
        .await?;

    println!("  Already in Pinata: {}", already_in_pinata);
    println!("  Need to upload: {}", cid_to_asset.len());

    if cid_to_asset.is_empty() {
        println!();
        println!("All files already in Pinata. Nothing to do.");
        return Ok(());
    }

    println!();

    // Process only the files that need uploading
    let to_upload: Vec<_> = cid_to_asset.into_iter().collect();
    let pb = ProgressBar::new(to_upload.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut uploaded = 0usize;
    let mut invalid_cids: Vec<(String, String, String)> = Vec::new(); // (name, expected, got)
    let mut missing_files: Vec<String> = Vec::new();
    let mut failed_uploads: Vec<String> = Vec::new();

    for (_target_cid, (encoded_name, display_name, original_cid)) in &to_upload {
        pb.inc(1);
        pb.set_message(format!("{}", display_name));

        // Try to find local file with various extensions
        let possible_paths: Vec<PathBuf> = ["png", "jpg", "jpeg", "webp", "gif"]
            .iter()
            .map(|ext| raw_dir.join(format!("{}.{}", encoded_name, ext)))
            .collect();

        let local_file = possible_paths.iter().find(|p| p.exists());

        match local_file {
            Some(path) => {
                // Validate CID
                let data = std::fs::read(path)?;
                if find_matching_cid(&data, original_cid).is_none() {
                    let computed = viewer_ingest::compute_cid_bytes(&data);
                    invalid_cids.push((
                        display_name.clone(),
                        original_cid.clone(),
                        computed.cid_v1,
                    ));
                    continue;
                }

                // Upload if not dry run
                if !dry_run {
                    match pinata
                        .upload_file(path, Some(display_name), Some(group_id))
                        .await
                    {
                        Ok(_result) => {
                            uploaded += 1;
                        }
                        Err(e) => {
                            failed_uploads.push(format!("{}: {}", display_name, e));
                        }
                    }
                } else {
                    uploaded += 1; // Count as "would upload" for dry run
                }
            }
            None => {
                missing_files.push(display_name.clone());
            }
        }

        // Update progress message
        pb.set_message(format!(
            "upload: {} invalid: {} missing: {}",
            uploaded,
            invalid_cids.len(),
            missing_files.len()
        ));
    }

    pb.finish_and_clear();

    // Summary
    println!();
    if dry_run {
        println!("DRY RUN complete:");
        println!("  Already in Pinata: {}", already_in_pinata);
        println!("  Would upload: {}", uploaded);
    } else {
        println!("Sync complete:");
        println!("  Already in Pinata: {}", already_in_pinata);
        println!("  Uploaded: {}", uploaded);
    }
    if !invalid_cids.is_empty() {
        println!("  Invalid CIDs: {}", invalid_cids.len());
    }
    if !missing_files.is_empty() {
        println!("  Missing files: {}", missing_files.len());
    }
    if !failed_uploads.is_empty() {
        println!("  Failed uploads: {}", failed_uploads.len());
    }

    // Report invalid CIDs
    if !invalid_cids.is_empty() {
        println!();
        println!("Invalid CIDs (file content doesn't match on-chain CID):");
        for (name, expected, got) in invalid_cids.iter().take(10) {
            println!("  {} expected: {} got: {}", name, expected, got);
        }
        if invalid_cids.len() > 10 {
            println!("  ... and {} more", invalid_cids.len() - 10);
        }
    }

    // Report missing files
    if !missing_files.is_empty() {
        println!();
        println!("Missing files (not found in raw directory):");
        for name in missing_files.iter().take(10) {
            println!("  {}", name);
        }
        if missing_files.len() > 10 {
            println!("  ... and {} more", missing_files.len() - 10);
        }
    }

    // Report failed uploads
    if !failed_uploads.is_empty() {
        println!();
        println!("Failed uploads:");
        for err in failed_uploads.iter().take(10) {
            println!("  {}", err);
        }
        if failed_uploads.len() > 10 {
            println!("  ... and {} more", failed_uploads.len() - 10);
        }
    }

    Ok(())
}

/// Clean up Pinata group by removing files that don't match on-chain CIDs.
///
/// This command:
/// 1. Fetches on-chain CIDs from CNFT.tools
/// 2. Lists all files in the Pinata group
/// 3. Deletes any files whose CID doesn't match an on-chain CID
async fn cmd_pinata_clean(
    policy_id: &str,
    config_path: Option<PathBuf>,
    dry_run: bool,
) -> anyhow::Result<()> {
    use std::collections::HashSet;
    use viewer_ingest::{AssetSource, CnftToolsSource, PinataClient, extract_cid, to_cidv1};

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .try_init()
        .ok();

    // Load config
    let config = load_cardano_config(policy_id, config_path.clone())?;

    // Check Pinata is configured
    if !config.pinata.is_enabled() {
        anyhow::bail!("Pinata is not enabled in config. Add [pinata] section with enabled = true");
    }
    let group_id = config
        .pinata
        .group_id
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Pinata group_id not configured"))?;

    println!("Pinata clean for: {}", policy_id);
    println!(
        "  Config: {}",
        config_path
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("configs/cardano/{}.toml", policy_id))
    );
    println!("  Group ID: {}", group_id);
    if dry_run {
        println!("  Mode: DRY RUN (no files will be deleted)");
    }
    println!();

    // Connect to Pinata
    println!("Connecting to Pinata...");
    let pinata = PinataClient::from_env()?;

    let group = pinata.find_group_by_id(group_id).await?;
    if group.is_none() {
        anyhow::bail!("Pinata group not found: {}", group_id);
    }

    // Fetch collection metadata and build set of valid CIDs
    println!("Fetching collection metadata...");
    let source = CnftToolsSource::new();
    let assets = source.fetch_collection(policy_id).await?;
    println!("  Found {} assets", assets.len());

    // Build set of valid CIDv1s from on-chain data
    let mut valid_cids: HashSet<String> = HashSet::new();
    for asset in &assets {
        if let Some(ref image_url) = asset.image_url {
            if let Some(cid) = extract_cid(image_url) {
                if let Some(cid_v1) = to_cidv1(&cid) {
                    valid_cids.insert(cid_v1);
                }
            }
        }
    }
    println!("  Valid CIDs: {}", valid_cids.len());

    // List all files in Pinata group and find orphans
    println!("\nScanning Pinata group for orphaned files...");
    let mut orphan_files: Vec<(String, String, Option<String>)> = Vec::new(); // (id, cid, name)
    let mut total_files = 0usize;

    pinata
        .list_files_in_group_paged(group_id, 100, |files, _total_fetched| {
            for file in files {
                total_files += 1;
                if let Some(cid_v1) = to_cidv1(&file.cid) {
                    if !valid_cids.contains(&cid_v1) {
                        orphan_files.push((file.id.clone(), file.cid.clone(), file.name.clone()));
                    }
                } else {
                    // Can't convert to CIDv1, treat as orphan
                    orphan_files.push((file.id.clone(), file.cid.clone(), file.name.clone()));
                }
            }
            true // continue pagination
        })
        .await?;

    println!("  Total files in group: {}", total_files);
    println!("  Orphaned files (not on-chain): {}", orphan_files.len());

    if orphan_files.is_empty() {
        println!("\nNo orphaned files found. Group is clean.");
        return Ok(());
    }

    // Show orphans
    println!("\nOrphaned files:");
    for (id, cid, name) in orphan_files.iter().take(20) {
        let display_name = name.as_deref().unwrap_or("(unnamed)");
        println!("  {} - {} [{}]", display_name, cid, id);
    }
    if orphan_files.len() > 20 {
        println!("  ... and {} more", orphan_files.len() - 20);
    }

    if dry_run {
        println!("\nDRY RUN: Would delete {} files", orphan_files.len());
        return Ok(());
    }

    // Delete orphaned files
    use indicatif::{ProgressBar, ProgressStyle};

    let pb = ProgressBar::new(orphan_files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut deleted = 0usize;
    let mut failed = 0usize;

    for (id, _cid, name) in &orphan_files {
        let display_name = name.as_deref().unwrap_or("(unnamed)");
        pb.set_message(display_name.to_string());

        match pinata.delete_file(id).await {
            Ok(()) => {
                deleted += 1;
            }
            Err(e) => {
                failed += 1;
                pb.suspend(|| {
                    println!("  Failed to delete {}: {}", display_name, e);
                });
            }
        }

        pb.inc(1);
    }

    pb.finish_and_clear();

    println!("\nClean complete:");
    println!("  Deleted: {}", deleted);
    if failed > 0 {
        println!("  Failed: {}", failed);
    }

    Ok(())
}

/// Pin every on-chain CID for a collection to the Filebase bucket associated
/// with FILEBASE_API_TOKEN. Experimental — used to evaluate DHT republishing
/// relative to Pinata.
async fn cmd_filebase_pin(
    policy_id: &str,
    config_path: Option<PathBuf>,
    dry_run: bool,
    limit: Option<usize>,
    delay_ms: Option<u64>,
) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    use std::time::Duration;
    use viewer_ingest::{AssetSource, CnftToolsSource, FilebaseClient, PinItem, extract_cid};

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let config = load_cardano_config(policy_id, config_path.clone())?;
    let slug = config.slug.clone();

    println!("Filebase pin for: {}", policy_id);

    println!("Fetching collection metadata...");
    let source = CnftToolsSource::new();
    let assets = source.fetch_collection(policy_id).await?;
    println!("  Found {} assets", assets.len());

    // Build a deterministic CID -> display_name map. Deduplicates the same CID
    // shared across multiple tokens (rare but possible).
    let mut cids: BTreeMap<String, String> = BTreeMap::new();
    let mut no_image_url = 0usize;
    let mut no_cid = 0usize;
    for asset in &assets {
        let Some(image_url) = asset.image_url.as_ref() else {
            no_image_url += 1;
            continue;
        };
        let Some(cid) = extract_cid(image_url) else {
            no_cid += 1;
            continue;
        };
        cids.entry(cid)
            .or_insert_with(|| asset.display_name.clone());
    }

    println!("  CIDs to pin: {}", cids.len());
    if no_image_url > 0 || no_cid > 0 {
        println!(
            "  Skipped: {} without image_url, {} without extractable CID",
            no_image_url, no_cid
        );
    }

    let mut items: Vec<PinItem> = cids
        .into_iter()
        .map(|(cid, display_name)| {
            let mut meta = BTreeMap::new();
            if let Some(ref s) = slug {
                meta.insert("slug".to_string(), s.clone());
            }
            meta.insert("policy_id".to_string(), policy_id.to_string());
            meta.insert("asset_name".to_string(), display_name.clone());
            PinItem {
                name: format!("{}/{}", policy_id, display_name),
                cid,
                meta,
            }
        })
        .collect();

    if let Some(limit) = limit {
        items.truncate(limit);
        println!("  Limited to first {}", items.len());
    }

    if items.is_empty() {
        println!("Nothing to pin.");
        return Ok(());
    }

    if dry_run {
        println!("\nDRY RUN — would pin:");
        for item in items.iter().take(10) {
            println!("  {} → {}", item.name, item.cid);
        }
        if items.len() > 10 {
            println!("  ... and {} more", items.len() - 10);
        }
        return Ok(());
    }

    println!("\nConnecting to Filebase...");
    let mut client = FilebaseClient::from_env()?;
    if let Some(ms) = delay_ms {
        println!("  Inter-request delay: {}ms", ms);
        client = client.with_delay(Duration::from_millis(ms));
    }

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(items.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let results = client
        .pin_cids(
            &items,
            Some(&|done, _total, result| {
                pb.set_position(done as u64);
                let label = match &result.outcome {
                    Ok(r) => format!("{} [{}]", result.name, r.status),
                    Err(e) => format!("{} (failed: {})", result.name, e),
                };
                pb.set_message(label);
            }),
        )
        .await;

    pb.finish_and_clear();

    let succeeded = results.iter().filter(|r| r.outcome.is_ok()).count();
    let failed = results.len() - succeeded;

    println!("\nFilebase pin complete:");
    println!("  Pinned: {}", succeeded);
    if failed > 0 {
        println!("  Failed: {}", failed);
        for r in results.iter().filter(|r| r.outcome.is_err()).take(10) {
            if let Err(e) = &r.outcome {
                println!("    {} ({}): {}", r.name, r.cid, e);
            }
        }
    }

    Ok(())
}

fn cmd_dump(path: &PathBuf, token_filter: Option<&str>) -> anyhow::Result<()> {
    use viewer_binary::{HEADER_SIZE, Header, TOKEN_FIXED_SIZE, TraitSchema};

    let bin_path = if path.is_file() {
        path.clone()
    } else {
        path.join("collection.bin")
    };

    let data = std::fs::read(&bin_path)?;
    anyhow::ensure!(data.len() >= HEADER_SIZE, "File too small for header");

    let header_bytes: [u8; HEADER_SIZE] = data[..HEADER_SIZE].try_into().unwrap();
    let header = Header::from_bytes(&header_bytes);

    let bitmap_size = header
        .bitmap_size()
        .ok_or_else(|| anyhow::anyhow!("Invalid bitmap size"))?;
    let bitmap_bytes = bitmap_size.byte_size();
    let token_entry_size = TOKEN_FIXED_SIZE + bitmap_bytes;

    // Parse string table
    let st_start = header.string_table_offset as usize + 4; // skip length prefix
    let st_end = header.trait_schema_offset as usize;
    let string_table = &data[st_start..st_end];

    let read_str = |offset: u16| -> &str {
        let start = offset as usize;
        if start >= string_table.len() {
            return "<invalid>";
        }
        let end = string_table[start..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| start + p)
            .unwrap_or(string_table.len());
        std::str::from_utf8(&string_table[start..end]).unwrap_or("<invalid utf8>")
    };

    // Parse trait schema
    let trait_schema_data =
        &data[header.trait_schema_offset as usize..header.trait_index_offset as usize];
    let schema = TraitSchema::from_bytes(trait_schema_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse trait schema"))?;

    println!("=== Collection Dump: {} ===", bin_path.display());
    println!(
        "Tokens: {}  Traits: {}  Bitmap: {} bytes  Total values: {}",
        header.token_count, header.trait_count, bitmap_bytes, schema.total_values
    );
    println!(
        "Flags: 0x{:04x} (hide_rarity={}, multi_source={})",
        header.flags,
        header.hide_rarity(),
        header.is_multi_source()
    );
    println!();

    // Print trait schema
    println!("=== Trait Schema ===");
    for trait_def in &schema.traits {
        let name = read_str(trait_def.name.0 as u16);
        println!(
            "  {} ({} values, bits {}..{})",
            name,
            trait_def.values.len(),
            trait_def.bitmap_offset,
            trait_def.bitmap_offset as usize + trait_def.values.len() - 1
        );
        for (i, val) in trait_def.values.iter().enumerate() {
            let val_name = read_str(val.name.0 as u16);
            println!(
                "    bit {:3}: {:30} (count: {})",
                trait_def.bitmap_offset + i as u16,
                val_name,
                val.count
            );
        }
    }
    println!();

    // Parse asset IDs
    let asset_id_index_start = header.asset_id_index_offset as usize;

    // Dump tokens
    let token_table_start = header.token_table_offset as usize;
    let mut found_token = false;

    for i in 0..header.token_count as usize {
        let offset = token_table_start + i * token_entry_size;
        let entry = &data[offset..offset + token_entry_size];

        let rarity_rank = u16::from_le_bytes([entry[0], entry[1]]);
        let _rarity_score = u16::from_le_bytes([entry[2], entry[3]]);
        let name_ref = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
        let bitmap = &entry[TOKEN_FIXED_SIZE..TOKEN_FIXED_SIZE + bitmap_bytes];

        let name = if name_ref & viewer_binary::NAME_REF_CUSTOM_FLAG != 0 {
            let str_offset = (name_ref & viewer_binary::NAME_REF_OFFSET_MASK) as u16;
            read_str(str_offset).to_string()
        } else {
            format!("#{name_ref:04}")
        };

        // Read asset ID
        let offset_entry = asset_id_index_start + i * 4;
        let asset_id_str_offset = u32::from_le_bytes([
            data[offset_entry],
            data[offset_entry + 1],
            data[offset_entry + 2],
            data[offset_entry + 3],
        ]) as usize;
        let asset_id_start = asset_id_index_start + asset_id_str_offset;
        let asset_id_end = data[asset_id_start..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| asset_id_start + p)
            .unwrap_or(data.len());
        let asset_id =
            std::str::from_utf8(&data[asset_id_start..asset_id_end]).unwrap_or("<invalid>");

        // If filtering by token name, only show matching
        if let Some(filter) = token_filter {
            if name != filter && asset_id != filter {
                continue;
            }
        }
        found_token = true;

        println!("--- Token {i}: {name} (asset_id: {asset_id}) ---");
        println!("  rarity_rank: {rarity_rank}");
        let bitmap_hex: String = bitmap.iter().map(|b| format!("{b:02x}")).collect();
        println!("  bitmap: {bitmap_hex}");
        println!("  traits:");

        for trait_def in &schema.traits {
            let trait_name = read_str(trait_def.name.0 as u16);
            let mut matched = Vec::new();
            for (j, val) in trait_def.values.iter().enumerate() {
                let bit = trait_def.bitmap_offset as usize + j;
                let byte_idx = bit / 8;
                let bit_idx = bit % 8;
                if byte_idx < bitmap.len() && bitmap[byte_idx] & (1 << bit_idx) != 0 {
                    let val_name = read_str(val.name.0 as u16);
                    matched.push(format!("{val_name} (count: {}, bit: {bit})", val.count));
                }
            }
            if !matched.is_empty() {
                for m in &matched {
                    println!("    {trait_name}: {m}");
                }
            }
        }
        println!();

        // If not filtering, only show first 5
        if token_filter.is_none() && i >= 4 {
            println!(
                "  ... ({} more tokens, use --token to show a specific one)",
                header.token_count as usize - 5
            );
            break;
        }
    }

    if token_filter.is_some() && !found_token {
        println!("Token '{}' not found", token_filter.unwrap());
    }

    Ok(())
}

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
        use indicatif::{ProgressBar, ProgressStyle};

        if !dir.exists() {
            anyhow::bail!("Directory does not exist: {}", dir.display());
        }

        let total = assets_with_cids.len() as u64;
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
                )
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message("Validating CIDs...");

        let mut matched = 0;
        let mut unmatched = 0;
        let mut missing = 0;
        let mut invalid_assets: Vec<(String, String, String, String)> = Vec::new();

        for (encoded_name, display_name, target_cid, _info) in &assets_with_cids {
            pb.inc(1);

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
                    if viewer_ingest::find_matching_cid(&data, target_cid).is_some() {
                        matched += 1;
                    } else {
                        // Record mismatch for reporting after progress bar completes
                        let computed = viewer_ingest::compute_cid_bytes(&data);
                        invalid_assets.push((
                            encoded_name.clone(),
                            target_cid.clone(),
                            computed.cid_v1,
                            path.display().to_string(),
                        ));
                        unmatched += 1;
                    }
                }
                None => {
                    missing += 1;
                }
            }
        }

        pb.finish_and_clear();

        // Report invalid CIDs after progress bar
        if !invalid_assets.is_empty() {
            println!("\nInvalid CIDs detected:");
            for (asset_id, expected, computed, path) in &invalid_assets {
                println!("  ✗ {}", asset_id);
                println!("    File:     {}", path);
                println!("    Expected: {}", expected);
                println!("    Got:      {}", computed);
            }
        }

        // Summary
        println!("\nValidation complete:");
        println!("  ✓ Valid:   {}", matched);
        if unmatched > 0 {
            println!("  ✗ Invalid: {}", unmatched);
        }
        if missing > 0 {
            println!("  ? Missing: {}", missing);
        }

        if unmatched > 0 {
            anyhow::bail!("{} assets have invalid CIDs", unmatched);
        }
    }

    Ok(())
}

/// Report duplicate-CID groups for a collection.
///
/// Groups assets by their extracted CID. For any CID claimed by more than
/// one asset, prints the asset names alongside their raw image URLs — so we
/// can tell at a glance whether the source data is genuinely sharing CIDs
/// or whether `extract_cid` is mis-parsing different URLs to the same value.
async fn cmd_cid_duplicates(policy_id: &str) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    use viewer_ingest::{AssetSource, CnftToolsSource, extract_cid};

    println!("Fetching collection metadata for: {}", policy_id);
    let source = CnftToolsSource::new();
    let assets = source.fetch_collection(policy_id).await?;
    println!("  Found {} assets\n", assets.len());

    // Group by extracted CID, retaining all (display_name, image_url) tuples.
    let mut by_cid: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut no_image_url = 0usize;
    let mut no_cid = 0usize;

    for asset in &assets {
        let Some(image_url) = asset.image_url.as_ref() else {
            no_image_url += 1;
            continue;
        };
        let Some(cid) = extract_cid(image_url) else {
            no_cid += 1;
            continue;
        };
        by_cid
            .entry(cid)
            .or_default()
            .push((asset.display_name.clone(), image_url.clone()));
    }

    let duplicates: Vec<_> = by_cid.iter().filter(|(_, v)| v.len() > 1).collect();

    println!("Summary:");
    println!("  Assets with extractable CID: {}", assets.len() - no_image_url - no_cid);
    println!("  Unique CIDs:                 {}", by_cid.len());
    println!("  CIDs shared by >1 asset:     {}", duplicates.len());
    if no_image_url > 0 || no_cid > 0 {
        println!(
            "  Skipped:                     {} no image_url, {} unparseable",
            no_image_url, no_cid
        );
    }

    if duplicates.is_empty() {
        println!("\nNo duplicate CIDs.");
        return Ok(());
    }

    println!("\nDuplicate groups:");
    for (cid, assets) in &duplicates {
        println!("  {} ({} assets)", cid, assets.len());

        // Identify whether the raw image_urls are byte-identical or differ.
        let first_url = &assets[0].1;
        let all_same = assets.iter().all(|(_, u)| u == first_url);
        let url_marker = if all_same { "same url" } else { "differing urls" };
        println!("    [{}]", url_marker);

        for (name, url) in assets.iter() {
            println!("    - {} -> {}", name, url);
        }
    }

    Ok(())
}

/// Compute CID for a local file.
fn cmd_cid_compute(path: &PathBuf, target: Option<String>) -> anyhow::Result<()> {
    use viewer_ingest::{compute_cid, find_matching_cid};

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

        if let Some(computed) = find_matching_cid(&data, target_cid) {
            println!("  ✓ Match found!");
            println!("  CIDv1: {}", computed.cid_v1);
            if let Some(v0) = &computed.cid_v0 {
                println!("  CIDv0: {}", v0);
            }
        } else {
            println!("  ✗ No match");

            let computed = viewer_ingest::compute_cid_bytes(&data);
            println!("\n  Computed CID:");
            println!("    CIDv1: {}", computed.cid_v1);
            if let Some(v0) = &computed.cid_v0 {
                println!("    CIDv0: {}", v0);
            }
        }
    } else {
        // Just compute and show CIDs
        let result = compute_cid(path)?;

        println!("\nComputed CID:");
        println!("  CIDv1: {}", result.cid_v1);
        if let Some(v0) = &result.cid_v0 {
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
