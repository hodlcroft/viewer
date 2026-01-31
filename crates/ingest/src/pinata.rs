//! Pinata API client for image pinning and serving.
//!
//! Provides group management and CID pinning functionality for collections
//! that want guaranteed availability via Pinata's infrastructure.

use reqwest::{multipart, Client};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

const PINATA_API_BASE: &str = "https://api.pinata.cloud/v3";

/// Maximum number of pending pin requests before we pause and wait.
/// Pinata can handle ~250 active requests before backfilling.
const PIN_QUEUE_CAPACITY: usize = 100;

/// Delay between API requests to respect rate limits.
/// 250 requests/minute = ~4 requests/second = 250ms between requests.
const API_RATE_LIMIT_DELAY: Duration = Duration::from_millis(250);

/// Pinata API client.
#[derive(Clone)]
pub struct PinataClient {
    client: Client,
    jwt: String,
    gateway_host: Option<String>,
    gateway_key: Option<String>,
}

/// Pinata API errors.
#[derive(Debug, Error)]
pub enum PinataError {
    #[error("Pinata API request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Pinata API error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("Missing PINATA_API_JWT environment variable")]
    MissingJwt,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

/// Group information from Pinata API.
#[derive(Debug, Clone, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub is_public: bool,
    pub created_at: String,
}

/// File information from Pinata API.
#[derive(Debug, Clone, Deserialize)]
pub struct PinataFile {
    pub id: String,
    pub name: Option<String>,
    pub cid: String,
    pub group_id: Option<String>,
    pub mime_type: Option<String>,
    pub created_at: String,
}

/// Pin status from pin-by-CID response.
#[derive(Debug, Clone, Deserialize)]
pub struct PinStatus {
    pub id: String,
    pub cid: String,
    pub status: String,
}

// API response wrappers
#[derive(Debug, Deserialize)]
struct GroupResponse {
    data: Group,
}

#[derive(Debug, Deserialize)]
struct GroupsListResponse {
    data: GroupsData,
}

#[derive(Debug, Deserialize)]
struct GroupsData {
    groups: Vec<Group>,
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FilesListResponse {
    data: FilesData,
}

#[derive(Debug, Deserialize)]
struct FilesData {
    files: Vec<PinataFile>,
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PinResponse {
    data: PinStatus,
}

#[derive(Debug, Deserialize)]
struct PinRequestsResponse {
    data: PinRequestsData,
}

#[derive(Debug, Deserialize)]
struct PinRequestsData {
    #[serde(default, deserialize_with = "deserialize_null_as_empty_vec")]
    jobs: Vec<PinJob>,
    #[serde(alias = "nextPageToken")]
    next_page_token: Option<String>,
}

fn deserialize_null_as_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    let opt: Option<Vec<T>> = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// A pin request job from the queue.
#[derive(Debug, Clone, Deserialize)]
pub struct PinJob {
    pub id: String,
    pub cid: String,
    pub name: Option<String>,
    pub status: String,
    pub group_id: Option<String>,
    pub date_queued: String,
}

#[derive(Debug, Serialize)]
struct CreateGroupRequest {
    name: String,
}

#[derive(Debug, Serialize)]
struct PinByCidRequest {
    cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_id: Option<String>,
}

/// Response from file upload.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadedFile {
    pub id: String,
    pub name: String,
    pub cid: String,
    pub size: u64,
    pub mime_type: String,
    pub group_id: Option<String>,
    #[serde(default)]
    pub is_duplicate: bool,
}

#[derive(Debug, Deserialize)]
struct UploadResponse {
    data: UploadedFile,
}

impl PinataClient {
    /// Create a new Pinata client from environment variables.
    ///
    /// Requires `PINATA_API_JWT` to be set.
    /// Optionally uses `PINATA_GATEWAY_HOST` for optimized image URLs.
    /// Optionally uses `PINATA_GATEWAY_KEY` for dedicated gateway authentication.
    pub fn from_env() -> Result<Self, PinataError> {
        let jwt = std::env::var("PINATA_API_JWT").map_err(|_| PinataError::MissingJwt)?;

        let gateway_host = std::env::var("PINATA_GATEWAY_HOST").ok();
        let gateway_key = std::env::var("PINATA_GATEWAY_KEY").ok();

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Ok(Self {
            client,
            jwt,
            gateway_host,
            gateway_key,
        })
    }

