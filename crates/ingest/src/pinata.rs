//! Pinata API client for image pinning and serving.
//!
//! Provides group management and CID pinning functionality for collections
//! that want guaranteed availability via Pinata's infrastructure.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn};

const PINATA_API_BASE: &str = "https://api.pinata.cloud/v3";

/// Pinata API client.
#[derive(Clone)]
pub struct PinataClient {
    client: Client,
    jwt: String,
    gateway_host: Option<String>,
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

impl PinataClient {
    /// Create a new Pinata client from environment variables.
    ///
    /// Requires `PINATA_API_JWT` to be set.
    /// Optionally uses `PINATA_GATEWAY_HOST` for optimized image URLs.
    pub fn from_env() -> Result<Self, PinataError> {
        let jwt = std::env::var("PINATA_API_JWT").map_err(|_| PinataError::MissingJwt)?;

        let gateway_host = std::env::var("PINATA_GATEWAY_HOST").ok();

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Ok(Self {
            client,
            jwt,
            gateway_host,
        })
    }

    /// Get the gateway host for image URLs.
    pub fn gateway_host(&self) -> Option<&str> {
        self.gateway_host.as_deref()
    }

    /// Build an optimized image URL for a CID.
    ///
    /// Uses Pinata's image optimization parameters.
    pub fn image_url(&self, cid: &str, width: u32, format: &str) -> Option<String> {
        let host = self.gateway_host.as_ref()?;
        Some(format!(
            "https://{}/ipfs/{}?img-width={}&img-format={}",
            host, cid, width, format
        ))
    }

    /// List groups, optionally filtering by name.
    pub async fn list_groups(&self, name_filter: Option<&str>) -> Result<Vec<Group>, PinataError> {
        let mut url = format!("{}/groups/public", PINATA_API_BASE);

        if let Some(name) = name_filter {
            url = format!("{}?name={}", url, urlencoding::encode(name));
        }

        debug!("Listing Pinata groups: {}", url);

        let response = self.client.get(&url).bearer_auth(&self.jwt).send().await?;

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

    /// Create a new group.
    pub async fn create_group(&self, name: &str) -> Result<Group, PinataError> {
        let url = format!("{}/groups/public", PINATA_API_BASE);

        info!("Creating Pinata group: {}", name);

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.jwt)
            .json(&CreateGroupRequest {
                name: name.to_string(),
            })
            .send()
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

    /// List files in a group.
    pub async fn list_files_in_group(
        &self,
        group_id: &str,
    ) -> Result<Vec<PinataFile>, PinataError> {
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!("{}/files/public?group={}", PINATA_API_BASE, group_id);

            if let Some(ref token) = page_token {
                url = format!("{}&pageToken={}", url, token);
            }

            debug!("Listing files in group: {}", url);

            let response = self.client.get(&url).bearer_auth(&self.jwt).send().await?;

            if !response.status().is_success() {
                let status = response.status().as_u16();
                let message = response.text().await.unwrap_or_default();
                return Err(PinataError::Api { status, message });
            }

            let result: FilesListResponse = response.json().await?;
            all_files.extend(result.data.files);

            match result.data.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }

        Ok(all_files)
    }

    /// Check if a CID is already pinned (anywhere in account).
    pub async fn find_file_by_cid(&self, cid: &str) -> Result<Option<PinataFile>, PinataError> {
        let url = format!("{}/files/public?cid={}", PINATA_API_BASE, cid);

        let response = self.client.get(&url).bearer_auth(&self.jwt).send().await?;

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

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.jwt)
            .json(&PinByCidRequest {
                cid: cid.to_string(),
                name: name.map(|s| s.to_string()),
                group_id: group_id.map(|s| s.to_string()),
            })
            .send()
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

        let response = self.client.put(&url).bearer_auth(&self.jwt).send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response.text().await.unwrap_or_default();
            return Err(PinataError::Api { status, message });
        }

        Ok(())
    }

    /// Ensure all CIDs are pinned to a group.
    ///
    /// Returns the number of newly pinned CIDs.
    pub async fn ensure_cids_pinned(
        &self,
        group: &Group,
        cids: &[String],
    ) -> Result<usize, PinataError> {
        // Get existing files in group
        let existing_files = self.list_files_in_group(&group.id).await?;
        let existing_cids: std::collections::HashSet<_> =
            existing_files.iter().map(|f| f.cid.as_str()).collect();

        info!(
            "Group {} has {} existing files, need to check {} CIDs",
            group.name,
            existing_cids.len(),
            cids.len()
        );

        let mut pinned = 0;
        for cid in cids {
            if existing_cids.contains(cid.as_str()) {
                continue;
            }

            // Check if already pinned elsewhere in account
            if let Some(file) = self.find_file_by_cid(cid).await? {
                // Already pinned, just add to group
                if file.group_id.as_deref() != Some(&group.id) {
                    self.add_file_to_group(&file.id, &group.id).await?;
                    info!("Added existing file {} to group", cid);
                }
            } else {
                // Pin new CID
                match self.pin_by_cid(cid, None, Some(&group.id)).await {
                    Ok(_) => {
                        info!("Pinned new CID: {}", cid);
                        pinned += 1;
                    }
                    Err(e) => {
                        warn!("Failed to pin CID {}: {}", cid, e);
                    }
                }
            }
        }

        Ok(pinned)
    }

    /// Fetch a thumbnail image from Pinata with optimization.
    ///
    /// Returns PNG bytes at the specified size.
    pub async fn fetch_thumbnail(&self, cid: &str, size: u32) -> Result<Vec<u8>, PinataError> {
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

        // Validate it's a PNG
        if !bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            return Err(PinataError::InvalidResponse(
                "Response is not a PNG image".to_string(),
            ));
        }

        Ok(bytes.to_vec())
    }
}
