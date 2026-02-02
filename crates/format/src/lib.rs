//! Core format types for NFT viewer.
//!
//! This crate defines the data structures used for NFT collection metadata,
//! designed to be lightweight and efficient for viewing large collections.
//!
//! # Example
//!
//! ```
//! use viewer_format::{AssetDetails, CollectionInfo, CollectionSource, TokenDetails};
//!
//! // Asset details are typically loaded from asset_details.json
//! let json = r#"{
//!     "collection": {
//!         "name": "My Collection",
//!         "total_tokens": 1000,
//!         "source": { "type": "live", "policy_id": "abc123", "synced_at": "2025-01-28T00:00:00Z" },
//!         "created_at": "2025-01-28T00:00:00Z"
//!     },
//!     "tokens": [],
//!     "trait_summary": {}
//! }"#;
//!
//! let details: AssetDetails = serde_json::from_str(json).unwrap();
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Combined asset details for a collection.
///
/// This is the primary data structure loaded by the viewer, containing
/// all metadata needed to display and filter a collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetDetails {
    /// Collection-level metadata
    pub collection: CollectionInfo,
    /// Per-token details with traits and rarity
    pub tokens: Vec<TokenDetails>,
    /// Trait distribution summary for filtering UI
    pub trait_summary: BTreeMap<String, TraitSummary>,
}

/// Collection-level information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionInfo {
    /// Collection name for display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Total number of tokens in the collection
    pub total_tokens: usize,
    /// Where the collection data comes from
    pub source: CollectionSource,
    /// When this bundle was created (ISO 8601)
    pub created_at: String,
}

/// Where the collection data originates from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CollectionSource {
    /// Generated preview collection (not yet minted)
    Preview {
        /// Generation seed for reproducibility
        seed: u64,
        /// Distribution mode used during generation
        distribution: Distribution,
    },
    /// Live on-chain collection synced from an indexer
    Live {
        /// Policy ID of the collection
        policy_id: String,
        /// When the collection was last synced (ISO 8601)
        synced_at: String,
    },
}

/// Distribution mode for trait value selection during generation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Distribution {
    /// Weighted random selection based on configured weights
    #[default]
    Weighted,
    /// Even distribution across all values
    Even,
}

impl std::fmt::Display for Distribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Distribution::Weighted => write!(f, "weighted"),
            Distribution::Even => write!(f, "even"),
        }
    }
}

/// Details for a single token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenDetails {
    /// Primary identifier.
    ///
    /// For preview collections: zero-padded numeric ID (e.g., "000001")
    /// For live collections: encoded asset name (e.g., "SquashuaChicken0001")
    pub id: String,
    /// Display name (e.g., "Squashua Chicken #1")
    pub name: String,
    /// Token attributes (trait_name -> value or array of values)
    pub attributes: BTreeMap<String, AttributeValue>,
    /// Rarity information
    pub rarity: TokenRarity,
}

/// Attribute value that can be single or multi-valued.
///
/// Most traits have a single value, but some (like layered clothing)
/// can have multiple values that are displayed as an array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    /// Single value (most common)
    Single(String),
    /// Multiple values (for grouped/layered traits)
    Multiple(Vec<String>),
}

impl AttributeValue {
    /// Create from a single optional string.
    pub fn from_option(value: Option<String>) -> Option<Self> {
        value.map(AttributeValue::Single)
    }

    /// Create from multiple optional strings, filtering out None values.
    ///
    /// Returns `Multiple` variant for consistency in grouped traits.
    pub fn from_options(values: Vec<Option<String>>) -> Option<Self> {
        let filtered: Vec<String> = values.into_iter().flatten().collect();
        if filtered.is_empty() {
            None
        } else {
            Some(AttributeValue::Multiple(filtered))
        }
    }
}

/// Rarity metrics for a single token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenRarity {
    /// Rarity score (sum of inverse frequencies) - higher = rarer
    pub score: f64,
    /// Rank among all tokens (1 = rarest)
    pub rank: usize,
    /// Percentile (0-100, higher = rarer)
    pub percentile: f64,
}