    /// Send a request with automatic retry on rate limit (429).
    ///
    /// Retries up to 3 times with exponential backoff when rate limited.
    async fn send_with_retry(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, PinataError> {
        const MAX_RETRIES: u32 = 3;
        let mut attempt = 0;

        loop {
            // Clone the request for retry (RequestBuilder can only be sent once)
            let request = request
                .try_clone()
                .ok_or_else(|| PinataError::InvalidResponse("Request cannot be cloned".into()))?;

            let response = request.send().await?;

            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(PinataError::Api {
                        status: 429,
                        message: "Rate limited after max retries".to_string(),
                    });
                }

                // Get retry-after header or use exponential backoff
                let retry_after = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(2u64.pow(attempt)); // 2, 4, 8 seconds

                warn!(
                    "Rate limited by Pinata API, waiting {}s (attempt {}/{})",
                    retry_after, attempt, MAX_RETRIES
                );
                tokio::time::sleep(Duration::from_secs(retry_after)).await;
                continue;
            }

            return Ok(response);
        }
    }

    /// Check response status and return appropriate error.
    fn check_response(response: &reqwest::Response) -> Option<PinataError> {
        if response.status().is_success() {
            None
        } else {
            Some(PinataError::Api {
                status: response.status().as_u16(),
                message: format!("HTTP {}", response.status()),
            })
        }
    }

    /// Get the gateway host for image URLs.
    pub fn gateway_host(&self) -> Option<&str> {
        self.gateway_host.as_deref()
    }

    /// Build an optimized image URL for a CID.
    ///
    /// Uses Pinata's image optimization parameters:
    /// - `img-width` + `img-height` = square bounding box
    /// - `img-fit=contain` = preserve aspect ratio, letterbox/pillarbox if needed
    /// - `img-format` = explicit format (png for thumbnails, webp for viewer)
    /// - `pinataGatewayToken` = authentication for dedicated gateways
    pub fn image_url(&self, cid: &str, size: u32, format: &str) -> Option<String> {
        let host = self.gateway_host.as_ref()?;
        let mut url = format!(
            "https://{}/ipfs/{}?img-width={}&img-height={}&img-fit=contain&img-format={}",
            host, cid, size, size, format
        );

        // Add gateway key if configured (required for dedicated gateways)
        if let Some(key) = &self.gateway_key {
            url.push_str("&pinataGatewayToken=");
            url.push_str(key);
        }

        Some(url)
    }

    /// List groups, optionally filtering by name.
    pub async fn list_groups(&self, name_filter: Option<&str>) -> Result<Vec<Group>, PinataError> {
        let mut url = format!("{}/groups/public", PINATA_API_BASE);

        if let Some(name) = name_filter {
            url = format!("{}?name={}", url, urlencoding::encode(name));
        }

        debug!("Listing Pinata groups: {}", url);

        let response = self
            .send_with_retry(self.client.get(&url).bearer_auth(&self.jwt))
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        let result: GroupsListResponse = response.json().await?;
        Ok(result.data.groups)
    }

    /// Find a group by exact name.
    pub async fn find_group(&self, name: &str) -> Result<Option<Group>, PinataError> {
        let groups = self.list_groups(Some(name)).await?;
        Ok(groups.into_iter().find(|g| g.name == name))
    }

    /// Find a group by ID.
    pub async fn find_group_by_id(&self, id: &str) -> Result<Option<Group>, PinataError> {
        let groups = self.list_groups(None).await?;
        Ok(groups.into_iter().find(|g| g.id == id))
    }

    /// Create a new group.
    pub async fn create_group(&self, name: &str) -> Result<Group, PinataError> {
        let url = format!("{}/groups/public", PINATA_API_BASE);

        info!("Creating Pinata group: {}", name);

        let response = self
            .send_with_retry(self.client.post(&url).bearer_auth(&self.jwt).json(
                &CreateGroupRequest {
                    name: name.to_string(),
                },
            ))
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        let result: GroupResponse = response.json().await?;
        Ok(result.data)
    }

