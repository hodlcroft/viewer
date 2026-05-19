//! Filebase IPFS pinning client (experimental).
//!
//! Thin native wrapper around the shared `filebase` crate. Adds per-request
//! rate limiting and 429 retry-with-backoff so a batch pin pass against a
//! whole collection doesn't trip Filebase's limits.

use std::collections::BTreeMap;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

use filebase_sdk::{FilebaseApi, FilebaseError as ApiError, PinByCidResponse, PinListQuery};

/// Default delay between successive pin requests (20 req/s).
///
/// Filebase's paid Pinning Service tier handles substantially more than this
/// in practice — the 429 retry path below catches any overshoot. Override via
/// `FilebaseClient::with_delay` for tuning.
const DEFAULT_RATE_LIMIT_DELAY: Duration = Duration::from_millis(50);

/// Maximum retries on a 429 response before giving up on a single pin.
const MAX_RETRIES: u32 = 3;

/// Errors surfaced by the Filebase client.
#[derive(Debug, Error)]
pub enum FilebaseError {
    #[error("Filebase API call failed: {0}")]
    Api(#[from] ApiError),

    #[error("Missing FILEBASE_API_TOKEN environment variable")]
    MissingToken,

    #[error("Exhausted {attempts} retries on transient error: {last}")]
    RetryExhausted { attempts: u32, last: String },
}

/// A single item to pin, including the folder-style `name` and any metadata
/// that should be attached to the pin in the bucket dashboard.
#[derive(Debug, Clone)]
pub struct PinItem {
    pub cid: String,
    pub name: String,
    pub meta: BTreeMap<String, String>,
}

/// Outcome of pinning a single CID, surfaced to progress callbacks.
#[derive(Debug, Clone)]
pub struct PinResult {
    pub cid: String,
    pub name: String,
    pub outcome: Result<PinByCidResponse, String>,
}

/// A single pin record, projected from a `GET /pins` entry.
#[derive(Debug, Clone)]
pub struct PinRecord {
    pub requestid: String,
    pub cid: String,
    pub status: String,
}

/// Pin counts by Pinning Service status for a single collection.
#[derive(Debug, Default, Clone)]
pub struct PinStatusCounts {
    pub queued: u32,
    pub pinning: u32,
    pub pinned: u32,
    pub failed: u32,
}

impl PinStatusCounts {
    /// Total pins across all statuses.
    pub fn total(&self) -> u32 {
        self.queued + self.pinning + self.pinned + self.failed
    }
}

/// Native Filebase client wrapping the shared crate's `FilebaseApi`.
pub struct FilebaseClient {
    api: FilebaseApi,
    delay: Duration,
}

impl FilebaseClient {
    /// Construct a client from the `FILEBASE_API_TOKEN` env var.
    pub fn from_env() -> Result<Self, FilebaseError> {
        let token = std::env::var("FILEBASE_API_TOKEN").map_err(|_| FilebaseError::MissingToken)?;
        Ok(Self {
            api: FilebaseApi::with_token(token),
            delay: DEFAULT_RATE_LIMIT_DELAY,
        })
    }

