//! Client for the collection-ownership Policy CID Index —
//! `GET https://ownership.cnft.dev/api/cids/{policy_id}`.
//!
//! The endpoint resolves every IPFS CID a policy's assets reference, from
//! on-chain CIP-25 / CIP-68 metadata, server-side. This replaces the older
//! cnft.tools / Maestro fetch-and-extract path for pinning.
//!
//! Wire types come from `platform_types::policy_cid_index` — the canonical
//! definitions shared with the producing service.

use std::time::Duration;

use platform_types::policy_cid_index::CidIndexResponse;
pub use platform_types::policy_cid_index::CidIndexStatus;
use reqwest::Client;
use thiserror::Error;
use tracing::debug;

const DEFAULT_BASE_URL: &str = "https://ownership.cnft.dev";

#[derive(Debug, Error)]
pub enum CidIndexError {
    #[error("CID index request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("CID index API error {status}: {body}")]
    Api { status: u16, body: String },
}

/// Fully-paged CID index for a policy.
#[derive(Debug, Clone)]
pub struct PolicyCidIndex {
    pub status: CidIndexStatus,
    pub cid_generation: u64,
    pub cids: Vec<String>,
}

/// Client for the collection-ownership CID index service.
pub struct CidIndexClient {
    client: Client,
    base_url: String,
}

impl Default for CidIndexClient {
    fn default() -> Self {
        Self::new()
    }
}

impl CidIndexClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Fetch a single page of the CID index.
    async fn fetch_page(
        &self,
        policy_id: &str,
        cursor: Option<&str>,
    ) -> Result<CidIndexResponse, CidIndexError> {
        let mut url = format!("{}/api/cids/{}", self.base_url, policy_id);
        if let Some(c) = cursor {
            url.push_str("?cursor=");
            url.push_str(c);
        }

        debug!(url, "Fetching CID index page");

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(CidIndexError::Api { status, body });
        }

        response.json().await.map_err(CidIndexError::Request)
    }

    /// Fetch the full CID index for a policy, paging until exhausted.
    ///
    /// The `status` of the *last* page is returned — a not-`Complete` status
    /// means the returned CID set may be incomplete.
    pub async fn fetch_all(&self, policy_id: &str) -> Result<PolicyCidIndex, CidIndexError> {
        let mut cids = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let page = self.fetch_page(policy_id, cursor.as_deref()).await?;
            let status = page.status;
            let cid_generation = page.cid_generation;
            cids.extend(page.cids);

            match page.next_cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => {
                    return Ok(PolicyCidIndex {
                        status,
                        cid_generation,
                        cids,
                    });
                }
            }
        }
    }
}