    /// Find or create a group by name.
    pub async fn ensure_group(&self, name: &str) -> Result<Group, PinataError> {
        if let Some(group) = self.find_group(name).await? {
            info!("Found existing Pinata group: {} ({})", name, group.id);
            return Ok(group);
        }

        self.create_group(name).await
    }

    /// List files not in any group.
    pub async fn list_ungrouped_files(&self) -> Result<Vec<PinataFile>, PinataError> {
        self.list_files_with_group_filter(Some("null")).await
    }

    /// List files in a group.
    pub async fn list_files_in_group(
        &self,
        group_id: &str,
    ) -> Result<Vec<PinataFile>, PinataError> {
        self.list_files_with_group_filter(Some(group_id)).await
    }

    /// List files with group filter.
    /// Pass Some("null") for ungrouped files, Some(id) for a specific group.
    async fn list_files_with_group_filter(
        &self,
        group_filter: Option<&str>,
    ) -> Result<Vec<PinataFile>, PinataError> {
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!("{}/files/public", PINATA_API_BASE);

            if let Some(group) = group_filter {
                url = format!("{}?group={}", url, group);
            }

            if let Some(ref token) = page_token {
                let sep = if group_filter.is_some() { "&" } else { "?" };
                url = format!("{}{}pageToken={}", url, sep, token);
            }

            debug!("Listing files: {}", url);

            let response = self
                .send_with_retry(self.client.get(&url).bearer_auth(&self.jwt))
                .await?;

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let message = response.text().await.unwrap_or_default();
                return Err(PinataError::Api { status, message });
            }

            let result: FilesListResponse = response.json().await?;
            let files_in_page = result.data.files.len();
            all_files.extend(result.data.files);

