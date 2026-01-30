//! NFTCDN.io client for fetching NFT images.
//!
//! NFTCDN provides image serving for Cardano NFTs using CIP-14 asset fingerprints.
//! Authentication uses SHA256 HMAC with base64url encoding.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use cardano_assets::AssetId;
use hmac::{Hmac, Mac};
use reqwest::Client;
use sha2::Sha256;
use thiserror::Error;
use tracing::debug;

type HmacSha256 = Hmac<Sha256>;

/// NFTCDN client for authenticated image fetching.
#[derive(Clone)]
pub struct NftcdnClient {
    client: Client,
    domain: String,
    secret: Vec<u8>,
}

/// NFTCDN errors.
#[derive(Debug, Error)]
pub enum NftcdnError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("NFTCDN error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("Missing NFTCDN_DOMAIN environment variable")]
    MissingDomain,

    #[error("Missing NFTCDN_SECRET environment variable")]
    MissingSecret,

    #[error("Invalid secret key: {0}")]
    InvalidSecret(String),

    #[error("Failed to compute asset fingerprint: {0}")]
    Fingerprint(String),
}

impl NftcdnClient {
    /// Create a new NFTCDN client from environment variables.
    ///
    /// Required env vars:
    /// - `NFTCDN_DOMAIN`: Your subdomain (e.g., "defrag.nftcdn.io")
    /// - `NFTCDN_SECRET`: Base64-encoded HMAC secret key
    pub fn from_env() -> Result<Self, NftcdnError> {
        let domain = std::env::var("NFTCDN_DOMAIN").map_err(|_| NftcdnError::MissingDomain)?;
        let secret_b64 = std::env::var("NFTCDN_SECRET").map_err(|_| NftcdnError::MissingSecret)?;

        let secret = base64::engine::general_purpose::STANDARD
            .decode(&secret_b64)
            .map_err(|e| NftcdnError::InvalidSecret(e.to_string()))?;

        Ok(Self {
            client: Client::new(),
            domain,
            secret,
        })
    }

    /// Create a new NFTCDN client with explicit configuration.
    pub fn new(domain: String, secret_b64: &str) -> Result<Self, NftcdnError> {
        let secret = base64::engine::general_purpose::STANDARD
            .decode(secret_b64)
            .map_err(|e| NftcdnError::InvalidSecret(e.to_string()))?;

        Ok(Self {
            client: Client::new(),
            domain,
            secret,
        })
    }

    /// Build an authenticated image URL for an asset.
    ///
    /// Returns the full URL with HMAC token for fetching the original image.
    pub fn image_url(&self, asset_id: &AssetId) -> Result<String, NftcdnError> {
        let fingerprint = asset_id
            .fingerprint()
            .map_err(|e| NftcdnError::Fingerprint(e.to_string()))?;

        // Build URL with empty tk parameter for HMAC computation
        let url_for_hmac = format!("https://{}.{}/image?tk=", fingerprint, self.domain);

        // Compute HMAC-SHA256
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC can take key of any size");
        mac.update(url_for_hmac.as_bytes());
        let result = mac.finalize();
        let hmac_bytes = result.into_bytes();

        // Encode as base64url (no padding)
        let token = URL_SAFE_NO_PAD.encode(hmac_bytes);

        Ok(format!(
            "https://{}.{}/image?tk={}",
            fingerprint, self.domain, token
        ))
    }

    /// Build an authenticated image URL with size parameter.
    ///
    /// Size must be a power of 2: 32, 64, 128, 256, 512, or 1024.
    /// Returns WebP format when size is specified.
    pub fn image_url_sized(&self, asset_id: &AssetId, size: u32) -> Result<String, NftcdnError> {
        let fingerprint = asset_id
            .fingerprint()
            .map_err(|e| NftcdnError::Fingerprint(e.to_string()))?;

        // Build URL with empty tk parameter for HMAC computation
        let url_for_hmac = format!(
            "https://{}.{}/image?tk=&size={}",
            fingerprint, self.domain, size
        );

        // Compute HMAC-SHA256
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC can take key of any size");
        mac.update(url_for_hmac.as_bytes());
        let result = mac.finalize();
        let hmac_bytes = result.into_bytes();

        // Encode as base64url (no padding)
        let token = URL_SAFE_NO_PAD.encode(hmac_bytes);

        Ok(format!(
            "https://{}.{}/image?tk={}&size={}",
            fingerprint, self.domain, token, size
        ))
    }

    /// Fetch an image for an asset, returning the raw bytes.
    pub async fn fetch_image(&self, asset_id: &AssetId) -> Result<Vec<u8>, NftcdnError> {
        let url = self.image_url(asset_id)?;
        debug!("Fetching image from NFTCDN: {}", url);

        let response = self.client.get(&url).send().await?;
        let status = response.status();

        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(NftcdnError::Api {
                status: status.as_u16(),
                message,
            });
        }

        Ok(response.bytes().await?.to_vec())
    }

    /// Fetch a sized image for an asset, returning the raw bytes (WebP format).
    pub async fn fetch_image_sized(
        &self,
        asset_id: &AssetId,
        size: u32,
    ) -> Result<Vec<u8>, NftcdnError> {
        let url = self.image_url_sized(asset_id, size)?;
        debug!("Fetching sized image from NFTCDN: {}", url);

        let response = self.client.get(&url).send().await?;
        let status = response.status();

        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(NftcdnError::Api {
                status: status.as_u16(),
                message,
            });
        }

        Ok(response.bytes().await?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_url_generation() {
        // Test with known values
        let client = NftcdnClient::new(
            "defrag.nftcdn.io".to_string(),
            "x4TJJY1iPQKpWhnNMAgxo3w1OaxCqdb7ZWVTiahBHDM=",
        )
        .unwrap();

        // Use a test asset ID
        let asset_id = AssetId::new(
            "1088b361c41f49906645cedeeb7a9ef0e0b793b1a2d24f623ea74876".to_string(),
            "4861766f63576f726c647333343037".to_string(),
        )
        .unwrap();

        let url = client.image_url(&asset_id).unwrap();

        // URL should contain the fingerprint and domain
        assert!(url.contains("nftcdn.io"));
        assert!(url.contains("asset1"));
        assert!(url.contains("tk="));
    }
}
