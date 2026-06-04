use clap::{Parser, Subcommand, ValueEnum};
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

    /// Asset source operations and diagnostics
    Source {
        #[command(subcommand)]
        action: SourceAction,
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

/// CID source for `filebase pin`.
#[derive(ValueEnum, Clone, Copy, Debug)]
enum PinSource {
    /// `ownership.cnft.dev/api/cids/{policy}` — pre-resolved.
    CidIndex,
    /// Live extraction via Maestro + local `cardano-assets`.
    Maestro,
}

#[derive(Subcommand)]
enum SourceAction {
    /// Fetch one page of assets from Maestro for a policy and report how
    /// they parse against cardano_assets::AssetMetadata. Used to validate
    /// the Maestro fallback path before doing a full ingestion.
    ProbeMaestro {
        /// Policy ID of the collection
        policy_id: String,
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

        /// Where to source the CID set. Default `cid-index` uses
        /// `ownership.cnft.dev`; `maestro` extracts live via Maestro +
        /// the local `cardano-assets` patch (use when a policy isn't yet
        /// indexed by the collection-ownership worker).
        #[arg(long, value_enum, default_value_t = PinSource::CidIndex)]
        source: PinSource,
    },

    /// Report pin status counts for a collection on Filebase.
    Status {
        /// Policy ID of the collection
        policy_id: String,
    },

    /// Group every `failed` pin in the bucket by the policy_id parsed
    /// from each pin's name prefix. Reports per-policy counts (sorted
    /// descending) so the rescue can be driven per policy.
    FailedByPolicy,

    /// List a collection's content that has no successful pin — the set
    /// needing rescue. Collapses CIDv0/v1 of the same block by multihash.
    Unpinned {
        /// Policy ID of the collection
        policy_id: String,

        /// Resolve each unpinned CID back to its asset via Maestro and
        /// include the display name in the listing.
        #[arg(long)]
        with_names: bool,
    },

    /// Audit root-level bucket objects. Lists every object directly at
    /// the bucket root (no `/` in key) and classifies each as either
    /// "covered" (a copy exists under some policy folder) or "orphan"
    /// (no folder copy — would lose content if deleted).
    RootAudit {
        /// Print first N entries from each bucket
        #[arg(long, default_value_t = 20)]
        sample: usize,
    },

    /// Delete root-level CID-keyed bucket objects whose content has a
    /// folder copy elsewhere ("covered"). Previews by default — pass
    /// --execute to actually delete. Orphan root entries (no folder copy)
    /// are never touched here.
    RootCleanup {
        /// Actually delete; without this, the command only previews.
        #[arg(long)]
        execute: bool,

        /// Delete at most N entries (applies only with --execute)
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Reclassify objects in `uncategorized/` by building a forward CID
    /// index from every known policy folder (via Maestro), inverting it,
    /// and moving each matched object into `{policy_id}/{cid_v1}`.
    /// Unmatched entries stay where they are. Previews by default.
    ClassifyUncategorized {
        /// Actually perform the moves; without this, only previews.
        #[arg(long)]
        execute: bool,

        /// Source folder to reclassify
        #[arg(long, default_value = "uncategorized")]
        source_folder: String,

        /// Move at most N entries (applies only with --execute)
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Relocate orphan root objects (no folder copy) into an
    /// `uncategorized/` folder via S3 COPY, then delete the root entry.
    /// Each delete is guarded by a HEAD on the copied key — root is only
    /// dropped after the destination is confirmed present.
    RootRelocate {
        /// Actually perform the relocation; without this, the command
        /// only previews.
        #[arg(long)]
        execute: bool,

        /// Relocate at most N entries (applies only with --execute)
        #[arg(long)]
        limit: Option<usize>,

        /// Destination folder (default `uncategorized`)
        #[arg(long, default_value = "uncategorized")]
        folder: String,
    },

    /// HEAD a bucket object — prints its size, etag, content-type, and
    /// any `x-amz-meta-*` metadata Filebase attached (incl. `cid`).
    S3Head {
        /// Bucket key
        key: String,
    },

    /// Delete every object under a given key prefix. Previews by
    /// default; --execute actually deletes. Use with care — content
    /// becomes unpinned if no other reference exists.
    DeletePrefix {
        /// Key prefix, e.g. "uncategorized/" or "audit-probe/"
        prefix: String,

        #[arg(long)]
        execute: bool,

        /// Delete at most N entries (applies only with --execute)
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Verify that deleting a root-level CID-keyed object does NOT unpin
    /// content that's also referenced by a path-keyed copy. Uploads a
    /// random test blob to a path key, confirms both entries share the
    /// same pin, deletes the root, then re-HEADs the path entry to check
    /// it survives intact.
    DeleteSafetyTest,

    /// List bucket objects with the given S3 key prefix (debug helper).
    S3List {
        /// Key prefix, e.g. "a4996cce.../" or "" for everything at root
        prefix: String,

        /// Limit how many entries to print
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// Remove orphaned root-level CIDv0 bucket objects from an earlier
    /// rescue, only when the matching v1 entry under `{policy}/...` is
    /// populated. Previews by default — pass --execute to actually delete.
    RescueCleanup {
        /// Policy ID of the collection
        policy_id: String,

        /// Actually delete; without this, the command only previews.
        #[arg(long)]
        execute: bool,
    },

    /// Recover a collection's failed-pin content from nftcdn. Probes by
    /// default — fetches each failed asset's image and checks the bytes
    /// reproduce its on-chain CID. With --execute, also re-hosts the
    /// verified content to Filebase via the IPFS RPC API.
    Rescue {
        /// Policy ID of the collection
        policy_id: String,

        /// Process at most N failed CIDs
        #[arg(long)]
        limit: Option<usize>,

        /// Re-host verified content to Filebase; without this, probe only.
        #[arg(long)]
        execute: bool,
    },

    /// Delete `failed` Pinning Service pin records for a collection
    /// when the CID has a folder copy in the bucket (i.e. content is
    /// preserved elsewhere). Cleans stale records left when Pinning
    /// Service didn't reconcile after the rescue. Previews by default.
    PruneFailed {
        /// Policy ID of the collection
        policy_id: String,

        /// Actually delete; without this, only previews.
        #[arg(long)]
        execute: bool,

        /// Delete at most N pin records (applies only with --execute)
        #[arg(long)]
        limit: Option<usize>,

        /// Inter-request delay (ms) for the delete pass. Default 50ms.
        #[arg(long)]
        delay_ms: Option<u64>,
    },

    /// Remove legacy CIDv0 pins for a collection, but only where a
    /// CIDv1 pin of the same content is already `pinned`. Previews by
    /// default — pass --execute to actually delete.
    PruneV0 {
        /// Policy ID of the collection
        policy_id: String,

        /// Actually delete; without this the command only previews.
        #[arg(long)]
        execute: bool,

        /// Delete at most N pins (applies only with --execute)
        #[arg(long)]
        limit: Option<usize>,

        /// Inter-request delay in milliseconds for the delete pass.
        /// Default is 50ms; use 0 to disable pacing.
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
        Commands::Source { action } => match action {
            SourceAction::ProbeMaestro { policy_id } => cmd_source_probe_maestro(&policy_id).await,
        },
        Commands::Filebase { action } => match action {
            FilebaseAction::Pin {
                policy_id,
                config,
                dry_run,
                limit,
                delay_ms,
                source,
            } => cmd_filebase_pin(&policy_id, config, dry_run, limit, delay_ms, source).await,
            FilebaseAction::Status { policy_id } => cmd_filebase_status(&policy_id).await,
            FilebaseAction::FailedByPolicy => cmd_filebase_failed_by_policy().await,
            FilebaseAction::Unpinned {
                policy_id,
                with_names,
            } => cmd_filebase_unpinned(&policy_id, with_names).await,
            FilebaseAction::RootAudit { sample } => cmd_filebase_root_audit(sample).await,
            FilebaseAction::RootCleanup { execute, limit } => {
                cmd_filebase_root_cleanup(execute, limit).await
            }
            FilebaseAction::RootRelocate {
                execute,
                limit,
                folder,
            } => cmd_filebase_root_relocate(execute, limit, &folder).await,
            FilebaseAction::ClassifyUncategorized {
                execute,
                source_folder,
                limit,
            } => cmd_filebase_classify_uncategorized(execute, &source_folder, limit).await,
            FilebaseAction::S3Head { key } => cmd_filebase_s3_head(&key).await,
            FilebaseAction::DeletePrefix {
                prefix,
                execute,
                limit,
            } => cmd_filebase_delete_prefix(&prefix, execute, limit).await,
            FilebaseAction::DeleteSafetyTest => cmd_filebase_delete_safety_test().await,
            FilebaseAction::S3List { prefix, limit } => cmd_filebase_s3_list(&prefix, limit).await,
            FilebaseAction::RescueCleanup { policy_id, execute } => {
                cmd_filebase_rescue_cleanup(&policy_id, execute).await
            }
            FilebaseAction::Rescue {
                policy_id,
                limit,
                execute,
            } => cmd_filebase_rescue(&policy_id, limit, execute).await,
            FilebaseAction::PruneFailed {
                policy_id,
                execute,
                limit,
                delay_ms,
            } => cmd_filebase_prune_failed(&policy_id, execute, limit, delay_ms).await,
            FilebaseAction::PruneV0 {
                policy_id,
                execute,
                limit,
                delay_ms,
            } => cmd_filebase_prune_v0(&policy_id, execute, limit, delay_ms).await,
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
    source: PinSource,
) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    use std::time::Duration;
    use viewer_ingest::{
        CidIndexClient, CidIndexStatus, FilebaseClient, MaestroClient, PinItem,
    };

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let config = load_cardano_config(policy_id, config_path.clone())?;
    let slug = config.slug.clone();

    println!("Filebase pin for: {}", policy_id);

    let cids: Vec<String> = match source {
        PinSource::CidIndex => {
            println!("Fetching CID index from ownership.cnft.dev...");
            let index = CidIndexClient::new().fetch_all(policy_id).await?;
            println!(
                "  Index status: {:?} (generation {})",
                index.status, index.cid_generation
            );
            if index.status != CidIndexStatus::Complete {
                println!("  WARNING: index is not fully resolved — the CID set may be incomplete.");
            }
            index.cids
        }
        PinSource::Maestro => {
            println!("Extracting CIDs from Maestro (local cardano-assets)...");
            MaestroClient::from_env()?
                .fetch_policy_cids(policy_id)
                .await?
        }
    };
    println!("  CIDs to pin: {}", cids.len());

    // CIDs are CIDv1-normalised and deduplicated, so pins are identified by
    // CID alone. No per-asset names are available — meta carries only
    // policy_id and slug.
    let mut items: Vec<PinItem> = cids
        .iter()
        .map(|cid| {
            let mut meta = BTreeMap::new();
            if let Some(ref s) = slug {
                meta.insert("slug".to_string(), s.clone());
            }
            meta.insert("policy_id".to_string(), policy_id.to_string());
            PinItem {
                name: format!("{}/{}", policy_id, cid),
                cid: cid.clone(),
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

/// Report Filebase pin status counts for a collection.
async fn cmd_filebase_status(policy_id: &str) -> anyhow::Result<()> {
    use viewer_ingest::FilebaseClient;

    println!("Filebase pin status for: {}", policy_id);

    let client = FilebaseClient::from_env()?;
    let counts = client.status_counts(policy_id).await?;

    println!("  pinned:  {}", counts.pinned);
    println!("  pinning: {}", counts.pinning);
    println!("  queued:  {}", counts.queued);
    println!("  failed:  {}", counts.failed);
    println!("  -------");
    println!("  total:   {}", counts.total());

    let unresolved = counts.queued + counts.pinning + counts.failed;
    if unresolved > 0 {
        println!(
            "\n{} pin(s) not yet resolved ({} queued, {} pinning, {} failed).",
            unresolved, counts.queued, counts.pinning, counts.failed
        );
    } else if counts.total() > 0 {
        println!("\nAll pins resolved.");
    }

    Ok(())
}

/// Group every `failed` pin in the bucket by policy_id (extracted from
/// the pin name's first segment). Reports per-policy counts sorted
/// descending so the rescue can be driven per policy.
async fn cmd_filebase_failed_by_policy() -> anyhow::Result<()> {
    use std::collections::HashMap;
    use viewer_ingest::FilebaseClient;

    println!("Listing all failed pins across the bucket...");
    let client = FilebaseClient::from_env()?;
    let pins = client.list_all_pins_with_status_global("failed").await?;
    println!("  Total failed pins: {}", pins.len());

    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut unknown = 0usize;
    for pin in &pins {
        match pin.name.as_deref() {
            Some(name) => match name.split_once('/') {
                Some((policy, _)) => {
                    *counts.entry(policy.to_string()).or_insert(0) += 1;
                }
                None => unknown += 1,
            },
            None => unknown += 1,
        }
    }

    let mut rows: Vec<(String, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    println!("\nPer-policy failed counts:");
    for (policy, count) in &rows {
        println!("  {count:>5}  {policy}");
    }
    if unknown > 0 {
        println!("  {unknown:>5}  (no policy in name)");
    }

    Ok(())
}

/// List a collection's content with no successful pin on Filebase.
///
/// Collapses CIDv0/v1 of the same block by multihash, so content counts
/// as covered if *either* version reached `pinned`. The genuinely-failed
/// set (every pin `failed`) is the input for an nftcdn-based rescue;
/// still-resolving content just needs more time.
async fn cmd_filebase_unpinned(policy_id: &str, with_names: bool) -> anyhow::Result<()> {
    use std::collections::HashMap;
    use viewer_ingest::{
        AssetSource, FilebaseClient, MaestroSource, extract_cid, to_cidv1,
    };

    println!("Filebase unpinned for: {}", policy_id);

    let client = FilebaseClient::from_env()?;
    println!("Listing all pins...");
    let pins = client.list_all_pins(policy_id).await?;
    println!("  Total pins: {}", pins.len());

    // Group pins by canonical v1 CID — collapses v0/v1 of the same block.
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let mut unparseable = 0usize;
    for pin in &pins {
        match to_cidv1(&pin.cid) {
            Some(v1) => groups.entry(v1).or_default().push(pin.status.clone()),
            None => unparseable += 1,
        }
    }

    // Unpinned content = no pin for it reached `pinned`.
    let mut unpinned: Vec<(&String, Vec<String>)> = groups
        .iter()
        .filter(|(_, statuses)| !statuses.iter().any(|s| s == "pinned"))
        .map(|(cid, statuses)| {
            let mut s: Vec<String> = statuses.clone();
            s.sort();
            s.dedup();
            (cid, s)
        })
        .collect();
    unpinned.sort_by(|a, b| a.0.cmp(b.0));

    let total_content = groups.len();
    let covered = total_content - unpinned.len();

    println!("  Unique content (by multihash): {}", total_content);
    println!("  Covered (>=1 pin `pinned`):    {}", covered);
    println!("  Unpinned (0 pins `pinned`):    {}", unpinned.len());
    if unparseable > 0 {
        println!("  Unparseable CIDs skipped:      {}", unparseable);
    }

    if unpinned.is_empty() {
        println!("\nAll content is pinned.");
        return Ok(());
    }

    // Split: every-pin-failed = genuine rescue target; anything still
    // queued/pinning just needs more time.
    let (failed, resolving): (Vec<_>, Vec<_>) = unpinned
        .iter()
        .partition(|(_, statuses)| statuses.iter().all(|s| s == "failed"));

    println!("\n  Genuinely failed (rescue target): {}", failed.len());
    println!("  Still resolving (just wait):      {}", resolving.len());

    if failed.is_empty() {
        return Ok(());
    }

    // Optionally resolve each CID back to its asset via Maestro for a
    // human-readable report.
    let names: HashMap<String, String> = if with_names {
        println!("\nResolving asset names via Maestro...");
        let assets = MaestroSource::from_env()?.fetch_collection(policy_id).await?;
        let mut map = HashMap::new();
        for a in &assets {
            if let Some(url) = a.image_url.as_ref() {
                if let Some(cid) = extract_cid(url) {
                    if let Some(v1) = to_cidv1(&cid) {
                        map.entry(v1).or_insert_with(|| a.display_name.clone());
                    }
                }
            }
        }
        println!("  Mapped {} CIDs to assets", map.len());
        map
    } else {
        HashMap::new()
    };

    println!("\nFailed content (needs rescue):");
    for (cid, _) in &failed {
        if with_names {
            let name = names.get(cid.as_str()).map(|s| s.as_str()).unwrap_or("(unknown)");
            println!("{}\t{}", name, cid);
        } else {
            println!("{}", cid);
        }
    }

    Ok(())
}

/// Audit root-level bucket objects: enumerate everything directly at root
/// (no `/` in key), enumerate every top-level policy folder, build the set
/// of v1 CIDs covered by folder copies, and classify each root entry as
/// "covered" (folder copy exists) or "orphan" (no folder copy).
async fn cmd_filebase_root_audit(sample: usize) -> anyhow::Result<()> {
    use std::collections::HashSet;
    use viewer_ingest::{FilebaseS3Client, to_cidv1};

    let s3 = FilebaseS3Client::from_env()?;
    println!("Auditing bucket={}", s3.bucket_name());

    // 1. Enumerate root files + top-level policy folders.
    println!("Listing root level...");
    let (root_objects, policy_prefixes) = s3.list_with_delimiter("", "/").await?;
    println!("  Root objects:        {}", root_objects.len());
    println!("  Policy folders:      {}", policy_prefixes.len());

    // 2. Walk each policy folder; build a set of v1 CIDs covered by folder copies.
    println!("Scanning policy folders for covered v1 CIDs...");
    let mut covered_v1: HashSet<String> = HashSet::new();
    let mut policy_object_count = 0usize;
    for prefix in &policy_prefixes {
        let entries = s3.list_prefix(prefix).await?;
        for (key, _size) in &entries {
            policy_object_count += 1;
            // The leaf may be an asset name (e.g. "0001") OR a CID (bafy/Qm).
            if let Some(leaf) = key.rsplit('/').next() {
                if let Some(v1) = to_cidv1(leaf) {
                    covered_v1.insert(v1);
                }
            }
        }
    }
    println!("  Total folder objects: {}", policy_object_count);
    println!("  Unique CIDs covered:  {}", covered_v1.len());

    // 3. Classify each root entry: covered (has folder copy by same multihash)
    // vs orphan (no folder copy).
    let mut covered_root: Vec<(String, u64)> = Vec::new();
    let mut orphan_root: Vec<(String, u64)> = Vec::new();
    let mut non_cid_root: Vec<(String, u64)> = Vec::new();
    for (key, size) in &root_objects {
        match to_cidv1(key) {
            Some(v1) => {
                if covered_v1.contains(&v1) {
                    covered_root.push((key.clone(), *size));
                } else {
                    orphan_root.push((key.clone(), *size));
                }
            }
            None => non_cid_root.push((key.clone(), *size)),
        }
    }

    println!("\nRoot classification:");
    println!("  Covered (folder copy exists, safe to delete): {}", covered_root.len());
    println!("  Orphan  (no folder copy — needs rescue):      {}", orphan_root.len());
    println!("  Non-CID root entry (manual review):           {}", non_cid_root.len());

    fn print_sample(label: &str, rows: &[(String, u64)], n: usize) {
        if rows.is_empty() {
            return;
        }
        println!("\n{label} (first {}):", n.min(rows.len()));
        for (key, size) in rows.iter().take(n) {
            println!("  {} ({} bytes)", key, size);
        }
        if rows.len() > n {
            println!("  ... and {} more", rows.len() - n);
        }
    }

    print_sample("Covered root entries", &covered_root, sample);
    print_sample("Orphan root entries", &orphan_root, sample);
    print_sample("Non-CID root entries", &non_cid_root, sample);

    Ok(())
}

/// Delete `failed` Pinning Service pin records for a policy when the
/// CID's content has a folder copy in the bucket — content is preserved
/// via the folder bucket entry, so the stale failed pin record can go.
async fn cmd_filebase_prune_failed(
    policy_id: &str,
    execute: bool,
    limit: Option<usize>,
    delay_ms: Option<u64>,
) -> anyhow::Result<()> {
    use std::collections::HashSet;
    use std::time::Duration;
    use viewer_ingest::{FilebaseClient, FilebaseS3Client, to_cidv1};

    println!("Filebase prune-failed for: {}", policy_id);

    let mut client = FilebaseClient::from_env()?;
    if let Some(ms) = delay_ms {
        client = client.with_delay(Duration::from_millis(ms));
    }
    let s3 = FilebaseS3Client::from_env()?;

    // 1. Failed pin records for this policy.
    let pins = client.list_all_pins(policy_id).await?;
    let failed: Vec<&viewer_ingest::PinRecord> =
        pins.iter().filter(|p| p.status == "failed").collect();
    println!("  Failed pin records: {}", failed.len());
    if failed.is_empty() {
        println!("Nothing to prune.");
        return Ok(());
    }

    // 2. Build covered-v1 set from this policy's folder.
    let folder_prefix = format!("{policy_id}/");
    println!("Listing {folder_prefix} for covered CIDs...");
    let entries = s3.list_prefix(&folder_prefix).await?;
    let prefix_len = folder_prefix.len();
    let covered_v1: HashSet<String> = entries
        .iter()
        .filter_map(|(key, _)| {
            let leaf = key.get(prefix_len..)?;
            to_cidv1(leaf)
        })
        .collect();
    println!("  Covered v1 CIDs in folder: {}", covered_v1.len());

    // 3. Classify each failed pin record.
    let mut eligible: Vec<String> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for pin in &failed {
        let Some(v1) = to_cidv1(&pin.cid) else {
            skipped.push((pin.cid.clone(), "unparseable CID".to_string()));
            continue;
        };
        if covered_v1.contains(&v1) {
            eligible.push(pin.requestid.clone());
        } else {
            skipped.push((pin.cid.clone(), "no folder copy".to_string()));
        }
    }

    println!(
        "\n  Eligible for deletion (folder copy exists): {}",
        eligible.len()
    );
    println!("  Skipped (no folder copy):                   {}", skipped.len());
    if !skipped.is_empty() && !execute {
        println!("\nFirst 5 skipped:");
        for (cid, reason) in skipped.iter().take(5) {
            println!("  {} — {}", cid, reason);
        }
    }

    if !execute {
        println!(
            "\nPREVIEW — re-run with --execute to delete {} stale pin record(s).",
            eligible.len()
        );
        return Ok(());
    }

    if let Some(n) = limit {
        eligible.truncate(n);
        println!("\nLimited to first {}", eligible.len());
    }
    if eligible.is_empty() {
        println!("\nNothing safe to delete.");
        return Ok(());
    }

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(eligible.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let results = client
        .delete_pins(&eligible, Some(&|done, _total| pb.set_position(done as u64)))
        .await;
    pb.finish_and_clear();

    let succeeded = results.iter().filter(|r| r.is_ok()).count();
    let failed_count = results.len() - succeeded;

    println!("\nPrune-failed complete:");
    println!("  Deleted: {}", succeeded);
    if failed_count > 0 {
        println!("  Failed:  {}", failed_count);
        for (i, r) in results.iter().enumerate().take(10) {
            if let Err(e) = r {
                println!("    {} — {}", &eligible[i], e);
            }
        }
    }

    Ok(())
}

/// Delete root-level CID-keyed bucket objects that have a folder copy
/// (the audit's "covered" set). Preview by default; --execute to delete.
async fn cmd_filebase_root_cleanup(execute: bool, limit: Option<usize>) -> anyhow::Result<()> {
    use std::collections::{HashMap, HashSet};
    use viewer_ingest::{FilebaseS3Client, to_cidv1};

    let s3 = FilebaseS3Client::from_env()?;
    println!("Filebase root-cleanup (bucket={})", s3.bucket_name());

    println!("Listing root + policy folders...");
    let (root_objects, policy_prefixes) = s3.list_with_delimiter("", "/").await?;
    println!(
        "  Root objects: {}, policy folders: {}",
        root_objects.len(),
        policy_prefixes.len()
    );

    println!("Scanning policy folders for covered v1 CIDs...");
    let mut covered_v1: HashSet<String> = HashSet::new();
    for prefix in &policy_prefixes {
        let entries = s3.list_prefix(prefix).await?;
        for (key, _) in &entries {
            if let Some(leaf) = key.rsplit('/').next() {
                if let Some(v1) = to_cidv1(leaf) {
                    covered_v1.insert(v1);
                }
            }
        }
    }
    println!("  Unique CIDs covered: {}", covered_v1.len());

    let mut eligible: Vec<String> = Vec::new();
    let mut orphan = 0usize;
    let mut non_cid = 0usize;
    for (key, _) in &root_objects {
        match to_cidv1(key) {
            Some(v1) => {
                if covered_v1.contains(&v1) {
                    eligible.push(key.clone());
                } else {
                    orphan += 1;
                }
            }
            None => non_cid += 1,
        }
    }
    eligible.sort();

    println!("\nEligible for deletion (folder copy exists): {}", eligible.len());
    println!("Orphan root entries (kept — would lose content): {}", orphan);
    if non_cid > 0 {
        println!("Non-CID root entries (kept — manual review):     {}", non_cid);
    }

    if let Some(n) = limit {
        if execute {
            eligible.truncate(n);
            println!("\nLimited to first {} deletion(s).", eligible.len());
        }
    }

    if !execute {
        println!(
            "\nPREVIEW — re-run with --execute to delete the {} eligible root object(s).",
            eligible.len()
        );
        return Ok(());
    }
    if eligible.is_empty() {
        println!("\nNothing to delete.");
        return Ok(());
    }

    // Build a v1 -> set-of-folder-keys map for the post-delete safety check,
    // exiting early if any eligible entry's coverage disappears between scan
    // and execute (unlikely but cheap defence).
    let mut v1_to_folders: HashMap<String, Vec<String>> = HashMap::new();
    for prefix in &policy_prefixes {
        let entries = s3.list_prefix(prefix).await?;
        for (key, _) in &entries {
            if let Some(leaf) = key.rsplit('/').next() {
                if let Some(v1) = to_cidv1(leaf) {
                    v1_to_folders.entry(v1).or_default().push(key.clone());
                }
            }
        }
    }

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(eligible.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut deleted = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut errors: Vec<(String, String)> = Vec::new();

    for key in &eligible {
        pb.inc(1);
        // Re-check coverage just before deleting (cheap defence).
        let v1 = match to_cidv1(key) {
            Some(v1) => v1,
            None => {
                skipped += 1;
                continue;
            }
        };
        if !v1_to_folders.contains_key(&v1) {
            skipped += 1;
            continue;
        }
        match s3.delete_object(key).await {
            Ok(()) => deleted += 1,
            Err(e) => {
                failed += 1;
                errors.push((key.clone(), e.to_string()));
            }
        }
    }
    pb.finish_and_clear();

    println!("\nCleanup complete:");
    println!("  Deleted: {}", deleted);
    if skipped > 0 {
        println!("  Skipped (coverage missing): {}", skipped);
    }
    if failed > 0 {
        println!("  Failed:  {}", failed);
        for (k, e) in errors.iter().take(10) {
            println!("    {} — {}", k, e);
        }
    }

    Ok(())
}

/// Build a forward CID -> policy index from every known policy folder
/// (via Maestro), then move matched objects out of the source folder
/// into `{policy_id}/{v1_cid}`. Unmatched objects stay put — they belong
/// to a policy outside our known set and need separate triage.
async fn cmd_filebase_classify_uncategorized(
    execute: bool,
    source_folder: &str,
    limit: Option<usize>,
) -> anyhow::Result<()> {
    use std::collections::HashMap;
    use viewer_ingest::{FilebaseS3Client, MaestroClient, to_cidv1};

    fn is_policy_id(s: &str) -> bool {
        s.len() == 56 && s.chars().all(|c| c.is_ascii_hexdigit())
    }

    let s3 = FilebaseS3Client::from_env()?;
    let src_folder = source_folder.trim_end_matches('/').to_string();

    println!("Enumerating policy folders in bucket={}...", s3.bucket_name());
    let (_, prefixes) = s3.list_with_delimiter("", "/").await?;
    let mut policies: Vec<String> = prefixes
        .iter()
        .filter_map(|p| {
            let trimmed = p.trim_end_matches('/').to_string();
            if is_policy_id(&trimmed) && trimmed != src_folder {
                Some(trimmed)
            } else {
                None
            }
        })
        .collect();
    policies.sort();
    println!("  Found {} policy folder(s)", policies.len());

    println!("\nIndexing each policy's CIDs via Maestro...");
    let maestro = MaestroClient::from_env()?;
    let mut cid_to_policy: HashMap<String, String> = HashMap::new();
    let mut indexed = 0usize;
    let mut failed_index: Vec<(String, String)> = Vec::new();

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(policies.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );
    for policy in &policies {
        pb.set_message(policy.clone());
        match maestro.fetch_policy_cids(policy).await {
            Ok(cids) => {
                indexed += 1;
                for cid in cids {
                    cid_to_policy.entry(cid).or_insert_with(|| policy.clone());
                }
            }
            Err(e) => failed_index.push((policy.clone(), e.to_string())),
        }
        pb.inc(1);
    }
    pb.finish_and_clear();
    println!(
        "  Indexed {indexed}/{} policies — {} unique CIDs",
        policies.len(),
        cid_to_policy.len()
    );
    if !failed_index.is_empty() {
        println!("  Failed to index {} policies:", failed_index.len());
        for (p, e) in failed_index.iter().take(5) {
            println!("    {} — {}", p, e);
        }
    }

    let src_prefix = format!("{src_folder}/");
    println!("\nListing source folder {src_prefix}");
    let entries = s3.list_prefix(&src_prefix).await?;
    println!("  Objects: {}", entries.len());

    let prefix_len = src_prefix.len();
    let mut moves: Vec<(String, String)> = Vec::new(); // (src_key, dst_key)
    let mut unmatched = 0usize;
    let mut non_cid = 0usize;
    for (key, _) in &entries {
        let Some(leaf) = key.get(prefix_len..) else {
            continue;
        };
        let Some(v1) = to_cidv1(leaf) else {
            non_cid += 1;
            continue;
        };
        match cid_to_policy.get(&v1) {
            Some(policy) => moves.push((key.clone(), format!("{policy}/{v1}"))),
            None => unmatched += 1,
        }
    }

    println!("\nClassification:");
    println!("  Matched (move into policy folder): {}", moves.len());
    println!("  Unmatched (stays in {src_folder}):     {}", unmatched);
    if non_cid > 0 {
        println!("  Non-CID keys (skipped): {}", non_cid);
    }

    if !execute {
        println!("\nFirst 10 planned moves:");
        for (src, dst) in moves.iter().take(10) {
            println!("  {} -> {}", src, dst);
        }
        if moves.len() > 10 {
            println!("  ... and {} more", moves.len() - 10);
        }
        println!(
            "\nPREVIEW — re-run with --execute to move {} object(s).",
            moves.len()
        );
        return Ok(());
    }

    let mut to_process = moves;
    if let Some(n) = limit {
        to_process.truncate(n);
        println!("\nLimited to first {}", to_process.len());
    }
    if to_process.is_empty() {
        println!("\nNothing to move.");
        return Ok(());
    }

    let pb = ProgressBar::new(to_process.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut copied = 0usize;
    let mut deleted = 0usize;
    let mut copy_failed = 0usize;
    let mut head_failed = 0usize;
    let mut delete_failed = 0usize;
    let mut errors: Vec<(String, String)> = Vec::new();

    for (src, dst) in &to_process {
        pb.inc(1);
        if let Err(e) = s3.copy_object(src, dst).await {
            copy_failed += 1;
            errors.push((src.clone(), format!("copy: {e}")));
            continue;
        }
        copied += 1;
        if let Err(e) = s3.head_object(dst).await {
            head_failed += 1;
            errors.push((src.clone(), format!("post-copy HEAD: {e}")));
            continue;
        }
        if let Err(e) = s3.delete_object(src).await {
            delete_failed += 1;
            errors.push((src.clone(), format!("delete: {e}")));
            continue;
        }
        deleted += 1;
    }
    pb.finish_and_clear();

    println!("\nClassify complete:");
    println!("  Copied:  {copied}");
    println!("  Deleted: {deleted}");
    if copy_failed + head_failed + delete_failed > 0 {
        println!(
            "  Errors:  copy={copy_failed} head={head_failed} delete={delete_failed}"
        );
        for (k, e) in errors.iter().take(10) {
            println!("    {} — {}", k, e);
        }
    }

    Ok(())
}

/// Relocate orphan root objects into a destination folder via S3 COPY,
/// then DELETE the root entry. Each delete is guarded by HEAD on the
/// destination key — root is only removed after the copy is confirmed.
async fn cmd_filebase_root_relocate(
    execute: bool,
    limit: Option<usize>,
    folder: &str,
) -> anyhow::Result<()> {
    use viewer_ingest::FilebaseS3Client;

    let s3 = FilebaseS3Client::from_env()?;
    println!(
        "Filebase root-relocate (bucket={}, destination folder={folder})",
        s3.bucket_name()
    );

    let (root_objects, _) = s3.list_with_delimiter("", "/").await?;
    println!("  Root objects: {}", root_objects.len());

    let mut targets: Vec<String> = root_objects.iter().map(|(k, _)| k.clone()).collect();
    targets.sort();
    if let Some(n) = limit {
        if execute {
            targets.truncate(n);
            println!("  Limited to first {}", targets.len());
        }
    }

    if !execute {
        println!("\nPreview (first 5):");
        for k in targets.iter().take(5) {
            println!("  {} -> {}/{}", k, folder, k);
        }
        if targets.len() > 5 {
            println!("  ... and {} more", targets.len() - 5);
        }
        println!(
            "\nPREVIEW — re-run with --execute to relocate {} root entries.",
            targets.len()
        );
        return Ok(());
    }

    if targets.is_empty() {
        println!("\nNothing to relocate.");
        return Ok(());
    }

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(targets.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut copied = 0usize;
    let mut deleted = 0usize;
    let mut copy_failed = 0usize;
    let mut head_failed = 0usize;
    let mut delete_failed = 0usize;
    let mut errors: Vec<(String, String)> = Vec::new();

    for src in &targets {
        pb.inc(1);
        let dst = format!("{folder}/{src}");

        if let Err(e) = s3.copy_object(src, &dst).await {
            copy_failed += 1;
            errors.push((src.clone(), format!("copy: {e}")));
            continue;
        }
        copied += 1;

        // Guard: HEAD the new key before deleting the old.
        if let Err(e) = s3.head_object(&dst).await {
            head_failed += 1;
            errors.push((src.clone(), format!("post-copy HEAD: {e}")));
            continue;
        }

        if let Err(e) = s3.delete_object(src).await {
            delete_failed += 1;
            errors.push((src.clone(), format!("delete: {e}")));
            continue;
        }
        deleted += 1;
    }
    pb.finish_and_clear();

    println!("\nRelocation complete:");
    println!("  Copied:            {}", copied);
    println!("  Deleted from root: {}", deleted);
    if copy_failed > 0 {
        println!("  Copy failed:       {}", copy_failed);
    }
    if head_failed > 0 {
        println!("  Post-copy HEAD failed (root kept): {}", head_failed);
    }
    if delete_failed > 0 {
        println!("  Delete failed (copy succeeded):    {}", delete_failed);
    }
    if !errors.is_empty() {
        println!("\nFirst 10 errors:");
        for (k, e) in errors.iter().take(10) {
            println!("  {} — {}", k, e);
        }
    }

    Ok(())
}

/// Empirical safety test for deleting a root-level CID-keyed object when a
/// folder-keyed copy of the same content exists elsewhere. Finds a real
/// existing "covered" pair in the bucket, deletes the root entry, and
/// confirms the folder copy survives unchanged.
async fn cmd_filebase_delete_safety_test() -> anyhow::Result<()> {
    use std::collections::HashMap;
    use std::time::Duration;
    use viewer_ingest::{FilebaseS3Client, to_cidv1};

    let s3 = FilebaseS3Client::from_env()?;

    println!("Finding a candidate root entry with a folder copy...");
    let (root_objects, policy_prefixes) = s3.list_with_delimiter("", "/").await?;
    println!(
        "  Root objects: {}, policy folders: {}",
        root_objects.len(),
        policy_prefixes.len()
    );

    // Map v1 CID -> first folder key with that CID
    let mut v1_to_folder_key: HashMap<String, String> = HashMap::new();
    for prefix in &policy_prefixes {
        let entries = s3.list_prefix(prefix).await?;
        for (key, _) in &entries {
            if let Some(leaf) = key.rsplit('/').next() {
                if let Some(v1) = to_cidv1(leaf) {
                    v1_to_folder_key.entry(v1).or_insert_with(|| key.clone());
                }
            }
        }
    }

    let candidate = root_objects.iter().find_map(|(k, _)| {
        to_cidv1(k).and_then(|v1| v1_to_folder_key.get(&v1).map(|fk| (k.clone(), fk.clone())))
    });
    let (root_key, folder_key) =
        candidate.ok_or_else(|| anyhow::anyhow!("no covered root entry found"))?;
    println!("  Picked root key:   {root_key}");
    println!("  Folder copy:       {folder_key}");

    let print_head = |label: &str, h: &viewer_ingest::filebase_s3::HeadInfo| {
        let pin = h
            .metadata
            .get("pinning-status")
            .map(|s| s.as_str())
            .unwrap_or("(none)");
        println!(
            "  {label}: size={} cid={} pinning-status={}",
            h.content_length,
            h.cid().unwrap_or("(none)"),
            pin
        );
    };

    println!("\n[1] HEAD root BEFORE:");
    let head_root_before = s3.head_object(&root_key).await?;
    print_head("root_before", &head_root_before);

    println!("\n[2] HEAD folder BEFORE:");
    let head_folder_before = s3.head_object(&folder_key).await?;
    print_head("folder_before", &head_folder_before);

    if head_root_before.cid() != head_folder_before.cid() {
        anyhow::bail!(
            "CID mismatch — root={:?} folder={:?}; aborting",
            head_root_before.cid(),
            head_folder_before.cid()
        );
    }

    println!("\n[3] DELETE root entry {root_key}");
    s3.delete_object(&root_key).await?;
    println!("  deleted; sleeping for consistency");
    tokio::time::sleep(Duration::from_secs(3)).await;

    println!("\n[4] HEAD folder AFTER root delete:");
    let head_folder_after = s3.head_object(&folder_key).await?;
    print_head("folder_after", &head_folder_after);

    let same_cid = head_folder_after.cid() == head_folder_before.cid();
    let same_size = head_folder_after.content_length == head_folder_before.content_length;
    let pinned_after = head_folder_after
        .metadata
        .get("pinning-status")
        .map(|s| s.as_str())
        == Some("pinned");

    if same_cid && same_size && pinned_after {
        println!("\nVERDICT: SAFE — folder copy intact, same CID, still pinned. Bulk root cleanup is safe.");
    } else {
        println!(
            "\nVERDICT: NOT SAFE — folder copy changed. same_cid={same_cid} same_size={same_size} pinned_after={pinned_after}"
        );
    }

    println!("\n[5] HEAD root AFTER delete (expecting gone):");
    match s3.head_object(&root_key).await {
        Ok(_) => println!("  root entry STILL resolves — Filebase may have re-materialized it"),
        Err(e) => println!("  confirmed gone: {e}"),
    }

    Ok(())
}

/// Delete every object under the given key prefix.
async fn cmd_filebase_delete_prefix(
    prefix: &str,
    execute: bool,
    limit: Option<usize>,
) -> anyhow::Result<()> {
    use viewer_ingest::FilebaseS3Client;

    if prefix.is_empty() {
        anyhow::bail!("refusing to delete with empty prefix — that's the whole bucket");
    }

    let s3 = FilebaseS3Client::from_env()?;
    println!("delete-prefix bucket={} prefix={prefix}", s3.bucket_name());

    let entries = s3.list_prefix(prefix).await?;
    let mut keys: Vec<String> = entries.into_iter().map(|(k, _)| k).collect();
    keys.sort();
    println!("  Objects matching prefix: {}", keys.len());

    if !execute {
        for k in keys.iter().take(10) {
            println!("  would delete: {}", k);
        }
        if keys.len() > 10 {
            println!("  ... and {} more", keys.len() - 10);
        }
        println!(
            "\nPREVIEW — re-run with --execute to delete {} object(s).",
            keys.len()
        );
        return Ok(());
    }

    if let Some(n) = limit {
        keys.truncate(n);
        println!("  Limited to first {}", keys.len());
    }
    if keys.is_empty() {
        println!("\nNothing to delete.");
        return Ok(());
    }

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(keys.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut deleted = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<(String, String)> = Vec::new();
    for k in &keys {
        pb.inc(1);
        match s3.delete_object(k).await {
            Ok(()) => deleted += 1,
            Err(e) => {
                failed += 1;
                errors.push((k.clone(), e.to_string()));
            }
        }
    }
    pb.finish_and_clear();

    println!("\nDelete complete:");
    println!("  Deleted: {}", deleted);
    if failed > 0 {
        println!("  Failed:  {}", failed);
        for (k, e) in errors.iter().take(10) {
            println!("    {} — {}", k, e);
        }
    }

    Ok(())
}

/// HEAD a bucket object and print its size + metadata.
async fn cmd_filebase_s3_head(key: &str) -> anyhow::Result<()> {
    use viewer_ingest::FilebaseS3Client;

    let s3 = FilebaseS3Client::from_env()?;
    let info = s3.head_object(key).await?;

    println!("Key:            {}", info.key);
    println!("Content-Length: {}", info.content_length);
    if let Some(etag) = &info.etag {
        println!("ETag:           {}", etag);
    }
    if let Some(ct) = &info.content_type {
        println!("Content-Type:   {}", ct);
    }
    if let Some(cid) = info.cid() {
        println!("CID (meta):     {}", cid);
    }
    if !info.metadata.is_empty() {
        println!("All metadata:");
        for (k, v) in &info.metadata {
            println!("  {} = {}", k, v);
        }
    }
    Ok(())
}

/// List bucket objects with the given S3 prefix (debug helper).
async fn cmd_filebase_s3_list(prefix: &str, limit: usize) -> anyhow::Result<()> {
    use viewer_ingest::FilebaseS3Client;

    let s3 = FilebaseS3Client::from_env()?;
    println!("Listing bucket={} prefix={:?}", s3.bucket_name(), prefix);
    let objects = s3.list_prefix(prefix).await?;
    println!("  Total objects: {}", objects.len());
    for (key, size) in objects.iter().take(limit) {
        println!("  {} ({} bytes)", key, size);
    }
    if objects.len() > limit {
        println!("  ... and {} more", objects.len() - limit);
    }
    Ok(())
}

/// Delete root-level CIDv0 bucket objects left by an earlier RPC-add
/// rescue, scoped to a single policy. Safe by construction: each delete
/// is gated on the corresponding v1 entry under `{policy}/...` being
/// populated (>0 bytes), so content always has at least one bucket copy.
async fn cmd_filebase_rescue_cleanup(policy_id: &str, execute: bool) -> anyhow::Result<()> {
    use std::collections::{HashMap, HashSet};
    use viewer_ingest::{FilebaseClient, FilebaseS3Client, to_cidv0, to_cidv1};

    println!("Filebase rescue-cleanup for: {}", policy_id);

    // 1. Failed v1 CID set (same as `unpinned` — content the rescue targeted).
    let client = FilebaseClient::from_env()?;
    println!("Listing pins...");
    let pins = client.list_all_pins(policy_id).await?;
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for pin in &pins {
        if let Some(v1) = to_cidv1(&pin.cid) {
            groups.entry(v1).or_default().push(pin.status.clone());
        }
    }
    let rescue_v1: Vec<String> = groups
        .into_iter()
        .filter(|(_, s)| !s.is_empty() && s.iter().all(|x| x == "failed"))
        .map(|(cid, _)| cid)
        .collect();
    println!("  Rescue v1 CIDs:    {}", rescue_v1.len());

    // 2. Populated v1 entries under the policy folder.
    let s3 = FilebaseS3Client::from_env()?;
    let folder_prefix = format!("{policy_id}/");
    let folder_entries = s3.list_prefix(&folder_prefix).await?;
    let prefix_len = folder_prefix.len();
    let populated_v1: HashSet<String> = folder_entries
        .iter()
        .filter(|(_, size)| *size > 0)
        .filter_map(|(key, _)| {
            let leaf = key.get(prefix_len..)?;
            if leaf.starts_with("bafy") {
                Some(leaf.to_string())
            } else {
                None
            }
        })
        .collect();
    println!("  Populated v1 in folder: {}", populated_v1.len());

    // 3. For each rescue v1, decide whether the v0 root copy is safe to delete.
    let mut eligible: Vec<(String, String)> = Vec::new(); // (v0 root key, v1 cid)
    let mut skipped: Vec<(String, String)> = Vec::new();
    for v1 in &rescue_v1 {
        if !populated_v1.contains(v1) {
            skipped.push((v1.clone(), "folder entry not populated".to_string()));
            continue;
        }
        let Some(v0) = to_cidv0(v1) else {
            skipped.push((v1.clone(), "no v0 representation".to_string()));
            continue;
        };
        eligible.push((v0, v1.clone()));
    }

    println!("\nEligible for delete (populated folder copy exists): {}", eligible.len());
    println!("Skipped (unsafe to delete):                          {}", skipped.len());

    if !skipped.is_empty() {
        println!("\nFirst 10 skip reasons:");
        for (v1, reason) in skipped.iter().take(10) {
            println!("  {} — {}", v1, reason);
        }
    }

    if eligible.is_empty() {
        println!("\nNothing safe to clean up.");
        return Ok(());
    }

    if !execute {
        println!(
            "\nPREVIEW — re-run with --execute to delete {} orphan root object(s).",
            eligible.len()
        );
        return Ok(());
    }

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(eligible.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut deleted = 0usize;
    let mut failed = 0usize;
    let mut errors: Vec<(String, String)> = Vec::new();
    for (v0, v1) in &eligible {
        pb.inc(1);
        match s3.delete_object(v0).await {
            Ok(()) => deleted += 1,
            Err(e) => {
                failed += 1;
                errors.push((v1.clone(), e.to_string()));
            }
        }
    }
    pb.finish_and_clear();

    println!("\nCleanup complete:");
    println!("  Deleted: {}", deleted);
    if failed > 0 {
        println!("  Failed:  {}", failed);
        for (cid, err) in errors.iter().take(10) {
            println!("    {} — {}", cid, err);
        }
    }

    Ok(())
}

/// Recover a collection's failed-pin content from nftcdn. For each failed
/// CID: map to its asset, fetch the image from nftcdn, and check whether
/// the bytes reproduce the on-chain CID via `find_matching_cid`. With
/// `execute`, verified content is re-hosted to Filebase via the IPFS RPC
/// `add` endpoint and the returned CID is confirmed against the original.
async fn cmd_filebase_rescue(
    policy_id: &str,
    limit: Option<usize>,
    execute: bool,
) -> anyhow::Result<()> {
    use cardano_assets::AssetId;
    use std::collections::HashMap;
    use viewer_ingest::{
        AssetSource, FilebaseClient, FilebaseS3Client, MaestroSource, NftcdnClient,
        compute_cid_bytes, extract_cid, find_matching_cid, to_cidv0, to_cidv1,
    };

    println!(
        "Filebase rescue ({}) for: {}",
        if execute { "execute" } else { "probe" },
        policy_id
    );

    let s3 = if execute {
        Some(FilebaseS3Client::from_env()?)
    } else {
        None
    };

    // 1. Failed content set from Filebase (every pin for it `failed`).
    let client = FilebaseClient::from_env()?;
    println!("Listing pins...");
    let pins = client.list_all_pins(policy_id).await?;
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for pin in &pins {
        if let Some(v1) = to_cidv1(&pin.cid) {
            groups.entry(v1).or_default().push(pin.status.clone());
        }
    }
    let mut failed: Vec<String> = groups
        .into_iter()
        .filter(|(_, s)| !s.is_empty() && s.iter().all(|x| x == "failed"))
        .map(|(cid, _)| cid)
        .collect();
    failed.sort();
    println!("  Failed content: {}", failed.len());

    if failed.is_empty() {
        println!("Nothing to rescue.");
        return Ok(());
    }

    // 2. Maestro asset list → v1 CID -> hex asset name.
    println!("Fetching asset list from Maestro...");
    let assets = MaestroSource::from_env()?.fetch_collection(policy_id).await?;
    let mut cid_to_asset: HashMap<String, String> = HashMap::new();
    for a in &assets {
        if let Some(url) = a.image_url.as_ref() {
            if let Some(cid) = extract_cid(url) {
                if let Some(v1) = to_cidv1(&cid) {
                    cid_to_asset
                        .entry(v1)
                        .or_insert_with(|| a.encoded_name.clone());
                }
            }
        }
    }
    println!("  Assets with resolvable CIDs: {}", cid_to_asset.len());

    // 3. For each failed CID: nftcdn fetch + CID verification.
    let nftcdn = NftcdnClient::from_env()?;
    let mut targets: Vec<&String> = failed.iter().collect();
    if let Some(n) = limit {
        targets.truncate(n);
        println!("  Limited to first {}", targets.len());
    }

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(targets.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut unmapped = 0usize;
    let mut fetch_failed = 0usize;
    let mut reproduced = 0usize;
    let mut mismatch = 0usize;
    let mut mismatch_samples: Vec<(String, String, usize)> = Vec::new();
    let mut rehosted = 0usize;
    let mut rehost_failed = 0usize;
    let mut rehost_errors: Vec<(String, String)> = Vec::new();

    for cid in targets {
        pb.inc(1);

        let Some(encoded_name) = cid_to_asset.get(cid) else {
            unmapped += 1;
            continue;
        };
        let Ok(asset_id) = AssetId::new(policy_id.to_string(), encoded_name.clone()) else {
            unmapped += 1;
            continue;
        };

        let bytes = match nftcdn.fetch_image(&asset_id).await {
            Ok(bytes) => bytes,
            Err(_) => {
                fetch_failed += 1;
                continue;
            }
        };

        if find_matching_cid(&bytes, cid).is_none() {
            mismatch += 1;
            if mismatch_samples.len() < 5 {
                let computed = compute_cid_bytes(&bytes);
                mismatch_samples.push((cid.clone(), computed.cid_v1, bytes.len()));
            }
            continue;
        }
        reproduced += 1;

        if !execute {
            continue;
        }

        // Re-host via S3 PUT under a per-policy key. Filebase auto-pins
        // content PUT to an IPFS-enabled bucket and exposes the computed
        // CID via the `x-amz-meta-cid` response header. Bytes already
        // verified locally, so an absent header isn't fatal — but when
        // present, confirm it normalises to the expected CID.
        let s3 = s3.as_ref().expect("s3 client present when execute=true");
        let key = format!("{policy_id}/{cid}");
        let put_ok = match s3.put_object(&key, &bytes).await {
            Ok(result) => {
                if let Some(returned) = &result.cid {
                    match to_cidv1(returned) {
                        Some(v1) if &v1 == cid => {
                            rehosted += 1;
                            true
                        }
                        other => {
                            rehost_failed += 1;
                            rehost_errors.push((
                                cid.clone(),
                                format!("returned CID {returned} (v1 {other:?}) != expected"),
                            ));
                            false
                        }
                    }
                } else {
                    // No CID header — bytes locally verified, trust the upload.
                    rehosted += 1;
                    true
                }
            }
            Err(e) => {
                rehost_failed += 1;
                rehost_errors.push((cid.clone(), e.to_string()));
                false
            }
        };

        // Inline cleanup: if a legacy v0 root entry exists from an earlier
        // RPC-add rescue, delete it now. Safe — we just confirmed the
        // folder copy was written via S3 PUT. Best-effort; a missing root
        // (404) is the common case and intentionally ignored.
        if put_ok {
            if let Some(v0) = to_cidv0(cid) {
                let _ = s3.delete_object(&v0).await;
            }
        }
    }
    pb.finish_and_clear();

    let probed = unmapped + fetch_failed + reproduced + mismatch;
    println!("\nRescue results ({} probed):", probed);
    println!("  Recoverable (CID reproduced): {}", reproduced);
    println!("  Fetched but CID mismatch:     {}", mismatch);
    println!("  nftcdn fetch failed:          {}", fetch_failed);
    println!("  No asset mapping:             {}", unmapped);

    if !mismatch_samples.is_empty() {
        println!("\nCID mismatch samples (expected -> nftcdn bytes):");
        for (expected, got, size) in &mismatch_samples {
            println!("  {} -> {} ({} bytes)", expected, got, size);
        }
    }

    if execute {
        println!("\nRe-host to Filebase:");
        println!("  Re-hosted (CID confirmed): {}", rehosted);
        println!("  Re-host failed:            {}", rehost_failed);
        for (cid, err) in rehost_errors.iter().take(10) {
            println!("    {} — {}", cid, err);
        }
    } else if reproduced > 0 {
        println!("\n{} CID(s) recoverable — re-run with --execute to re-host.", reproduced);
    }

    Ok(())
}

/// Remove legacy CIDv0 pins for a collection, but only where a `pinned`
/// CIDv1 pin of the same content already exists. Previews by default.
async fn cmd_filebase_prune_v0(
    policy_id: &str,
    execute: bool,
    limit: Option<usize>,
    delay_ms: Option<u64>,
) -> anyhow::Result<()> {
    use std::collections::HashMap;
    use std::time::Duration;
    use viewer_ingest::{FilebaseClient, to_cidv1};

    println!("Filebase prune-v0 for: {}", policy_id);

    let mut client = FilebaseClient::from_env()?;
    if let Some(ms) = delay_ms {
        client = client.with_delay(Duration::from_millis(ms));
    }

    println!("Listing all pins...");
    let pins = client.list_all_pins(policy_id).await?;
    println!("  Total pins: {}", pins.len());

    // CIDv0 always starts "Qm"; everything else is treated as v1+.
    // Build a status map of the non-v0 pins keyed by CID.
    let mut v1_status: HashMap<&str, &str> = HashMap::new();
    let mut v0_pins = Vec::new();
    for pin in &pins {
        if pin.cid.starts_with("Qm") {
            v0_pins.push(pin);
        } else {
            v1_status.insert(pin.cid.as_str(), pin.status.as_str());
        }
    }
    println!("  CIDv0 pins: {}", v0_pins.len());
    println!("  CIDv1 pins: {}", v1_status.len());

    // Each v0 pin is deletable only if its v1 equivalent is pinned.
    let mut eligible: Vec<(String, String)> = Vec::new(); // (requestid, v0 cid)
    let mut skipped: Vec<(String, String)> = Vec::new(); // (v0 cid, reason)
    for pin in &v0_pins {
        match to_cidv1(&pin.cid) {
            None => skipped.push((pin.cid.clone(), "cannot convert to CIDv1".to_string())),
            Some(v1) => match v1_status.get(v1.as_str()) {
                Some(&"pinned") => eligible.push((pin.requestid.clone(), pin.cid.clone())),
                Some(other) => {
                    skipped.push((pin.cid.clone(), format!("v1 sibling status={other}")))
                }
                None => skipped.push((pin.cid.clone(), "no v1 sibling pin".to_string())),
            },
        }
    }

    println!("\nEligible for deletion (v1 sibling pinned): {}", eligible.len());
    println!("Skipped (unsafe to delete):                {}", skipped.len());

    if !skipped.is_empty() {
        println!("\nSkip reasons (first 10):");
        for (cid, reason) in skipped.iter().take(10) {
            println!("  {} — {}", cid, reason);
        }
        if skipped.len() > 10 {
            println!("  ... and {} more", skipped.len() - 10);
        }
    }

    if eligible.is_empty() {
        println!("\nNothing safe to prune.");
        return Ok(());
    }

    if !execute {
        println!("\nPREVIEW — re-run with --execute to delete the {} eligible v0 pin(s).", eligible.len());
        return Ok(());
    }

    if let Some(limit) = limit {
        eligible.truncate(limit);
        println!("\nLimited to first {} deletion(s).", eligible.len());
    }

    let requestids: Vec<String> = eligible.iter().map(|(rid, _)| rid.clone()).collect();

    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new(requestids.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let results = client
        .delete_pins(
            &requestids,
            Some(&|done, _total| pb.set_position(done as u64)),
        )
        .await;
    pb.finish_and_clear();

    let deleted = results.iter().filter(|r| r.is_ok()).count();
    let failed = results.len() - deleted;
    println!("\nPrune complete:");
    println!("  Deleted: {}", deleted);
    if failed > 0 {
        println!("  Failed:  {}", failed);
        for (result, (_, cid)) in results.iter().zip(eligible.iter()) {
            if let Err(e) = result {
                println!("    {} — {}", cid, e);
            }
        }
    }

    Ok(())
}

/// Fetch one page from Maestro for a policy and report parse results.
///
/// Reports: how many assets came back, how many had CIP-25 metadata, and
/// how many of those parsed cleanly via `cardano_assets::AssetMetadata`.
/// Dumps the first asset's parsed form so we can sanity-check the mapping.
async fn cmd_source_probe_maestro(policy_id: &str) -> anyhow::Result<()> {
    use cardano_assets::{Asset, AssetMetadata};
    use viewer_ingest::MaestroClient;

    println!("Probing Maestro for: {}", policy_id);

    let client = MaestroClient::from_env()?;

    // Dump the raw first asset to surface field names we may have missed.
    let raw = client.fetch_policy_assets_page_raw(policy_id).await?;
    if let Some(first) = raw.get("data").and_then(|d| d.as_array()).and_then(|a| a.first()) {
        println!("\nRaw first asset (top-level keys):");
        if let Some(obj) = first.as_object() {
            for k in obj.keys() {
                println!("  {}", k);
            }
        }
        println!("\nRaw first asset JSON:\n{}", serde_json::to_string_pretty(first)?);
    }

    let page = client.fetch_policy_assets_page(policy_id, None).await?;

    let total = page.data.len();
    let mut with_cip25 = 0usize;
    let mut with_cip68 = 0usize;
    let mut parsed_ok = 0usize;
    let mut parse_failures: Vec<(String, String)> = Vec::new();
    let mut first_sample: Option<Asset> = None;

    for asset in &page.data {
        let Some(standards) = asset.asset_standards.as_ref() else {
            continue;
        };

        // Prefer CIP-25 if present, otherwise unwrap the CIP-68 envelope and
        // parse the inner metadata (which follows the same shapes).
        let metadata_value = if let Some(cip25) = standards.cip25_metadata.as_ref() {
            with_cip25 += 1;
            Some(cip25.clone())
        } else if let Some(envelope) = standards.cip68_metadata.as_ref() {
            with_cip68 += 1;
            Some(envelope.metadata.clone())
        } else {
            None
        };

        let Some(value) = metadata_value else { continue };

        match serde_json::from_value::<AssetMetadata>(value) {
            Ok(metadata) => {
                parsed_ok += 1;
                if first_sample.is_none() {
                    first_sample = Some(Asset::from(metadata));
                }
            }
            Err(e) => {
                if parse_failures.len() < 5 {
                    parse_failures.push((asset.asset_name.clone(), e.to_string()));
                }
            }
        }
    }

    println!("\nPage summary:");
    println!("  Assets in page:       {}", total);
    println!("  With cip25_metadata:  {}", with_cip25);
    println!("  With cip68_metadata:  {}", with_cip68);
    println!("  Parsed via AssetMetadata: {}", parsed_ok);
    println!(
        "  Parse failures:       {}",
        (with_cip25 + with_cip68).saturating_sub(parsed_ok)
    );
    println!(
        "  next_cursor:          {}",
        page.next_cursor.as_deref().unwrap_or("(none)")
    );

    if let Some(sample) = first_sample {
        let trait_count = sample.traits.inner().len();
        println!("\nFirst parsed asset:");
        println!("  name:   {}", sample.name);
        println!("  image:  {}", sample.image);
        println!("  media:  {:?}", sample.media_type);
        println!("  traits: {} keys", trait_count);
        for (k, vs) in sample.traits.iter().take(5) {
            println!("    {} = {:?}", k, vs);
        }
        if trait_count > 5 {
            println!("    ... and {} more", trait_count - 5);
        }
    }

    if !parse_failures.is_empty() {
        println!("\nFirst {} parse failures:", parse_failures.len());
        for (name, err) in &parse_failures {
            println!("  {}: {}", name, err);
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