            // Only continue pagination if we got a full page of results
            // Pinata sometimes returns next_page_token even with empty/partial results
            match result.data.next_page_token {
                Some(token) if !token.is_empty() && files_in_page > 0 => page_token = Some(token),
                _ => break,
            }
        }

        Ok(all_files)
    }

    /// Check if a CID is already pinned (anywhere in account).
    pub async fn find_file_by_cid(&self, cid: &str) -> Result<Option<PinataFile>, PinataError> {
        let url = format!("{}/files/public?cid={}", PINATA_API_BASE, cid);

        let response = self
            .send_with_retry(self.client.get(&url).bearer_auth(&self.jwt))
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        let result: FilesListResponse = response.json().await?;
        Ok(result.data.files.into_iter().next())
    }

    /// Pin a CID to Pinata, optionally adding to a group.
    pub async fn pin_by_cid(
        &self,
        cid: &str,
        name: Option<&str>,
        group_id: Option<&str>,
    ) -> Result<PinStatus, PinataError> {
        let url = format!("{}/files/public/pin_by_cid", PINATA_API_BASE);

        debug!("Pinning CID {} to group {:?}", cid, group_id);

        let response =
            self.send_with_retry(self.client.post(&url).bearer_auth(&self.jwt).json(
                &PinByCidRequest {
                    cid: cid.to_string(),
                    name: name.map(|s| s.to_string()),
                    group_id: group_id.map(|s| s.to_string()),
                },
            ))
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        let result: PinResponse = response.json().await?;
        Ok(result.data)
    }

    /// Add an existing file to a group.
    pub async fn add_file_to_group(
        &self,
        file_id: &str,
        group_id: &str,
    ) -> Result<(), PinataError> {
        let url = format!(
            "{}/groups/public/{}/ids/{}",
            PINATA_API_BASE, group_id, file_id
        );

        debug!("Adding file {} to group {}", file_id, group_id);

        let response = self
            .send_with_retry(self.client.put(&url).bearer_auth(&self.jwt))
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        Ok(())

    }
    /// Upload a file to Pinata.
    ///
    /// The file is uploaded directly to Pinata's storage (not pin-by-CID).
    /// This is useful for uploading new content that doesn't exist on IPFS yet.
    ///
    /// # Arguments
    /// * `path` - Path to the file to upload
    /// * `name` - Optional name for the file (defaults to filename)
    /// * `group_id` - Optional group ID to add the file to
    ///
    /// # Returns
    /// The uploaded file information including its CID.
    pub async fn upload_file(
        &self,
        path: &Path,
        name: Option<&str>,
        group_id: Option<&str>,
    ) -> Result<UploadedFile, PinataError> {
        const UPLOAD_BASE: &str = "https://uploads.pinata.cloud/v3";

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let display_name = name.unwrap_or(file_name);

        debug!("Uploading file {} as '{}' to group {:?}", path.display(), display_name, group_id);

        // Read file contents
        let file_bytes = tokio::fs::read(path).await.map_err(|e| {
            PinataError::InvalidResponse(format!("Failed to read file {}: {}", path.display(), e))
        })?;

        // Detect MIME type from extension
        let mime_type = match path.extension().and_then(|e| e.to_str()) {
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            Some("svg") => "image/svg+xml",
            Some("json") => "application/json",
            _ => "application/octet-stream",
        };

        // Build multipart form
        let file_part = multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str(mime_type)
            .map_err(|e| PinataError::InvalidResponse(format!("Invalid MIME type: {}", e)))?;

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("name", display_name.to_string())
            .text("network", "public");

        if let Some(gid) = group_id {
            form = form.text("group_id", gid.to_string());
        }

        let url = format!("{}/files", UPLOAD_BASE);

        // Note: multipart requests cannot use send_with_retry easily,
        // so we handle rate limiting manually
        tokio::time::sleep(API_RATE_LIMIT_DELAY).await;

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.jwt)
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        let result: UploadResponse = response.json().await?;
        
        if result.data.is_duplicate {
            debug!("File already exists with CID {}", result.data.cid);
        } else {
            info!("Uploaded {} -> CID {}", display_name, result.data.cid);
        }

        Ok(result.data)
    }

    /// Upload multiple files to Pinata with progress reporting.
    ///
    /// # Arguments
    /// * `files` - Slice of (path, name) pairs to upload
    /// * `group_id` - Optional group ID to add files to
    /// * `on_progress` - Optional callback called with (completed, total, uploaded, duplicates, failed)
    ///
    /// # Returns
    /// Vector of results for each file (Ok with UploadedFile or Err with error message).
    pub async fn upload_files(
        &self,
        files: &[(std::path::PathBuf, String)],
        group_id: Option<&str>,
        on_progress: Option<Box<dyn Fn(usize, usize, usize, usize, usize) + Send + Sync>>,
    ) -> Vec<Result<UploadedFile, String>> {
        let total = files.len();
        let mut results = Vec::with_capacity(total);
        let mut uploaded = 0usize;
        let mut duplicates = 0usize;
        let mut failed = 0usize;

        for (i, (path, name)) in files.iter().enumerate() {
            let result = self.upload_file(path, Some(name), group_id).await;

            match &result {
                Ok(uploaded_file) => {
                    if uploaded_file.is_duplicate {
                        duplicates += 1;
                    } else {
                        uploaded += 1;
                    }
                    results.push(Ok(uploaded_file.clone()));
                }
                Err(e) => {
                    failed += 1;
                    results.push(Err(format!("{}: {}", path.display(), e)));
                }
            }

            if let Some(ref callback) = on_progress {
                callback(i + 1, total, uploaded, duplicates, failed);
            }
        }

        results
    }
    /// Ensure all CIDs are pinned to a group.
    ///
    /// Takes a slice of (name, cid) pairs. The name is used for display in Pinata's UI.
    ///
    /// For each CID:
    /// 1. If already in group → skip
    /// 2. If pinned elsewhere in account → add to group
    /// 3. If not pinned → pin with group_id
    ///
    /// Rate limited to respect Pinata's 180 requests/minute limit.
    /// Returns the number of newly pinned CIDs.
    pub async fn ensure_cids_pinned(
        &self,
        group_id: &str,
        items: &[(String, String)], // (name, cid)
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<usize, PinataError> {
        // Cancel any backfilled requests from previous runs to start fresh
        let cancelled = self.cancel_backfilled_pins().await?;
        if cancelled > 0 {
            eprintln!(
                "  Cancelled {} backfilled pin requests from previous run",
                cancelled
            );
        }

        // Get existing files in group
        eprint!("  Fetching files already in group...");
        let existing_files = self.list_files_in_group(group_id).await?;
        eprintln!(" {} files", existing_files.len());
        info!(
            "Group {} has {} files already",
            group_id,
            existing_files.len()
        );

        let group_cids: std::collections::HashSet<_> =
            existing_files.iter().map(|f| f.cid.as_str()).collect();

        // Get ungrouped files - these are pinned but not in any group yet
        eprint!("  Fetching ungrouped files...");
        let ungrouped_files = self.list_ungrouped_files().await?;
        eprintln!(" {} files", ungrouped_files.len());
        info!(
            "Account has {} ungrouped pinned files",
            ungrouped_files.len()
        );

        let cid_to_file_id: std::collections::HashMap<&str, &str> = ungrouped_files
            .iter()
            .map(|f| (f.cid.as_str(), f.id.as_str()))
            .collect();

        // Get CIDs already in the pin queue (in-flight)
        let pending_jobs = self.query_pin_requests(None).await?;
        let backfilled_count = pending_jobs
            .iter()
            .filter(|j| j.status == "backfilled")
            .count();
        let active_count = pending_jobs.len() - backfilled_count;

        info!(
            "Found {} pending pin requests ({} active, {} backfilled)",
            pending_jobs.len(),
            active_count,
            backfilled_count
        );

        // If there are backfilled items OR we're near the active limit, wait for capacity
        let should_wait = backfilled_count > 0 || active_count >= PIN_QUEUE_CAPACITY;

        if should_wait {
            let reason = if backfilled_count > 0 {
                format!("{} backfilled", backfilled_count)
            } else {
                format!("{} active (threshold {})", active_count, PIN_QUEUE_CAPACITY)
            };
            info!(
                "Pin queue is busy ({}). Waiting for queue to drain before adding more...",
                reason
            );
            eprintln!(
                "  Pinata queue busy ({}). Waiting for queue to drain...",
                reason
            );

            // Wait for queue to have capacity
            let poll_interval = Duration::from_secs(30);
            loop {
                tokio::time::sleep(poll_interval).await;

                let current_jobs = self.query_pin_requests(None).await?;
                let current_backfilled = current_jobs
                    .iter()
                    .filter(|j| j.status == "backfilled")
                    .count();
                let current_active = current_jobs.len() - current_backfilled;

                info!(
                    "Queue status: {} total ({} active, {} backfilled)",
                    current_jobs.len(),
                    current_active,
                    current_backfilled
                );
                eprint!(
                    "\r  Queue status: {} active, {} backfilled    ",
                    current_active, current_backfilled
                );

                // Resume when backfilled is 0 AND active is below threshold
                if current_backfilled == 0 && current_active < PIN_QUEUE_CAPACITY {
                    info!(
                        "Queue has capacity ({} active), resuming...",
                        current_active
                    );
                    eprintln!(
                        "\n  Queue has capacity ({} active), resuming...",
                        current_active
                    );
                    break;
                }
            }

            // Re-fetch pending jobs after waiting
            let pending_jobs = self.query_pin_requests(None).await?;
            let pending_cids: std::collections::HashSet<_> =
                pending_jobs.iter().map(|j| j.cid.as_str()).collect();

            // Re-filter to_process
            let to_process: Vec<_> = items
                .iter()
                .filter(|(_, cid)| {
                    !group_cids.contains(cid.as_str()) && !pending_cids.contains(cid.as_str())
                })
                .collect();

            info!(
                "Group has {} files, {} in-flight, {} to process",
                group_cids.len(),
                pending_cids.len(),
                to_process.len()
            );

            if to_process.is_empty() {
                return Ok(0);
            }
        }

        let pending_cids: std::collections::HashSet<_> =
            pending_jobs.iter().map(|j| j.cid.as_str()).collect();

        // Filter to CIDs not already in group and not in-flight
        let to_process: Vec<_> = items
            .iter()
            .filter(|(_, cid)| {
                !group_cids.contains(cid.as_str()) && !pending_cids.contains(cid.as_str())
            })
            .collect();

        info!(
            "Group has {} files, {} in-flight, {} to process",
            group_cids.len(),
            pending_cids.len(),
            to_process.len()
        );

        if to_process.is_empty() {
            return Ok(0);
        }

        let delay = API_RATE_LIMIT_DELAY;
        let mut pinned = 0;
        let mut added_to_group = 0;
        let mut failed = 0;
        let total = to_process.len();

        for (i, (name, cid)) in to_process.iter().enumerate() {
            // Check if file is already pinned in account
            if let Some(file_id) = cid_to_file_id.get(cid.as_str()) {
                // File exists in account, add to group
                match self.add_file_to_group(file_id, group_id).await {
                    Ok(()) => {
                        added_to_group += 1;
                        info!("Added {} / {} to group", name, cid);
                    }
                    Err(e) => {
                        warn!("Failed to add {} / {} to group: {}", name, cid, e);
                        failed += 1;
                    }
                }
            } else {
                // Before pinning, check if queue is getting too long (every 10 pins)
                if pinned > 0 && pinned % 10 == 0 {
                    if let Ok(queue_full) = self.is_pin_queue_full().await {
                        if queue_full {
                            info!("Pin queue has >100 pending requests, pausing...");
                            eprintln!("\n  Pin queue has >100 pending requests, pausing...");
                            self.wait_for_queue_capacity().await?;
                        }
                    }
                }

                // File not pinned yet, need to pin it
                match self.pin_by_cid(cid, Some(name), Some(group_id)).await {
                    Ok(status) => {
                        if status.status == "prechecking" || status.status == "pinned" {
                            pinned += 1;
                            info!("Pinned {} / {} (status: {})", name, cid, status.status);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to pin {} / {}: {}", name, cid, e);
                        failed += 1;
                    }
                }
            }

            // Report progress
            if let Some(callback) = on_progress {
                if (i + 1) % 50 == 0 || i + 1 == total {
                    callback(i + 1, total);
                }
            }

            // Rate limit delay (skip on last item)
            if i + 1 < total {
                tokio::time::sleep(delay).await;
            }
        }

        if failed > 0 {
            warn!("Failed to process {} CIDs", failed);
        }

        info!(
            "Processed group {}: {} added to group, {} newly pinned",
            group_id, added_to_group, pinned
        );
        Ok(pinned)
    }

    /// Cancel a pin-by-CID request.
    pub async fn cancel_pin_request(&self, request_id: &str) -> Result<(), PinataError> {
        let url = format!("{}/files/public/pin_by_cid/{}", PINATA_API_BASE, request_id);

        debug!("Cancelling pin request: {}", request_id);

        let response = self
            .send_with_retry(self.client.delete(&url).bearer_auth(&self.jwt))
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        Ok(())
    }

    /// Cancel all backfilled pin requests.
    /// Leaves actively processing requests (searching, retrieving, prechecking) alone.
    pub async fn cancel_backfilled_pins(&self) -> Result<usize, PinataError> {
        let jobs = self.query_pin_requests(None).await?;

        let to_cancel: Vec<_> = jobs.iter().filter(|j| j.status == "backfilled").collect();

        info!("Cancelling {} backfilled pin requests", to_cancel.len());

        let mut cancelled = 0;
        for (i, job) in to_cancel.iter().enumerate() {
            match self.cancel_pin_request(&job.id).await {
                Ok(()) => {
                    cancelled += 1;
                    info!(
                        "Cancelled pin request {} ({})",
                        job.id,
                        job.name.as_deref().unwrap_or("?")
                    );
                }
                Err(e) => {
                    warn!("Failed to cancel pin request {}: {}", job.id, e);
                }
            }

            if i + 1 < to_cancel.len() {
                tokio::time::sleep(API_RATE_LIMIT_DELAY).await;
            }
        }

        info!("Cancelled {} pin requests", cancelled);
        Ok(cancelled)
    }

    /// Cancel ALL pending pin requests (all statuses).
    /// Use this to completely clear the pin queue.
    pub async fn cancel_all_pins(&self) -> Result<usize, PinataError> {
        let jobs = self.query_pin_requests(None).await?;

        if jobs.is_empty() {
            info!("No pending pin requests to cancel");
            return Ok(0);
        }

        info!("Cancelling {} pin requests (all statuses)", jobs.len());

        let mut cancelled = 0;
        let mut failed = 0;

        for (i, job) in jobs.iter().enumerate() {
            match self.cancel_pin_request(&job.id).await {
                Ok(()) => {
                    cancelled += 1;
                    debug!(
                        "Cancelled pin request {} ({}) [{}]",
                        job.id,
                        job.name.as_deref().unwrap_or("?"),
                        job.status
                    );
                }
                Err(e) => {
                    warn!("Failed to cancel pin request {}: {}", job.id, e);
                    failed += 1;
                }
            }

            if i + 1 < jobs.len() {
                tokio::time::sleep(API_RATE_LIMIT_DELAY).await;
            }
        }

        info!("Cancelled {} pin requests ({} failed)", cancelled, failed);
        Ok(cancelled)
    }

    /// Check if pin queue has more than PIN_QUEUE_CAPACITY pending requests.
    async fn is_pin_queue_full(&self) -> Result<bool, PinataError> {
        let jobs = self.query_pin_requests(None).await?;
        info!(
            "is_pin_queue_full: {} jobs (capacity: {})",
            jobs.len(),
            PIN_QUEUE_CAPACITY
        );
        Ok(jobs.len() >= PIN_QUEUE_CAPACITY)
    }

    /// Wait for pin queue to have capacity (<PIN_QUEUE_CAPACITY pending requests).
    async fn wait_for_queue_capacity(&self) -> Result<(), PinataError> {
        let poll_interval = Duration::from_secs(30);

        loop {
            tokio::time::sleep(poll_interval).await;

            let jobs = self.query_pin_requests(None).await?;
            info!("Queue check: {} pending jobs", jobs.len());

            if jobs.len() < PIN_QUEUE_CAPACITY {
                eprintln!(
                    "\r  Queue has capacity ({} pending), resuming...    ",
                    jobs.len()
                );
                info!(
                    "Pin queue has capacity ({} pending), resuming...",
                    jobs.len()
                );
                break;
            } else {
                eprint!(
                    "\r  Waiting for queue capacity ({} pending)...    ",
                    jobs.len()
                );
            }
        }

        Ok(())
    }

    /// Fetch a thumbnail image from Pinata with optimization.
    ///
    /// Returns image bytes at the specified size. The format may vary based on the
    /// source image - Pinata may not always convert formats successfully.
    /// Returns (bytes, extension) where extension is "png", "webp", "jpg", or "gif".
    pub async fn fetch_thumbnail(
        &self,
        cid: &str,
        size: u32,
    ) -> Result<(Vec<u8>, &'static str), PinataError> {
        // Request PNG but accept whatever we get back
        let url = self.image_url(cid, size, "png").ok_or_else(|| {
            PinataError::InvalidResponse("No gateway host configured".to_string())
        })?;

        debug!("Fetching thumbnail: {}", url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        let bytes = response.bytes().await?;

        // Detect format from magic bytes
        let ext = if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            "png"
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            "jpg"
        } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            "webp"
        } else if bytes.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
            "gif"
        } else {
            return Err(PinataError::InvalidResponse(
                "Response is not a recognized image format".to_string(),
            ));
        };

        Ok((bytes.to_vec(), ext))
    }

    /// Query pin requests (jobs in the pinning queue).
    ///
    /// Returns all pending pin jobs, optionally filtered by status.
    pub async fn query_pin_requests(
        &self,
        status_filter: Option<&str>,
    ) -> Result<Vec<PinJob>, PinataError> {
        let mut all_jobs = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!("{}/files/public/pin_by_cid?limit=100", PINATA_API_BASE);

            if let Some(status) = status_filter {
                url = format!("{}&status={}", url, status);
            }

            if let Some(ref token) = page_token {
                url = format!("{}&pageToken={}", url, token);
            }

            debug!("Querying pin requests: {}", url);

            let response = self
                .send_with_retry(self.client.get(&url).bearer_auth(&self.jwt))
                .await?;

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let message = response.text().await.unwrap_or_default();
                return Err(PinataError::Api { status, message });
            }

            let text = response.text().await?;
            let result: PinRequestsResponse = serde_json::from_str(&text).map_err(|e| {
                PinataError::InvalidResponse(format!("Failed to parse pin requests: {}", e))
            })?;
            let jobs_in_page = result.data.jobs.len();
            debug!(
                "Pin requests page: {} jobs, next_page_token: {:?}",
                jobs_in_page, result.data.next_page_token
            );
            all_jobs.extend(result.data.jobs);

            // Only continue pagination if we got results
            // Pinata sometimes returns next_page_token even with empty results
            match result.data.next_page_token {
                Some(token) if !token.is_empty() && jobs_in_page > 0 => page_token = Some(token),
                _ => break,
            }
        }

        Ok(all_jobs)
    }

    /// Wait for all pin requests for the given CIDs to complete.
    ///
    /// Polls the pin queue until none of our CIDs are in a pending state.
    /// Returns the CIDs that failed to pin.
    pub async fn wait_for_pins(
        &self,
        cids: &[String],
        on_progress: Option<&dyn Fn(usize, usize, &str)>,
    ) -> Result<Vec<String>, PinataError> {
        let cid_set: std::collections::HashSet<&str> = cids.iter().map(|s| s.as_str()).collect();
        let total = cids.len();
        let poll_interval = Duration::from_secs(10);
        let mut failed_cids = Vec::new();

        // Statuses that indicate work in progress
        // "backfilled" means queued but waiting - Pinata can only process 250 at a time
        let pending_statuses = ["prechecking", "searching", "retrieving", "backfilled"];
        // Statuses that indicate failure
        let failed_statuses = [
            "expired",
            "over_free_limit",
            "over_max_size",
            "invalid_object",
            "bad_host_node",
        ];

        loop {
            // Get all pin jobs (no status filter to see everything)
            let jobs = self.query_pin_requests(None).await?;

            // Filter to our CIDs
            let our_jobs: Vec<_> = jobs
                .iter()
                .filter(|j| cid_set.contains(j.cid.as_str()))
                .collect();

            // Count pending vs completed
            let pending: Vec<_> = our_jobs
                .iter()
                .filter(|j| pending_statuses.contains(&j.status.as_str()))
                .collect();

            // Track failures
            for job in &our_jobs {
                if failed_statuses.contains(&job.status.as_str()) {
                    if !failed_cids.contains(&job.cid) {
                        warn!("Pin failed for CID {}: {}", job.cid, job.status);
                        failed_cids.push(job.cid.clone());
                    }
                }
            }

            let completed = total - pending.len() - failed_cids.len();

            if let Some(callback) = on_progress {
                let status_msg = if pending.is_empty() {
                    "complete".to_string()
                } else {
                    // Show what statuses we're waiting on
                    let mut status_counts: std::collections::HashMap<&str, usize> =
                        std::collections::HashMap::new();
                    for job in &pending {
                        *status_counts.entry(&job.status).or_insert(0) += 1;
                    }
                    status_counts
                        .iter()
                        .map(|(s, c)| format!("{}: {}", s, c))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                callback(completed, total, &status_msg);
            }

            if pending.is_empty() {
                info!(
                    "All pins complete: {} succeeded, {} failed",
                    total - failed_cids.len(),
                    failed_cids.len()
                );
                break;
            }

            debug!("Waiting for {} pins to complete...", pending.len());
            tokio::time::sleep(poll_interval).await;
        }

        Ok(failed_cids)
    }
}
