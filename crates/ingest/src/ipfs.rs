//! IPFS fetching with gateway racing and rate limiting.
//!
//! Features:
//! - Multiple gateway support with parallel racing
//! - Exponential backoff with jitter for rate limiting
//! - Per-gateway health tracking
//! - Concurrency limits via semaphore

use cid::Cid;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, warn};
use viewer_format::IpfsGateway;

/// Convert a CID (v0 or v1) to CIDv1 string.
/// Blockfrost requires CIDv1 format.
fn to_cidv1(cid_str: &str) -> Option<String> {
    let cid = Cid::from_str(cid_str).ok()?;
    let cidv1 = if cid.version() == cid::Version::V0 {
        Cid::new_v1(cid.codec(), cid.hash().to_owned())
    } else {
        cid
    };
    Some(cidv1.to_string())
}

/// IPFS gateway configuration.
#[derive(Debug, Clone)]
pub struct Gateway {
    /// Gateway name for logging
    pub name: String,
    /// URL template with {cid} placeholder
    pub url_template: String,
    /// Optional authentication token (e.g., Pinata gateway key)
    pub token: Option<String>,
    /// Whether this gateway requires CIDv1 format
    pub requires_cidv1: bool,
    /// Current failure count (for deprioritization)
    failures: Arc<AtomicU32>,
}

impl Gateway {
    pub fn new(name: &str, url_template: &str) -> Self {
        Self {
            name: name.to_string(),
            url_template: url_template.to_string(),
            token: None,
            requires_cidv1: false,
            failures: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Create the Blockfrost gateway (requires CIDv1).
    pub fn new_blockfrost() -> Self {
        Self {
            name: "Blockfrost".to_string(),
            url_template: "https://ipfs.blockfrost.dev/ipfs/{cid}".to_string(),
            token: None,
            requires_cidv1: true,
            failures: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Create a gateway with an authentication token.
    ///
    /// For Pinata dedicated gateways, the token is appended as `?pinataGatewayToken=<token>`.
    pub fn new_with_token(name: &str, url_template: &str, token: &str) -> Self {
        Self {
            name: name.to_string(),
            url_template: url_template.to_string(),
            token: Some(token.to_string()),
            requires_cidv1: false,
            failures: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn build_url(&self, cid: &str, path: Option<&str>) -> Option<String> {
        // Convert to CIDv1 if required
        let cid_to_use = if self.requires_cidv1 {
            match to_cidv1(cid) {
                Some(v1) => v1,
                None => {
                    debug!("Failed to convert CID to v1: {}", cid);
                    return None;
                }
            }
        } else {
            cid.to_string()
        };

        let base = self.url_template.replace("{cid}", &cid_to_use);
        let url = match path {
            Some(p) => format!("{}/{}", base, p),
            None => base,
        };
        // Append token as query parameter if present
        Some(match &self.token {
            Some(token) => format!("{}?pinataGatewayToken={}", url, token),
            None => url,
        })
    }

    pub fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_success(&self) {
        self.failures.store(0, Ordering::Relaxed);
    }

    pub fn failure_count(&self) -> u32 {
        self.failures.load(Ordering::Relaxed)
    }
}

impl From<IpfsGateway> for Gateway {
    fn from(gw: IpfsGateway) -> Self {
        match gw {
            IpfsGateway::Blockfrost => Gateway::new_blockfrost(),
            IpfsGateway::Pinata => {
                // Use dedicated gateway if configured, otherwise public
                if let (Ok(host), Ok(key)) = (
                    std::env::var("PINATA_GATEWAY_HOST"),
                    std::env::var("PINATA_GATEWAY_KEY"),
                ) {
                    Gateway::new_with_token(
                        "Pinata (dedicated)",
                        &format!("https://{}/ipfs/{{cid}}", host),
                        &key,
                    )
                } else {
                    Gateway::new("Pinata", "https://gateway.pinata.cloud/ipfs/{cid}")
                }
            }
            IpfsGateway::Dweb => Gateway::new("dweb.link", "https://dweb.link/ipfs/{cid}"),
            IpfsGateway::IpfsIo => Gateway::new("ipfs.io", "https://ipfs.io/ipfs/{cid}"),
        }
    }
}

/// Default IPFS gateways in priority order.
///
/// Includes Blockfrost (no API key needed) and optionally Pinata.
/// If PINATA_GATEWAY_HOST and PINATA_GATEWAY_KEY are set in the environment,
/// uses the dedicated Pinata gateway. Otherwise falls back to public gateway.
pub fn default_gateways() -> Vec<Gateway> {
    let mut gateways = vec![
        // Blockfrost - no API key needed, requires CIDv1 (handled in build_url)
        Gateway::new_blockfrost(),
    ];

    // Check for dedicated Pinata gateway configuration
    if let (Ok(host), Ok(key)) = (
        std::env::var("PINATA_GATEWAY_HOST"),
        std::env::var("PINATA_GATEWAY_KEY"),
    ) {
        tracing::info!("Using dedicated Pinata gateway: {}", host);
        gateways.push(Gateway::new_with_token(
            "Pinata (dedicated)",
            &format!("https://{}/ipfs/{{cid}}", host),
            &key,
        ));
    } else {
        tracing::debug!(
            "Using public Pinata gateway (set PINATA_GATEWAY_HOST and PINATA_GATEWAY_KEY for dedicated)"
        );
        gateways.push(Gateway::new(
            "Pinata",
            "https://gateway.pinata.cloud/ipfs/{cid}",
        ));
    }

    gateways
}

/// Fetched image data with format detection.
#[derive(Debug)]
pub struct FetchedImage {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
}

/// Detected image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
    Gif,
    Svg,
    Unknown,
}

impl ImageFormat {
    /// Detect format from magic bytes.
    pub fn from_magic_bytes(bytes: &[u8]) -> Self {
        if bytes.len() < 12 {
            return ImageFormat::Unknown;
        }

        // PNG: 89 50 4E 47 0D 0A 1A 0A
        if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            return ImageFormat::Png;
        }

        // JPEG: FF D8 FF
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return ImageFormat::Jpeg;
        }

        // WebP: RIFF....WEBP
        if bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            return ImageFormat::WebP;
        }

        // GIF: GIF87a or GIF89a
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            return ImageFormat::Gif;
        }

        // SVG: <svg or <?xml
        if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
            return ImageFormat::Svg;
        }

        ImageFormat::Unknown
    }

    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::WebP => "webp",
            ImageFormat::Gif => "gif",
            ImageFormat::Svg => "svg",
            ImageFormat::Unknown => "bin",
        }
    }
}

