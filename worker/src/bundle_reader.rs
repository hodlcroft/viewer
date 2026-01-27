use serde::{Deserialize, Serialize};
use worker_stack::prelude::*;
use worker_stack::worker::Range;

/// Index entry for a token in the bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub offset: u64,
    pub length: u64,
    #[serde(default)]
    pub hash: Option<String>,
    /// Shard index (0 = images_000.bin, 1 = images_001.bin, etc.)
    #[serde(default)]
    pub shard: u32,
}

/// Bundle index mapping token IDs to their location in image shards
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleIndex {
    pub version: u32,
    pub image_format: String,
    pub image_count: u32,
    /// Number of shard files
    #[serde(default = "default_shard_count")]
    pub shard_count: u32,
    pub entries: std::collections::HashMap<String, IndexEntry>,
}

fn default_shard_count() -> u32 {
    1
}

const INDEX_CACHE_TTL: u64 = 86400; // 24 hours

/// Reader for bundle format (index.json + images.bin)
pub struct BundleReader<'a> {
    bucket: &'a Bucket,
    project: &'a str,
    seed: &'a str,
}

impl<'a> BundleReader<'a> {
    pub fn new(bucket: &'a Bucket, project: &'a str, seed: &'a str) -> Self {
        Self {
            bucket,
            project,
            seed,
        }
    }

    /// Get the base path in R2 for this generation
    fn base_path(&self) -> String {
        format!("generations/{}/{}", self.project, self.seed)
    }

    /// Load the bundle index, using KV cache if available
    async fn load_index(&self, kv: Option<&KvStore>) -> Result<BundleIndex> {
        let cache_key = format!("bundle-index:{}:{}", self.project, self.seed);

        // Try KV cache first
        if let Some(kv_store) = kv {
            if let Ok(Some(cached)) = kv_store.get(&cache_key).json::<BundleIndex>().await {
                return Ok(cached);
            }
        }

        // Fetch from R2
        let key = format!("{}/index.json", self.base_path());
        let object = self
            .bucket
            .get(&key)
            .execute()
            .await?
            .ok_or_else(|| Error::RustError(format!("Bundle index not found: {key}")))?;

        let body = object
            .body()
            .ok_or_else(|| Error::RustError("Index body not available".to_string()))?;

        let bytes = body.bytes().await?;
        let index: BundleIndex = serde_json::from_slice(&bytes)
            .map_err(|e| Error::RustError(format!("Failed to parse index: {e}")))?;

        // Cache in KV
        if let Some(kv_store) = kv {
            let json = serde_json::to_string(&index)
                .map_err(|e| Error::RustError(format!("Failed to serialize index: {e}")))?;

            let _ = kv_store
                .put(&cache_key, json)
                .map_err(|e| Error::RustError(format!("KV put error: {e:?}")))?
                .expiration_ttl(INDEX_CACHE_TTL)
                .execute()
                .await;
        }

        Ok(index)
    }

    /// Get an image by token ID from the bundle
    /// Returns (image_bytes, image_format)
    pub async fn get_image(&self, id: &str, kv: Option<&KvStore>) -> Result<(Vec<u8>, String)> {
        // Load index
        let index = self.load_index(kv).await?;

        let image_format = index.image_format.clone();

        // Normalize ID by stripping leading zeros (asset_details uses "000001", index uses "1")
        let normalized_id = id.trim_start_matches('0');
        let normalized_id = if normalized_id.is_empty() {
            "0"
        } else {
            normalized_id
        };

        // Find entry for this ID
        let entry = index
            .entries
            .get(normalized_id)
            .ok_or_else(|| Error::RustError(format!("Token {id} not found in index")))?;

        // Fetch the specific byte range from the correct shard file
        let images_key = format!("{}/images_{:03}.bin", self.base_path(), entry.shard);

        // Use range request to get just the bytes we need
        let object = self
            .bucket
            .get(&images_key)
            .range(Range::OffsetWithLength {
                offset: entry.offset,
                length: entry.length,
            })
            .execute()
            .await?
            .ok_or_else(|| Error::RustError(format!("Images file not found: {images_key}")))?;

        let body = object
            .body()
            .ok_or_else(|| Error::RustError("Images body not available".to_string()))?;

        let bytes = body.bytes().await?;

        if bytes.len() != entry.length as usize {
            return Err(Error::RustError(format!(
                "Size mismatch: expected {}, got {}",
                entry.length,
                bytes.len()
            )));
        }

        Ok((bytes, image_format))
    }
}