/// Summary statistics for a single trait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitSummary {
    /// Distribution of values for this trait
    pub values: BTreeMap<String, ValueStats>,
}

/// Statistics for a single trait value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueStats {
    /// Number of tokens with this value
    pub count: usize,
    /// Percentage of tokens with this value (0-100)
    pub percentage: f64,
}

// ============================================================================
// Ingestion Config
// ============================================================================

/// Per-collection configuration for ingestion.
///
/// Stored in `configs/{chain}/{collection_id}.toml`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IngestionConfig {
    /// URL-friendly identifier for the collection (e.g., "blackflag")
    ///
    /// Used for routes like `blackflag.viewer.hodlcroft.com` or
    /// `viewer.hodlcroft.com/blackflag`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,

    /// Override display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Image source configuration
    #[serde(default)]
    pub images: ImageSourceConfig,

    /// Trait configuration
    #[serde(default)]
    pub traits: TraitConfig,

    /// Rarity calculation overrides
    #[serde(default)]
    pub rarity: RarityConfig,

    /// Pinata configuration for pinning and serving images
    #[serde(default)]
    pub pinata: PinataConfig,
}

/// Pinata configuration for image pinning and serving.
///
/// When enabled, the sync process will:
/// 1. Pin all collection CIDs to the specified group
/// 2. Fetch thumbnails via Pinata's image optimization
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PinataConfig {
    /// Whether Pinata integration is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Group ID for this collection's images.
    /// Create the group in Pinata's UI and copy the ID here.
    #[serde(default)]
    pub group_id: Option<String>,

    /// Thumbnail size in pixels (default: 256)
    #[serde(default = "default_thumbnail_size")]
    pub thumbnail_size: u32,
}

fn default_thumbnail_size() -> u32 {
    256
}

impl PinataConfig {
    /// Check if Pinata is enabled and properly configured.
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.group_id.is_some()
    }
}

/// Image source configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSourceConfig {
    /// Image source type: "ipfs" (default) or "iiif"
    #[serde(default = "default_image_source")]
    pub source: String,

    /// IIIF base URL (required if source = "iiif")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iiif_base_url: Option<String>,

    /// IIIF max size parameter (default: 1686)
    #[serde(default = "default_iiif_size")]
    pub iiif_size: u32,

    /// Custom IPFS gateways for this collection.
    ///
    /// Use shortcodes: "blockfrost", "pinata", "dweb", "ipfs_io"
    ///
    /// If not specified, uses the default gateways (Blockfrost + Pinata).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gateways: Vec<IpfsGateway>,
}

/// Known IPFS gateways that can be configured per collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IpfsGateway {
    /// Blockfrost IPFS gateway (requires CIDv1 conversion)
    Blockfrost,
    /// Pinata public gateway
    Pinata,
    /// dweb.link gateway (Protocol Labs)
    Dweb,
    /// ipfs.io gateway
    #[serde(rename = "ipfs_io")]
    IpfsIo,
}

fn default_image_source() -> String {
    "ipfs".to_string()
}

fn default_iiif_size() -> u32 {
    1686
}

impl Default for ImageSourceConfig {
    fn default() -> Self {
        Self {
            source: default_image_source(),
            iiif_base_url: None,
            iiif_size: default_iiif_size(),
            gateways: Vec::new(),
        }
    }
}

impl ImageSourceConfig {
    /// Build a IIIF image URL for a Cardano asset.
    pub fn iiif_url(&self, policy_id: &str, encoded_name: &str) -> Option<String> {
        let base = self.iiif_base_url.as_ref()?;
        Some(format!(
            "{}/iiif/3/{}:{}/full/{},/0/default.jpg",
            base.trim_end_matches('/'),
            policy_id,
            encoded_name,
            self.iiif_size
        ))
    }

    /// Check if using IIIF source.
    pub fn is_iiif(&self) -> bool {
        self.source == "iiif"
    }

