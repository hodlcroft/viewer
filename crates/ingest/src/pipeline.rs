//! Ingestion pipeline orchestration.
//!
//! Directory structure in `.build/{policy_id}/`:
//! ```text
//! .build/
//!   {policy_id}/
//!     raw/              # Original images from IPFS (various formats)
//!       {encoded_name}.{ext}
//!     images/           # Normalized WebP images (max 2048px)
//!       {encoded_name}.webp
//!     sprites/          # Generated sprite sheets (from raw, faster)
//!       sprites_000.webp
//!       sprites_001.webp
//!     hcf/              # HCF bundle shards (from normalized images)
//!       images_000.hcf
//!       images_001.hcf
//!     collection.bin    # Final binary format
//!     metadata.json     # Pipeline state/progress
//! ```

use std::path::{Path, PathBuf};
use tracing::debug;

/// Pipeline configuration.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Root build directory (default: .build)
    pub build_dir: PathBuf,
    /// IPFS fetch concurrency
    pub fetch_concurrency: usize,
    /// Max sprite sheet dimension (default 2048 for GPU compatibility)
    pub sprite_max_sheet_size: u32,
    /// HCF shard size in bytes
    pub hcf_shard_size: usize,
    /// Max image dimension for HCF
    pub hcf_max_dimension: u32,
    /// WebP quality (0-100)
    pub webp_quality: u8,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            build_dir: PathBuf::from(".build"),
            fetch_concurrency: 5,
            sprite_max_sheet_size: 1024, // GPU-friendly 4x4 grid, auto-detect cell size from first image
            hcf_shard_size: 250 * 1024 * 1024, // 250 MB
            hcf_max_dimension: 2048,
            webp_quality: 85,
        }
    }
}

/// Working directories for a collection.
#[derive(Debug, Clone)]
pub struct CollectionDirs {
    /// Root directory for this collection
    pub root: PathBuf,
    /// Raw downloaded images (original format from IPFS)
    pub raw: PathBuf,
    /// Normalized WebP images (max 2048px, for HCF bundles)
    pub images: PathBuf,
    /// Generated sprite sheets
    pub sprites: PathBuf,
    /// HCF bundle shards
    pub hcf: PathBuf,
}

impl CollectionDirs {
    /// Create directory structure for a collection.
    pub fn create(build_dir: &Path, policy_id: &str) -> std::io::Result<Self> {
        let root = build_dir.join(policy_id);
        let raw = root.join("raw");
        let images = root.join("images");
        let sprites = root.join("sprites");
        let hcf = root.join("hcf");

        std::fs::create_dir_all(&raw)?;
        std::fs::create_dir_all(&images)?;
        std::fs::create_dir_all(&sprites)?;
        std::fs::create_dir_all(&hcf)?;

        Ok(Self {
            root,
            raw,
            images,
            sprites,
            hcf,
        })
    }

    /// Path to the final collection.bin file.
    pub fn collection_bin(&self) -> PathBuf {
        self.root.join("collection.bin")
    }

    /// Path to pipeline metadata/state file.
    pub fn metadata(&self) -> PathBuf {
        self.root.join("metadata.json")
    }

    /// Path to build log file.
    pub fn build_log(&self) -> PathBuf {
        self.root.join("build.log")
    }

    /// Path for a raw downloaded image.
    pub fn raw_path(&self, encoded_name: &str, ext: &str) -> PathBuf {
        self.raw.join(format!("{}.{}", encoded_name, ext))
    }

    /// Path for a normalized WebP image.
    pub fn image_path(&self, encoded_name: &str) -> PathBuf {
        self.images.join(format!("{}.webp", encoded_name))
    }

    /// Path for a sprite sheet.
    pub fn sprite_path(&self, index: u32) -> PathBuf {
        self.sprites.join(format!("sprites_{:03}.webp", index))
    }

    /// Path for an HCF shard.
    pub fn hcf_path(&self, index: u32) -> PathBuf {
        self.hcf.join(format!("images_{:03}.hcf", index))
    }
}

/// Pipeline state tracking for resumability.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PipelineState {
    /// Policy ID being processed
    pub policy_id: String,
    /// Total number of assets
    pub total_assets: usize,
    /// Number of images fetched
    pub images_fetched: usize,
    /// Number of images failed
    pub images_failed: usize,
    /// Sprite generation complete
    pub sprites_complete: bool,
    /// HCF bundling complete
    pub hcf_complete: bool,
    /// Binary format complete
    pub binary_complete: bool,
}

impl PipelineState {
    pub fn new(policy_id: &str, total_assets: usize) -> Self {
        Self {
            policy_id: policy_id.to_string(),
            total_assets,
            images_fetched: 0,
            images_failed: 0,
            sprites_complete: false,
            hcf_complete: false,
            binary_complete: false,
        }
    }

    /// Load state from file, or create new if not exists.
    pub fn load_or_create(path: &Path, policy_id: &str, total_assets: usize) -> Self {
        if path.exists()
            && let Ok(content) = std::fs::read_to_string(path)
                && let Ok(state) = serde_json::from_str::<Self>(&content) {
                    debug!(policy_id = %state.policy_id, "Resuming from saved state");
                    return state;
                }
        Self::new(policy_id, total_assets)
    }

    /// Save state to file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)
    }
}

/// Main pipeline orchestrator.
pub struct Pipeline {
    pub config: PipelineConfig,
    pub dirs: CollectionDirs,
    pub state: PipelineState,
}

impl Pipeline {
    /// Initialize pipeline for a collection.
    pub fn new(
        policy_id: &str,
        total_assets: usize,
        config: PipelineConfig,
    ) -> std::io::Result<Self> {
        let dirs = CollectionDirs::create(&config.build_dir, policy_id)?;
        let state = PipelineState::load_or_create(&dirs.metadata(), policy_id, total_assets);

        Ok(Self {
            config,
            dirs,
            state,
        })
    }

    /// Save current state.
    pub fn save_state(&self) -> std::io::Result<()> {
        self.state.save(&self.dirs.metadata())
    }

    /// Check if a raw image has already been downloaded.
    pub fn raw_exists(&self, encoded_name: &str) -> Option<PathBuf> {
        for ext in &["png", "jpg", "jpeg", "gif", "webp"] {
            let path = self.dirs.raw_path(encoded_name, ext);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Check if a normalized image exists.
    pub fn image_exists(&self, encoded_name: &str) -> bool {
        self.dirs.image_path(encoded_name).exists()
    }
}
