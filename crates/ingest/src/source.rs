//! Data sources for NFT collections.

use std::collections::HashMap;

/// Normalized asset representation from any source.
#[derive(Debug, Clone)]
pub struct NormalizedAsset {
    /// Hex-encoded asset name (lookup key)
    pub encoded_name: String,
    /// Human-readable display name
    pub display_name: String,
    /// Trait name -> values mapping
    pub traits: HashMap<String, Vec<String>>,
    /// Rarity rank from source (if available)
    pub rarity_rank: Option<u32>,
    /// Image URL (IPFS or HTTP)
    pub image_url: Option<String>,
}

/// Trait for collection data sources.
#[async_trait::async_trait]
pub trait AssetSource: Send + Sync {
    /// Fetch all assets for a policy ID.
    async fn fetch_collection(&self, policy_id: &str) -> anyhow::Result<Vec<NormalizedAsset>>;
}

/// CNFT.tools data source.
pub struct CnftToolsSource;

impl CnftToolsSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CnftToolsSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AssetSource for CnftToolsSource {
    async fn fetch_collection(&self, policy_id: &str) -> anyhow::Result<Vec<NormalizedAsset>> {
        let api = cnft_tools::CnftApi::default();
        let assets = api.get_for_policy(policy_id).await?;

        Ok(assets
            .into_iter()
            .map(|a| NormalizedAsset {
                encoded_name: a.encoded_name,
                display_name: a.name,
                traits: a.traits,
                rarity_rank: Some(a.rarity_rank),
                image_url: a.icon_url,
            })
            .collect())
    }
}
