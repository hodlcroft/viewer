use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "viewer")]
#[command(about = "CLI for syncing and managing NFT collection bundles")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Sync a collection from chain by policy ID
    Sync {
        /// Policy ID of the collection
        policy_id: String,
        /// Output directory for the bundle
        #[arg(short, long, default_value = "./bundle")]
        output: String,
    },
    /// Verify a bundle's integrity
    Verify {
        /// Path to the bundle directory
        path: String,
    },
    /// Show bundle info
    Info {
        /// Path to the bundle directory
        path: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Sync { policy_id, output } => {
            println!("Syncing collection {} to {}", policy_id, output);
            // TODO: Implement sync from cnft.tools
            todo!("Sync not yet implemented")
        }
        Commands::Verify { path } => {
            println!("Verifying bundle at {}", path);
            // TODO: Implement verification
            todo!("Verify not yet implemented")
        }
        Commands::Info { path } => {
            println!("Bundle info for {}", path);
            let index = viewer_bundle::BundleIndex::read_from_file(&std::path::Path::new(&path).join("index.json"))?;
            println!("  Format version: {}", index.version);
            println!("  Image format: {}", index.image_format);
            println!("  Image count: {}", index.image_count);
            println!("  Shard count: {}", index.shard_count);
            if let Some(sprites) = &index.sprites {
                println!("  Sprites: {}x{} grid, {} sheets", sprites.columns, sprites.rows, sprites.sheet_count);
            }
            Ok(())
        }
    }
}