/// IPFS fetcher with sequential gateway fallback.
pub struct IpfsFetcher {
    client: reqwest::Client,
    gateways: Vec<Gateway>,
    semaphore: Arc<Semaphore>,
    #[allow(dead_code)]
    max_retries: u32,
    #[allow(dead_code)]
    base_delay: Duration,
}

impl IpfsFetcher {
    /// Create a new fetcher with default gateways.
    pub fn new(concurrency: usize) -> Self {
        Self::with_gateways(concurrency, default_gateways())
    }

    /// Create a fetcher with custom gateways (does not use defaults).
    pub fn with_gateways(concurrency: usize, gateways: Vec<Gateway>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(90))
                .build()
                .expect("Failed to create HTTP client"),
            gateways,
            semaphore: Arc::new(Semaphore::new(concurrency)),
            max_retries: 3,
            base_delay: Duration::from_millis(500),
        }
    }

    /// Parse an IPFS URL and extract CID and optional path.
    fn parse_ipfs_url(url: &str) -> Option<(&str, Option<&str>)> {
        let url = url.trim();

        // ipfs://CID or ipfs://CID/path
        if let Some(rest) = url.strip_prefix("ipfs://") {
            // Handle malformed ipfs://ipfs/CID
            let rest = rest.strip_prefix("ipfs/").unwrap_or(rest);
            return Some(Self::split_cid_path(rest));
        }

        // Bare CID (starts with Qm or bafy)
        if url.starts_with("Qm") || url.starts_with("bafy") {
            return Some(Self::split_cid_path(url));
        }

        None
    }

    fn split_cid_path(s: &str) -> (&str, Option<&str>) {
        match s.find('/') {
            Some(pos) => (&s[..pos], Some(&s[pos + 1..])),
            None => (s, None),
        }
    }

    /// Fetch an image from IPFS with gateway racing.
    ///
    /// Returns the first successful response from any gateway.
    pub async fn fetch(&self, url: &str) -> Result<FetchedImage, FetchError> {
        let (cid, path) = Self::parse_ipfs_url(url).ok_or_else(|| FetchError::InvalidUrl {
            url: url.to_string(),
        })?;

        // Acquire semaphore permit for concurrency limiting
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| FetchError::Cancelled)?;

        self.fetch_with_fallback(cid, path).await
    }

    /// Fetch with sequential gateway fallback.
    /// Always tries gateways in configured order (first = primary, rest = fallbacks).
    async fn fetch_with_fallback(
        &self,
        cid: &str,
        path: Option<&str>,
    ) -> Result<FetchedImage, FetchError> {
        let num_gateways = self.gateways.len();
        if num_gateways == 0 {
            return Err(FetchError::Gateway {
                gateway: "none".to_string(),
                message: "No gateways configured".to_string(),
            });
        }

        let mut last_error = None;

        // Try each gateway in order (primary first, then fallbacks)
        for gateway in &self.gateways {
            let Some(url) = gateway.build_url(cid, path) else {
                debug!(gateway = %gateway.name, "Failed to build URL for CID {}", cid);
                continue;
            };

            tracing::info!(gateway = %gateway.name, url = %url, "Fetching from IPFS");

            match Self::fetch_from_gateway(&self.client, &url, &gateway.name).await {
                Ok(image) => {
                    tracing::info!(gateway = %gateway.name, url = %url, bytes = image.bytes.len(), "Fetch succeeded");
                    return Ok(image);
                }
                Err(e) => {
                    warn!(gateway = %gateway.name, url = %url, error = %e, "Gateway failed, trying next");
                    last_error = Some(e);
                    // Continue to next gateway
                }
            }
        }

        Err(last_error.unwrap_or_else(|| FetchError::Gateway {
            gateway: "all".to_string(),
            message: "All gateways failed".to_string(),
        }))
    }

    async fn fetch_from_gateway(
        client: &reqwest::Client,
        url: &str,
        gateway_name: &str,
    ) -> Result<FetchedImage, FetchError> {
        debug!("Fetching from {}: {}", gateway_name, url);

        let response = client.get(url).send().await.map_err(|e| {
            warn!(gateway = %gateway_name, error = %e, url = %url, "Gateway connection failed");
            FetchError::Gateway {
                gateway: gateway_name.to_string(),
                message: e.to_string(),
            }
        })?;

        let status = response.status();

        // Handle rate limiting
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(5);

            warn!(
                "Gateway {} rate limited, retry after {}s",
                gateway_name, retry_after
            );

            return Err(FetchError::RateLimited {
                gateway: gateway_name.to_string(),
                retry_after_secs: retry_after,
            });
        }

        if !status.is_success() {
            warn!(gateway = %gateway_name, status = %status, url = %url, "Gateway request failed");
            return Err(FetchError::Gateway {
                gateway: gateway_name.to_string(),
                message: format!("HTTP {}", status),
            });
        }

        let bytes = response.bytes().await.map_err(|e| FetchError::Gateway {
            gateway: gateway_name.to_string(),
            message: format!("Failed to read body: {}", e),
        })?;

        let format = ImageFormat::from_magic_bytes(&bytes);

        // Reject non-image responses (e.g., HTML error pages)
        if format == ImageFormat::Unknown {
            // Check if it looks like HTML
            if bytes.starts_with(b"<!DOCTYPE")
                || bytes.starts_with(b"<html")
                || bytes.starts_with(b"<HTML")
            {
                return Err(FetchError::Gateway {
                    gateway: gateway_name.to_string(),
                    message: "Received HTML instead of image".to_string(),
                });
            }
            return Err(FetchError::Gateway {
                gateway: gateway_name.to_string(),
                message: "Unknown image format".to_string(),
            });
        }

        debug!(
            "Gateway {} succeeded: {} bytes, format {:?}",
            gateway_name,
            bytes.len(),
            format
        );

        Ok(FetchedImage {
            bytes: bytes.to_vec(),
            format,
        })
    }

    /// Fetch from HTTPS URL (non-IPFS).
    pub async fn fetch_https(&self, url: &str) -> Result<FetchedImage, FetchError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| FetchError::Cancelled)?;

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| FetchError::Http {
                url: url.to_string(),
                message: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(FetchError::Http {
                url: url.to_string(),
                message: format!("HTTP {}", response.status()),
            });
        }

        let bytes = response.bytes().await.map_err(|e| FetchError::Http {
            url: url.to_string(),
            message: format!("Failed to read body: {}", e),
        })?;

        let format = ImageFormat::from_magic_bytes(&bytes);

        Ok(FetchedImage {
            bytes: bytes.to_vec(),
            format,
        })
    }

    /// Fetch image from either IPFS or HTTPS URL.
    pub async fn fetch_image(&self, url: &str) -> Result<FetchedImage, FetchError> {
        if url.starts_with("http://") || url.starts_with("https://") {
            self.fetch_https(url).await
        } else {
            self.fetch(url).await
        }
    }
}

