//! Client for `assets-resolve`'s collection listing —
//! `GET https://assets-resolve.hodlcroft.com/list?policy={policy_id}&format=ndjson`.
//!
//! The listing is every standing asset for a policy, with its image URI
//! exactly as the chain stated it. It replaces the Maestro fetch-and-extract
//! path for mapping Filebase CIDs back to asset names: the index holds the
//! metadata, so no on-chain round trip is needed.
//!
//! Wire types mirror `api_types::asset_listing` in `cnft.dev-workers`. The
//! NDJSON response is a header line followed by one line per asset; a paged
//! response carries the opaque `next` cursor on the header.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use tracing::debug;

const DEFAULT_BASE_URL: &str = "https://assets-resolve.hodlcroft.com";

#[derive(Debug, Error)]
pub enum AssetListError {
    #[error("asset listing request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("asset listing API error {status} ({code}): {message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },

    #[error("asset listing response is empty")]
    Empty,

    #[error("malformed asset listing at line {line}: {source}")]
    Decode {
        line: usize,
        #[source]
        source: serde_json::Error,
    },

    #[error("asset listing cursor did not advance")]
    CursorStalled,
}

/// A fully-paged collection listing.
#[derive(Debug, Clone)]
pub struct AssetListing {
    /// The pointer's `built_through` when the listing was written.
    pub as_of_chunk: u16,
    /// The listing's full entry count, not the number of lines fetched.
    pub total: u32,
    pub assets: Vec<ListedAsset>,
}

/// One standing asset, as stated by the listing.
#[derive(Debug, Clone, Deserialize)]
pub struct ListedAsset {
    /// The asset name, hex — the exact bytes as minted.
    pub name_hex: String,
    /// The name rendered for display.
    pub name: String,
    /// The image URI, exactly as the chain stated it. Empty when none.
    pub image: String,
}

/// Client for the `assets-resolve` collection listing.
pub struct AssetListClient {
    client: Client,
    base_url: String,
}

impl Default for AssetListClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetListClient {
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: base_url.into(),
        }
    }

    /// Fetch the whole listing for a policy, paging until `next` is null.
    pub async fn fetch_all(&self, policy_id: &str) -> Result<AssetListing, AssetListError> {
        let mut assets = Vec::new();
        let mut after: Option<String> = None;

        loop {
            let (header, mut page) = self.fetch_page(policy_id, after.as_deref()).await?;
            assets.append(&mut page);

            match header.next {
                Some(next) if !next.is_empty() => {
                    if after.as_deref() == Some(next.as_str()) {
                        return Err(AssetListError::CursorStalled);
                    }
                    after = Some(next);
                }
                _ => {
                    debug!(
                        total = header.total,
                        fetched = assets.len(),
                        as_of_chunk = header.as_of_chunk,
                        "asset listing fetched"
                    );
                    return Ok(AssetListing {
                        as_of_chunk: header.as_of_chunk,
                        total: header.total,
                        assets,
                    });
                }
            }
        }
    }

    async fn fetch_page(
        &self,
        policy_id: &str,
        after: Option<&str>,
    ) -> Result<(ListingHeader, Vec<ListedAsset>), AssetListError> {
        let mut params = vec![("policy", policy_id), ("format", "ndjson"), ("view", "all")];
        if let Some(after) = after {
            params.push(("after", after));
        }

        let url = format!("{}/list", self.base_url);
        debug!(url, "Fetching asset listing page");
        let response = self.client.get(&url).query(&params).send().await?;

        let status = response.status();
        if !status.is_success() {
            let status = status.as_u16();
            let body = response.text().await.unwrap_or_default();
            let (code, message) = match serde_json::from_str::<ListingErrorBody>(&body) {
                Ok(error) => (error.code, error.message),
                Err(_) => ("UNKNOWN".to_string(), body),
            };
            return Err(AssetListError::Api {
                status,
                code,
                message,
            });
        }

        let body = response.text().await?;
        parse_ndjson(&body)
    }
}

/// The first NDJSON line: what the response holds and how fresh it is.
#[derive(Debug, Deserialize)]
struct ListingHeader {
    as_of_chunk: u16,
    total: u32,
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListingErrorBody {
    code: String,
    message: String,
}

fn parse_ndjson(body: &str) -> Result<(ListingHeader, Vec<ListedAsset>), AssetListError> {
    let mut lines = body.lines().filter(|line| !line.trim().is_empty());

    let Some(header_line) = lines.next() else {
        return Err(AssetListError::Empty);
    };
    let header = serde_json::from_str(header_line)
        .map_err(|source| AssetListError::Decode { line: 1, source })?;

    let mut assets = Vec::new();
    for (index, line) in lines.enumerate() {
        let asset = serde_json::from_str(line).map_err(|source| AssetListError::Decode {
            line: index + 2,
            source,
        })?;
        assets.push(asset);
    }

    Ok((header, assets))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"policy":"b1e10e38af86a570e76f7eb5825d76fea3742a760d1b528afc8ded5a","as_of_chunk":5897,"total":2,"next":null}
{"name_hex":"4163756572646f","name":"Acuerdo","image":"ipfs://QmXdjsFN8WUHDS8RAGVWk4Qexb3Ns2etMakREwHUHvBU2M","media_type":"image/jpeg","supply":1}
{"name_hex":"4165737468657465","name":"Aesthete","image":"ipfs://QmabxgTxQYK5yN8LpcdCqjDPWFSzK6P3wgV9ECWjBvSnz4","media_type":"image/jpeg","supply":1}"#;

    #[test]
    fn parses_header_and_assets() {
        let (header, assets) = parse_ndjson(SAMPLE).unwrap();
        assert_eq!(header.as_of_chunk, 5897);
        assert_eq!(header.total, 2);
        assert!(header.next.is_none());
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].name_hex, "4163756572646f");
        assert_eq!(assets[0].name, "Acuerdo");
        assert_eq!(
            assets[0].image,
            "ipfs://QmXdjsFN8WUHDS8RAGVWk4Qexb3Ns2etMakREwHUHvBU2M"
        );
        assert_eq!(assets[1].name, "Aesthete");
    }

    #[test]
    fn parses_a_page_with_a_cursor() {
        let body = r#"{"policy":"b1e10e38af86a570e76f7eb5825d76fea3742a760d1b528afc8ded5a","as_of_chunk":5897,"total":2,"next":"page-2"}
{"name_hex":"4163756572646f","name":"Acuerdo","image":"ipfs://QmXdjsFN8WUHDS8RAGVWk4Qexb3Ns2etMakREwHUHvBU2M"}"#;
        let (header, assets) = parse_ndjson(body).unwrap();
        assert_eq!(header.next.as_deref(), Some("page-2"));
        assert_eq!(assets.len(), 1);
    }

    #[test]
    fn ignores_blank_lines() {
        let body = format!("\n{SAMPLE}\n\n");
        let (_, assets) = parse_ndjson(&body).unwrap();
        assert_eq!(assets.len(), 2);
    }

    #[test]
    fn rejects_an_empty_body() {
        assert!(matches!(parse_ndjson(""), Err(AssetListError::Empty)));
    }

    #[test]
    fn reports_the_failing_line() {
        let body = format!(
            "{}\n{{\"name_hex\":\"41\"}}",
            r#"{"policy":"b1e10e38af86a570e76f7eb5825d76fea3742a760d1b528afc8ded5a","as_of_chunk":5897,"total":1,"next":null}"#
        );
        match parse_ndjson(&body) {
            Err(AssetListError::Decode { line, .. }) => assert_eq!(line, 2),
            other => panic!("expected a decode error, got {other:?}"),
        }
    }
}
