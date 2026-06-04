//! Filebase S3-compatible API client.
//!
//! Used for the rescue re-host path — the RPC `/api/v0/add` endpoint always
//! keys bucket objects by CID, so we use the S3 API where the `Key` directly
//! controls the bucket path. Filebase auto-pins content PUT to an
//! IPFS-enabled bucket and exposes the computed CID via response headers.

use std::time::Duration;

use s3::creds::Credentials;
use s3::{Bucket, Region};
use thiserror::Error;
use tracing::debug;

const DEFAULT_ENDPOINT: &str = "https://s3.filebase.com";
const DEFAULT_REGION: &str = "us-east-1";
const DEFAULT_BUCKET: &str = "hodlcroft";

#[derive(Debug, Error)]
pub enum FilebaseS3Error {
    #[error("Missing FILEBASE_S3_ACCESS_TOKEN environment variable")]
    MissingAccessToken,

    #[error("Missing FILEBASE_S3_SECRET_KEY environment variable")]
    MissingSecretKey,

    #[error("S3 client error: {0}")]
    S3(String),
}

/// Outcome of a successful PUT — the bucket key written and the CID
/// Filebase computed (when exposed via response headers).
#[derive(Debug, Clone)]
pub struct PutResult {
    pub key: String,
    pub cid: Option<String>,
}

/// Result of HEAD-ing an object. `metadata` contains lower-cased
/// `x-amz-meta-*` headers Filebase attaches — notably `cid`.
#[derive(Debug, Clone)]
pub struct HeadInfo {
    pub key: String,
    pub content_length: i64,
    pub etag: Option<String>,
    pub content_type: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

impl HeadInfo {
    pub fn cid(&self) -> Option<&str> {
        self.metadata
            .get("cid")
            .or_else(|| self.metadata.get("x-amz-meta-cid"))
            .map(|s| s.as_str())
    }
}

/// Native Filebase S3 client.
pub struct FilebaseS3Client {
    bucket: Box<Bucket>,
    bucket_name: String,
}

impl FilebaseS3Client {
    /// Construct from env vars: `FILEBASE_S3_ACCESS_TOKEN`,
    /// `FILEBASE_S3_SECRET_KEY`. `FILEBASE_BUCKET` overrides the default
    /// `hodlcroft` bucket.
    pub fn from_env() -> Result<Self, FilebaseS3Error> {
        let access =
            std::env::var("FILEBASE_S3_ACCESS_TOKEN").map_err(|_| FilebaseS3Error::MissingAccessToken)?;
        let secret =
            std::env::var("FILEBASE_S3_SECRET_KEY").map_err(|_| FilebaseS3Error::MissingSecretKey)?;
        let bucket_name = std::env::var("FILEBASE_BUCKET").unwrap_or_else(|_| DEFAULT_BUCKET.to_string());

        let region = Region::Custom {
            region: DEFAULT_REGION.to_string(),
            endpoint: DEFAULT_ENDPOINT.to_string(),
        };
        let creds = Credentials::new(Some(&access), Some(&secret), None, None, None)
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?;
        let bucket = Bucket::new(&bucket_name, region, creds)
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?
            .with_request_timeout(Duration::from_secs(60))
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?;

        Ok(Self {
            bucket,
            bucket_name,
        })
    }

    /// PUT an object to the bucket at the given key.
    ///
    /// Filebase auto-pins on PUT to an IPFS-enabled bucket and returns the
    /// IPFS CID via the `x-amz-meta-cid` response header.
    pub async fn put_object(
        &self,
        key: &str,
        bytes: &[u8],
    ) -> Result<PutResult, FilebaseS3Error> {
        debug!(key, bytes = bytes.len(), "S3 PUT");
        let resp = self
            .bucket
            .put_object(key, bytes)
            .await
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?;

        let status = resp.status_code();
        if !(200..300).contains(&status) {
            return Err(FilebaseS3Error::S3(format!(
                "PUT {key} returned HTTP {status}: {}",
                String::from_utf8_lossy(resp.bytes())
            )));
        }

        let headers = resp.headers();
        let cid = headers
            .get("x-amz-meta-cid")
            .or_else(|| headers.get("cid"))
            .cloned();

        Ok(PutResult {
            key: key.to_string(),
            cid,
        })
    }

    /// Server-side COPY from `src_key` to `dst_key` within the bucket. No
    /// data is re-uploaded; Filebase reuses the existing block.
    pub async fn copy_object(&self, src_key: &str, dst_key: &str) -> Result<(), FilebaseS3Error> {
        debug!(src = src_key, dst = dst_key, "S3 COPY");
        let status = self
            .bucket
            .copy_object_internal(src_key, dst_key)
            .await
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(FilebaseS3Error::S3(format!(
                "COPY {src_key} -> {dst_key} returned HTTP {status}"
            )));
        }
        Ok(())
    }

    /// DELETE an object by key.
    pub async fn delete_object(&self, key: &str) -> Result<(), FilebaseS3Error> {
        debug!(key, "S3 DELETE");
        let resp = self
            .bucket
            .delete_object(key)
            .await
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?;
        let status = resp.status_code();
        if !(200..300).contains(&status) {
            return Err(FilebaseS3Error::S3(format!(
                "DELETE {key} returned HTTP {status}"
            )));
        }
        Ok(())
    }

    pub fn bucket_name(&self) -> &str {
        &self.bucket_name
    }

    /// HEAD an object — returns its size, etag, content-type, and any
    /// `x-amz-meta-*` metadata Filebase attached (notably `cid`).
    pub async fn head_object(&self, key: &str) -> Result<HeadInfo, FilebaseS3Error> {
        debug!(key, "S3 HEAD");
        let (head, status) = self
            .bucket
            .head_object(key)
            .await
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(FilebaseS3Error::S3(format!(
                "HEAD {key} returned HTTP {status}"
            )));
        }
        Ok(HeadInfo {
            key: key.to_string(),
            content_length: head.content_length.unwrap_or(0),
            etag: head.e_tag,
            content_type: head.content_type,
            metadata: head.metadata.unwrap_or_default(),
        })
    }

    /// List objects under the given prefix. Returns each object's key and
    /// size; paging is handled by the underlying client.
    pub async fn list_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, u64)>, FilebaseS3Error> {
        let pages = self
            .bucket
            .list(prefix.to_string(), None)
            .await
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?;
        let mut out = Vec::new();
        for page in pages {
            for obj in page.contents {
                out.push((obj.key, obj.size));
            }
        }
        Ok(out)
    }

    /// List with a delimiter so common prefixes (top-level "folders")
    /// are reported separately from immediate objects. Returns
    /// `(contents, common_prefixes)`.
    pub async fn list_with_delimiter(
        &self,
        prefix: &str,
        delimiter: &str,
    ) -> Result<(Vec<(String, u64)>, Vec<String>), FilebaseS3Error> {
        let pages = self
            .bucket
            .list(prefix.to_string(), Some(delimiter.to_string()))
            .await
            .map_err(|e| FilebaseS3Error::S3(e.to_string()))?;
        let mut contents = Vec::new();
        let mut prefixes = Vec::new();
        for page in pages {
            for obj in page.contents {
                contents.push((obj.key, obj.size));
            }
            if let Some(cps) = page.common_prefixes {
                for cp in cps {
                    prefixes.push(cp.prefix);
                }
            }
        }
        Ok((contents, prefixes))
    }
}
