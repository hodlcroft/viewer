use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    tracing_subscriber::fmt::init();

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
        viewer_binary::HcfIndexSize::U32U16,
        false, // single source
    );
    let estimated_token_table = assets.len() * token_entry_size;
    println!("\nEstimated collection.bin:");
    println!(
        "  Token table: {} bytes ({} bytes/token)",
        estimated_token_table, token_entry_size
    );

    Ok(())
}

/// Sync a Cardano collection: fetch, analyze, generate sprites, HCF bundles, and collection.bin.
async fn cmd_sync_cardano(
    policy_id: &str,
    output: &PathBuf,
    config_path: Option<PathBuf>,
    skip_images: bool,
) -> anyhow::Result<()> {
    use viewer_ingest::{
        AssetSource, CnftToolsSource, Pipeline, PipelineConfig, TraitAnalysis, fetch_images,
        fetch_images_iiif,
    };

    println!("Syncing Cardano collection: {}", policy_id);

    // Load config
    let config = load_cardano_config(policy_id, config_path)?;
    let ignore_traits = config.traits.ignore.clone();

    // Fetch metadata from CNFT.tools
    println!("\n[1/5] Fetching collection metadata...");
    let source = CnftToolsSource::new();
    let assets = source.fetch_collection(policy_id).await?;
    println!("  Found {} assets", assets.len());

    // Analyze traits
    println!("\n[2/5] Analyzing traits...");
    let analysis = TraitAnalysis::from_assets(&assets, &ignore_traits)?;
    println!("  {}", analysis.summary());

    // Initialize pipeline
    let pipeline_config = PipelineConfig {
        build_dir: PathBuf::from(".build"),
        ..Default::default()
    };
    let mut pipeline = Pipeline::new(policy_id, assets.len(), pipeline_config)?;
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

    // TODO: Generate sprites
    println!("\n[4/5] Generating sprites...");
    println!("  (Not yet implemented)");

    // TODO: Generate HCF bundles
    println!("\n[5/5] Generating HCF bundles...");
    println!("  (Not yet implemented)");

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