    /// Check if using NFTCDN source.
    pub fn is_nftcdn(&self) -> bool {
        self.source == "nftcdn"
    }
}

/// Trait processing configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraitConfig {
    /// Traits to exclude from filtering/indexing.
    ///
    /// Use for high-cardinality traits like unique identifiers.
    /// These traits are still stored in token details but not indexed.
    #[serde(default)]
    pub ignore: Vec<String>,

    /// Trait name aliases for display normalization.
    ///
    /// Maps original trait names to display names.
    #[serde(default)]
    pub aliases: std::collections::HashMap<String, String>,
}

/// Rarity calculation configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RarityConfig {
    /// Traits to exclude from rarity score calculation.
    #[serde(default)]
    pub exclude: Vec<String>,

    /// Hide rarity rankings in the viewer UI.
    #[serde(default)]
    pub hide: bool,
}

impl IngestionConfig {
    /// Check if a trait should be ignored for indexing.
    pub fn should_ignore_trait(&self, trait_name: &str) -> bool {
        self.traits.ignore.iter().any(|t| t == trait_name)
    }

    /// Get the display name for a trait, applying aliases.
    pub fn display_name<'a>(&'a self, trait_name: &'a str) -> &'a str {
        self.traits
            .aliases
            .get(trait_name)
            .map(|s| s.as_str())
            .unwrap_or(trait_name)
    }

    /// Check if a trait should be excluded from rarity calculation.
    pub fn exclude_from_rarity(&self, trait_name: &str) -> bool {
        self.rarity.exclude.iter().any(|t| t == trait_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_live_collection() {
        let json = r#"{
            "collection": {
                "name": "Test Collection",
                "total_tokens": 100,
                "source": {
                    "type": "live",
                    "policy_id": "abc123def456",
                    "synced_at": "2025-01-28T12:00:00Z"
                },
                "created_at": "2025-01-28T12:00:00Z"
            },
            "tokens": [],
            "trait_summary": {}
        }"#;

        let details: AssetDetails = serde_json::from_str(json).unwrap();
        assert_eq!(details.collection.name, Some("Test Collection".to_string()));
        assert!(matches!(
            details.collection.source,
            CollectionSource::Live { .. }
        ));
    }

    #[test]
    fn test_deserialize_preview_collection() {
        let json = r#"{
            "collection": {
                "total_tokens": 1000,
                "source": {
                    "type": "preview",
                    "seed": 12345,
                    "distribution": "weighted"
                },
                "created_at": "2025-01-28T12:00:00Z"
            },
            "tokens": [],
            "trait_summary": {}
        }"#;

        let details: AssetDetails = serde_json::from_str(json).unwrap();
        assert!(matches!(
            details.collection.source,
            CollectionSource::Preview { seed: 12345, .. }
        ));
    }

    #[test]
    fn test_attribute_value_single() {
        let json = r#""Blue""#;
        let value: AttributeValue = serde_json::from_str(json).unwrap();
        assert_eq!(value, AttributeValue::Single("Blue".to_string()));
    }

    #[test]
    fn test_attribute_value_multiple() {
        let json = r#"["Tee", "Hoodie"]"#;
        let value: AttributeValue = serde_json::from_str(json).unwrap();
        assert_eq!(
            value,
            AttributeValue::Multiple(vec!["Tee".to_string(), "Hoodie".to_string()])
        );
    }

    #[test]
    fn test_ingestion_config() {
        let mut config = IngestionConfig::default();
        config.traits.ignore = vec!["Call Sign".to_string(), "Serial".to_string()];
        config
            .traits
            .aliases
            .insert("BG".to_string(), "Background".to_string());

        assert!(config.should_ignore_trait("Call Sign"));
        assert!(config.should_ignore_trait("Serial"));
        assert!(!config.should_ignore_trait("Background"));

        assert_eq!(config.display_name("BG"), "Background");
        assert_eq!(config.display_name("Eyes"), "Eyes");
    }
}