/// Errors that can occur during IPFS fetching.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("Invalid IPFS URL: {url}")]
    InvalidUrl { url: String },

    #[error("Gateway {gateway} failed: {message}")]
    Gateway { gateway: String, message: String },

    #[error("Gateway {gateway} rate limited, retry after {retry_after_secs}s")]
    RateLimited {
        gateway: String,
        retry_after_secs: u64,
    },

    #[error("HTTP request to {url} failed: {message}")]
    Http { url: String, message: String },

    #[error("Fetch cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipfs_url() {
        // ipfs:// prefix
        assert_eq!(
            IpfsFetcher::parse_ipfs_url("ipfs://QmTest123"),
            Some(("QmTest123", None))
        );

        // ipfs:// with path
        assert_eq!(
            IpfsFetcher::parse_ipfs_url("ipfs://QmTest123/image.png"),
            Some(("QmTest123", Some("image.png")))
        );

        // Malformed double prefix
        assert_eq!(
            IpfsFetcher::parse_ipfs_url("ipfs://ipfs/QmTest123"),
            Some(("QmTest123", None))
        );

        // Bare CID
        assert_eq!(
            IpfsFetcher::parse_ipfs_url("QmTest123"),
            Some(("QmTest123", None))
        );

        // HTTPS URL should return None
        assert_eq!(
            IpfsFetcher::parse_ipfs_url("https://example.com/image.png"),
            None
        );
    }

    #[test]
    fn test_image_format_detection() {
        // PNG
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];
        assert_eq!(ImageFormat::from_magic_bytes(&png), ImageFormat::Png);

        // JPEG
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(ImageFormat::from_magic_bytes(&jpeg), ImageFormat::Jpeg);

        // WebP
        let webp = b"RIFF\x00\x00\x00\x00WEBP";
        assert_eq!(ImageFormat::from_magic_bytes(webp), ImageFormat::WebP);
    }

    #[test]
    fn test_gateway_url_building() {
        let gateway = Gateway::new("Test", "https://test.io/ipfs/{cid}");

        assert_eq!(
            gateway.build_url("QmTest", None),
            Some("https://test.io/ipfs/QmTest".to_string())
        );

        assert_eq!(
            gateway.build_url("QmTest", Some("path/image.png")),
            Some("https://test.io/ipfs/QmTest/path/image.png".to_string())
        );
    }
}
