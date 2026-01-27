//! Ingestion pipeline for NFT collections.
//!
//! This crate provides the tools to:
//! - Fetch collection data from sources (CNFT.tools, Maestro)
//! - Fetch images from IPFS with gateway racing and rate limiting
//! - Analyze traits and compute rarity
//! - Generate sprites and HCF bundles
//! - Pack everything into the binary format

pub mod analyze;
pub mod ipfs;
pub mod source;

pub use analyze::TraitAnalysis;
pub use ipfs::IpfsFetcher;
pub use source::{AssetSource, CnftToolsSource, NormalizedAsset};
