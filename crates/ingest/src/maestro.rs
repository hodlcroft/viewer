//! Minimal Maestro Cardano API client + `AssetSource` impl.
//!
//! Used as a fallback source when cnft.tools doesn't index a collection.
//! Only implements the policy assets endpoint with inline CIP-25/CIP-68
//! metadata — enough for the Filebase pinning experiment.

use std::time::Duration;

use async_trait::async_trait;
use cardano_assets::{Asset, AssetMetadata};
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use tracing::{debug, info};

use crate::source::{AssetSource, NormalizedAsset};

const DEFAULT_BASE_URL: &str = "https://mainnet.gomaestro-api.org/v1";
const PAGE_SIZE: usize = 100;

#[derive(Debug, Error)]
pub enum MaestroError {
    #[error("Missing MAESTRO_API_KEY environment variable")]
    MissingApiKey,

    #[error("Maestro request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Maestro API error {status}: {body}")]
    Api { status: u16, body: String },
}

/// Lightweight Maestro client. One method per endpoint we need.
pub struct MaestroClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl MaestroClient {
    pub fn from_env() -> Result<Self, MaestroError> {
        let api_key = std::env::var("MAESTRO_API_KEY").map_err(|_| MaestroError::MissingApiKey)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Ok(Self {
            client,
            api_key,
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    /// Fetch a single page of assets for a policy.
    ///
    /// `cursor` is `None` for the first page. Returns the page plus the
    /// `next_cursor` (None when exhausted).
    pub async fn fetch_policy_assets_page(
        &self,
        policy_id: &str,
        cursor: Option<&str>,
    ) -> Result<PolicyAssetsResponse, MaestroError> {
        let mut url = format!(
            "{}/policy/{}/assets?count={}",
            self.base_url, policy_id, PAGE_SIZE
        );
        if let Some(c) = cursor {
            url.push_str("&cursor=");
            url.push_str(c);
        }

        debug!(url, "Fetching Maestro policy assets page");

        let response = self
            .client
            .get(&url)
            .header("api-key", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(MaestroError::Api { status, body });
        }

        response.json().await.map_err(MaestroError::Request)
    }

    /// Return the raw first-page JSON body for a policy — used during
    /// development to inspect Maestro's response shape.
    pub async fn fetch_policy_assets_page_raw(
        &self,
        policy_id: &str,
    ) -> Result<serde_json::Value, MaestroError> {
        let url = format!(
            "{}/policy/{}/assets?count={}",
            self.base_url, policy_id, PAGE_SIZE
        );

        let response = self
            .client
            .get(&url)
            .header("api-key", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(MaestroError::Api { status, body });
        }

        response.json().await.map_err(MaestroError::Request)
    }

    /// Page every asset in a policy, run each parseable CIP-25/CIP-68
    /// metadata through `AssetMetadata::extract_cids`, and return the
    /// CIDv1-normalised deduped set — the same shape `ownership.cnft.dev`
    /// produces, but driven locally so the patched `cardano-assets` is
    /// used without needing the worker to redeploy.
    pub async fn fetch_policy_cids(&self, policy_id: &str) -> Result<Vec<String>, MaestroError> {
        use cardano_assets::AssetMetadata;
        use std::collections::HashSet;

        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let page = self
                .fetch_policy_assets_page(policy_id, cursor.as_deref())
                .await?;

            for asset in page.data {
                let Some(standards) = asset.asset_standards else {
                    continue;
                };
                let metadata_value = if let Some(cip25) = standards.cip25_metadata {
                    cip25
                } else if let Some(env) = standards.cip68_metadata {
                    env.metadata
                } else {
                    continue;
                };
                let Ok(metadata) = serde_json::from_value::<AssetMetadata>(metadata_value) else {
                    continue;
                };
                for extracted in metadata.extract_cids() {
                    if seen.insert(extracted.cid.clone()) {
                        out.push(extracted.cid);
                    }
                }
            }

            match page.next_cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => break,
            }
        }

        out.sort();
        Ok(out)
    }
}

/// Response shape for `/policy/{id}/assets`.
#[derive(Debug, Deserialize)]
pub struct PolicyAssetsResponse {
    pub data: Vec<MaestroAsset>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// One asset row from Maestro's policy assets endpoint.
///
/// Field set is the subset we actually need — Maestro returns more (supply,
/// fingerprint, mint/burn counts, etc.) which we discard.
#[derive(Debug, Deserialize)]
pub struct MaestroAsset {
    /// Hex-encoded asset name.
    pub asset_name: String,
    /// ASCII-decoded asset name (best-effort; may be empty if the bytes
    /// aren't valid ASCII).
    #[serde(default)]
    pub asset_name_ascii: Option<String>,
    #[serde(default)]
    pub asset_standards: Option<AssetStandards>,
}

/// Inline metadata payload as returned by Maestro. Collections may use
/// CIP-25 or CIP-68 — the CIP-68 variant wraps the metadata in `{purpose,
/// version, metadata}`, with the inner shape matching CIP-25 conventions.
#[derive(Debug, Deserialize)]
pub struct AssetStandards {
    #[serde(default)]
    pub cip25_metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub cip68_metadata: Option<Cip68Envelope>,
}

/// CIP-68 envelope: the actual asset metadata sits under `.metadata`.
#[derive(Debug, Deserialize)]
pub struct Cip68Envelope {
    pub purpose: String,
    pub version: u32,
    pub metadata: serde_json::Value,
}

/// AssetSource implementation backed by Maestro.
///
/// Pages through every asset in a policy and feeds the inline metadata
/// through `cardano_assets::AssetMetadata` to build `NormalizedAsset`s.
/// Assets with no parseable metadata (e.g. CIP-68 user NFTs that delegate
/// to a reference NFT) are silently skipped — the dedup-by-CID at the call
/// site collapses any remaining duplication.
pub struct MaestroSource {
    client: MaestroClient,
}

impl MaestroSource {
    pub fn from_env() -> Result<Self, MaestroError> {
        Ok(Self {
            client: MaestroClient::from_env()?,
        })
    }
}

#[async_trait]
impl AssetSource for MaestroSource {
    async fn fetch_collection(&self, policy_id: &str) -> anyhow::Result<Vec<NormalizedAsset>> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        let mut fetched = 0usize;
        let mut skipped_no_metadata = 0usize;
        let mut skipped_parse_failure = 0usize;