    /// Override the inter-request delay used by `pin_cids`.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Pin a single CID with automatic retry on transient failures.
    ///
    /// Retries cover both transport-level errors (TCP/HTTP-level drops, where
    /// `status_code()` is `None`) and server-side transient HTTP responses
    /// (429, 502, 503, 504). Other 4xx/5xx codes are surfaced immediately —
    /// retrying won't help.
    pub async fn pin_by_cid(
        &self,
        cid: &str,
        name: Option<&str>,
        meta: Option<&BTreeMap<String, String>>,
    ) -> Result<PinByCidResponse, FilebaseError> {
        let mut attempt = 0u32;
        loop {
            match self.api.pin_by_cid(cid, name, meta).await {
                Ok(response) => return Ok(response),
                Err(ApiError::Request(http_err))
                    if matches!(
                        http_err.status_code(),
                        None | Some(429 | 502 | 503 | 504)
                    ) =>
                {
                    attempt += 1;
                    if attempt > MAX_RETRIES {
                        return Err(FilebaseError::RetryExhausted {
                            attempts: MAX_RETRIES,
                            last: format!("{http_err:?}"),
                        });
                    }

                    let wait = http_err
                        .retry_after_seconds()
                        .unwrap_or_else(|| 2u64.pow(attempt));
                    warn!(
                        cid,
                        attempt,
                        MAX_RETRIES,
                        wait,
                        status = ?http_err.status_code(),
                        "Transient Filebase error, retrying"
                    );
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
                Err(e) => return Err(FilebaseError::Api(e)),
            }
        }
    }

    /// Pin a batch of items, pacing requests and reporting progress. Errors
    /// on individual pins are captured per-item rather than aborting the
    /// batch — the caller decides what to do with the summary.
    pub async fn pin_cids(
        &self,
        items: &[PinItem],
        on_progress: Option<&dyn Fn(usize, usize, &PinResult)>,
    ) -> Vec<PinResult> {
        let total = items.len();
        let mut results = Vec::with_capacity(total);

        for (i, item) in items.iter().enumerate() {
            let meta = (!item.meta.is_empty()).then_some(&item.meta);
            let outcome = match self.pin_by_cid(&item.cid, Some(&item.name), meta).await {
                Ok(response) => {
                    debug!(name = %item.name, cid = %item.cid, request_id = %response.requestid, status = %response.status, "Pinned to Filebase");
                    Ok(response)
                }
                Err(e) => {
                    warn!(name = %item.name, cid = %item.cid, error = %e, "Filebase pin failed");
                    Err(e.to_string())
                }
            };

            let result = PinResult {
                cid: item.cid.clone(),
                name: item.name.clone(),
                outcome,
            };

            if let Some(cb) = on_progress {
                cb(i + 1, total, &result);
            }
            results.push(result);

            // Pace the next request unless we're done.
            if i + 1 < total && !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
        }

        let succeeded = results.iter().filter(|r| r.outcome.is_ok()).count();
        info!(succeeded, failed = total - succeeded, total, "Filebase batch complete");

        results
    }

    /// Tally pins for a collection by Pinning Service status.
    ///
    /// Issues one `GET /pins` count query per status, scoped to the
    /// collection by partial-matching the `{policy_id}/` name prefix every
    /// pin carries. Cheap — `count` is returned regardless of page size, so
    /// each query uses `limit=1`.
    pub async fn status_counts(
        &self,
        policy_id: &str,
    ) -> Result<PinStatusCounts, FilebaseError> {
        let mut counts = PinStatusCounts::default();
        for status in ["queued", "pinning", "pinned", "failed"] {
            let query = PinListQuery {
                status: Some(status.to_string()),
                name: Some(format!("{policy_id}/")),
                name_match: Some("partial".to_string()),
                limit: 1,
                ..Default::default()
            };
            let resp = self.api.list_pins(&query).await?;
            match status {
                "queued" => counts.queued = resp.count,
                "pinning" => counts.pinning = resp.count,
                "pinned" => counts.pinned = resp.count,
                "failed" => counts.failed = resp.count,
                _ => unreachable!(),
            }
        }
        Ok(counts)
    }

    /// List every pin for a collection, paging through `GET /pins`.
    ///
    /// Pages newest-first via the `before` cursor; entries are deduplicated
    /// by request id to absorb any timestamp-boundary overlap between pages.
    /// All four statuses are requested explicitly — `GET /pins` with no
    /// `status` filter omits `failed` (and `queued`) pins.
    pub async fn list_all_pins(
        &self,
        policy_id: &str,
    ) -> Result<Vec<PinRecord>, FilebaseError> {
        use std::collections::HashSet;

        const PAGE: u32 = 1000;
        let mut records = Vec::new();
        let mut seen = HashSet::new();
        let mut before: Option<String> = None;

        loop {
            let query = PinListQuery {
                name: Some(format!("{policy_id}/")),
                name_match: Some("partial".to_string()),
                status: Some("queued,pinning,pinned,failed".to_string()),
                before: before.clone(),
                limit: PAGE,
                ..Default::default()
            };
            let resp = self.api.list_pins(&query).await?;
            let page_len = resp.results.len();
            let oldest = resp.results.last().map(|e| e.created.clone());

            let before_count = records.len();
            for entry in resp.results {
                if seen.insert(entry.requestid.clone()) {
                    records.push(PinRecord {
                        requestid: entry.requestid,
                        cid: entry.pin.cid,
                        status: entry.status,
                    });
                }
            }

            // Stop on a short page, or if a full page yielded no new records
            // (a pathological all-same-timestamp boundary) to avoid looping.
            if page_len < PAGE as usize || records.len() == before_count {
                break;
            }
            match oldest {
                Some(ts) => before = Some(ts),
                None => break,
            }
        }

        Ok(records)
    }

    /// Delete a batch of pins by request id, pacing and retrying transient
    /// failures. Per-item errors are captured rather than aborting the batch.
    pub async fn delete_pins(
        &self,
        requestids: &[String],
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Vec<Result<(), String>> {
        let total = requestids.len();
        let mut out = Vec::with_capacity(total);

        for (i, rid) in requestids.iter().enumerate() {
            let result = self
                .delete_pin_with_retry(rid)
                .await
                .map_err(|e| e.to_string());
            if let Err(ref e) = result {
                warn!(requestid = %rid, error = %e, "Filebase delete failed");
            }
            out.push(result);

            if let Some(cb) = on_progress {
                cb(i + 1, total);
            }
            if i + 1 < total && !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
        }

        let deleted = out.iter().filter(|r| r.is_ok()).count();
        info!(deleted, failed = total - deleted, total, "Filebase delete batch complete");

        out
    }

    /// Delete a single pin with automatic retry on transient failures.
    async fn delete_pin_with_retry(&self, requestid: &str) -> Result<(), FilebaseError> {
        let mut attempt = 0u32;
        loop {
            match self.api.delete_pin(requestid).await {
                Ok(()) => return Ok(()),
                Err(ApiError::Request(http_err))
                    if matches!(
                        http_err.status_code(),
                        None | Some(429 | 502 | 503 | 504)
                    ) =>
                {
                    attempt += 1;
                    if attempt > MAX_RETRIES {
                        return Err(FilebaseError::RetryExhausted {
                            attempts: MAX_RETRIES,
                            last: format!("{http_err:?}"),
                        });
                    }
                    let wait = http_err
                        .retry_after_seconds()
                        .unwrap_or_else(|| 2u64.pow(attempt));
                    debug!(requestid, attempt, wait, "Transient delete error, retrying");
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
                Err(e) => return Err(FilebaseError::Api(e)),
            }
        }
    }
}
