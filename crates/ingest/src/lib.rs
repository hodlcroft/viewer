//! Ingestion pipeline for NFT collections.
//!
//! This crate provides the tools to:
//! - Fetch collection data from sources (CNFT.tools, Maestro)
//! - Fetch images from IPFS with gateway racing and rate limiting
//! - Analyze traits and compute rarity
//! - Normalize images (resize, convert to WebP)
//! - Generate sprites and HCF bundles
//! - Pack everything into the binary format

pub mod analyze;
pub mod bundle;
pub mod cid_compute;
pub mod cid_index;
pub mod fetch;
pub mod filebase;
pub mod filebase_s3;
pub mod ipfs;
pub mod maestro;
pub mod nftcdn;
pub mod normalize;
pub mod pinata;
pub mod pipeline;
pub mod source;
pub mod sprites;
pub mod writer;

pub use analyze::TraitAnalysis;
pub use bundle::{HcfBundleResult, HcfBundler, HcfConfig, HcfError, ImageLocation, ShardInfo};
pub use cid_compute::{
    CidInfo, ComputedCid, compute_cid, compute_cid_bytes, find_matching_cid, parse_cid_info,
};
pub use cid_index::{CidIndexClient, CidIndexError, CidIndexStatus, PolicyCidIndex};
pub use fetch::{
    FetchResult, fetch_images, fetch_images_iiif, fetch_images_nftcdn, fetch_thumbnails_pinata,
};
pub use ipfs::{IpfsFetcher, extract_cid, to_cidv0, to_cidv1};
pub use maestro::{
    Cip68Envelope, MaestroAsset, MaestroClient, MaestroError, MaestroSource, PolicyAssetsResponse,
};
pub use nftcdn::{NftcdnClient, NftcdnError};
pub use normalize::{
    BatchNormalizer, NormalizeConfig, NormalizeError, NormalizeResult, normalize_image,
};
pub use filebase::{
    FilebaseClient, FilebaseError, PinItem, PinRecord, PinResult, PinStatusCounts,
};
pub use filebase_s3::{FilebaseS3Client, FilebaseS3Error, PutResult as S3PutResult};
pub use pinata::{PinataClient, PinataError};
pub use pipeline::{CollectionDirs, Pipeline, PipelineConfig, PipelineState};
pub use source::{AssetSource, CnftToolsSource, NormalizedAsset};
pub use sprites::{SpriteConfig, SpriteError, SpriteGenerator, SpriteLocation, SpriteSheet};
pub use writer::{CollectionWriter, TokenData, WriterError};