        loop {
            let page = self
                .client
                .fetch_policy_assets_page(policy_id, cursor.as_deref())
                .await?;
            pages += 1;
            fetched += page.data.len();

            for asset in page.data {
                match to_normalized(asset) {
                    Ok(Some(n)) => out.push(n),
                    Ok(None) => skipped_no_metadata += 1,
                    Err(_) => skipped_parse_failure += 1,
                }
            }

            info!(pages, fetched, parsed = out.len(), "Maestro page consumed");

            match page.next_cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => break,
            }
        }

        info!(
            pages,
            fetched,
            parsed = out.len(),
            skipped_no_metadata,
            skipped_parse_failure,
            "Maestro fetch complete"
        );

        Ok(out)
    }
}

/// Convert a Maestro asset into a `NormalizedAsset` by deserializing its
/// inline CIP-25 or CIP-68 metadata. Returns `Ok(None)` for assets that
/// have no usable metadata (e.g. CIP-68 user NFTs).
fn to_normalized(asset: MaestroAsset) -> Result<Option<NormalizedAsset>, serde_json::Error> {
    let Some(standards) = asset.asset_standards else {
        return Ok(None);
    };

    let metadata_value = if let Some(cip25) = standards.cip25_metadata {
        cip25
    } else if let Some(envelope) = standards.cip68_metadata {
        envelope.metadata
    } else {
        return Ok(None);
    };

    let parsed: AssetMetadata = serde_json::from_value(metadata_value)?;
    let asset_norm: Asset = parsed.into();

    let image_url = if asset_norm.image.is_empty() {
        None
    } else {
        Some(asset_norm.image)
    };

    Ok(Some(NormalizedAsset {
        encoded_name: asset.asset_name,
        display_name: asset_norm.name,
        traits: asset_norm.traits.inner().clone(),
        rarity_rank: asset_norm.rarity_rank,
        image_url,
    }))
}
