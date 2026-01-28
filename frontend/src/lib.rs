//! # NFT Collection Viewer Frontend
//!
//! A Leptos frontend for viewing NFT collections with trait filtering.
//! Supports HCF range requests for full-resolution images.
//!
//! Features:
//! - Gallery view with lazy-loaded sprite thumbnails
//! - Trait-based filtering using bitmap operations
//! - Detail view with full image and trait information

mod components;
mod pages;

use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::components::*;
use leptos_router::path;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

pub use components::*;
pub use pages::*;

/// Base URL for fetching collection assets from R2
pub const FILES_BASE_URL: &str = "https://files.hodlcroft.com/collections";

/// Sprite sheet constants (must match ingestion pipeline)
pub const SPRITE_THUMB_SIZE: u32 = 256;
pub const SPRITE_COLUMNS: u32 = 4;
pub const SPRITE_ROWS: u32 = 4;
pub const SPRITES_PER_SHEET: u32 = SPRITE_COLUMNS * SPRITE_ROWS; // 16

/// Extract build hash from the loaded WASM filename (e.g., "viewer-frontend-1c27406143753b04_bg.wasm")
fn get_build_hash() -> String {
    let href = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector("link[href*='_bg.wasm']").ok().flatten())
        .and_then(|el| el.get_attribute("href"));

    if let Some(href) = href {
        // Extract hash from "/viewer-frontend-1c27406143753b04_bg.wasm"
        if let (Some(start), Some(end)) = (href.rfind('-'), href.find("_bg.wasm")) {
            if start < end {
                return href[start + 1..end].to_string();
            }
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

use std::sync::OnceLock;
static BUILD_HASH: OnceLock<String> = OnceLock::new();

/// Get cached build hash
pub fn build_hash() -> &'static str {
    BUILD_HASH.get_or_init(get_build_hash)
}

/// Build URL for collection assets
/// Adds cache-busting version parameter to .bin files
pub fn collection_url(slug: &str, path: &str) -> String {
    let base = format!("{FILES_BASE_URL}/{slug}/{path}");
    if path.ends_with(".bin") {
        format!("{base}?v={}", build_hash())
    } else {
        base
    }
}

/// HCF metadata for full image access
#[derive(Clone, Debug)]
pub struct HcfInfo {
    /// Shard size in bytes
    pub shard_size: u32,
    /// Number of shards
    pub shard_count: u16,
    /// Image format (webp, png, avif)
    pub image_format: String,
    /// Content type for the format
    pub content_type: String,
}

/// HCF location for a token's full image
#[derive(Clone, Debug)]
pub struct HcfLocation {
    /// Which shard file (0 = images_000.hcf)
    pub shard: u16,
    /// Byte offset within the shard
    pub offset: u64,
    /// Byte length of the image
    pub length: u32,
}

/// Parsed collection data from collection.bin
#[derive(Clone)]
pub struct CollectionData {
    /// Collection slug
    pub slug: String,
    /// Raw binary data (kept for zero-copy access)
    pub raw: Arc<Vec<u8>>,
    /// Parsed header info
    pub token_count: u32,
    pub trait_count: u8,
    /// Whether to hide rarity rankings in the UI
    pub hide_rarity: bool,
    /// Sprite metadata
    pub sprite_thumb_width: u16,
    pub sprite_thumb_height: u16,
    pub sprite_grid_columns: u8,
    pub sprite_grid_rows: u8,
    pub sprite_sheet_count: u16,
    /// HCF metadata (if available)
    pub hcf: Option<HcfInfo>,
    /// Trait names and values (resolved from string table)
    pub traits: Vec<TraitInfo>,
    /// Token data
    pub tokens: Vec<TokenInfo>,
}

/// Information about a trait
#[derive(Clone, Debug)]
pub struct TraitInfo {
    pub name: String,
    pub values: Vec<TraitValueInfo>,
}

/// Information about a trait value
#[derive(Clone, Debug)]
pub struct TraitValueInfo {
    pub value: String,
    pub count: u16,
    pub bit_index: u16,
}

/// Information about a token
#[derive(Clone, Debug)]
pub struct TokenInfo {
    pub index: u32,
    pub name: String,
    pub asset_id: String,
    pub rarity_rank: u16,
    pub sprite_sheet: u16,
    pub sprite_x: u8,
    pub sprite_y: u8,
    /// Bitmap of trait values this token has
    pub trait_bitmap: Vec<u8>,
    /// HCF location for full image (if available)
    pub hcf_location: Option<HcfLocation>,
}

impl CollectionData {
    /// Parse collection.bin data
    pub fn parse(slug: String, data: Vec<u8>) -> Result<Self, String> {
        use viewer_binary::{HEADER_SIZE, Header, HcfMetadata, MAGIC, SpriteMetadata, TraitSchema};

        if data.len() < HEADER_SIZE {
            return Err("File too small for header".to_string());
        }

        // Parse header
        let header_bytes: [u8; HEADER_SIZE] = data[..HEADER_SIZE]
            .try_into()
            .map_err(|_| "Failed to read header")?;
        let header = Header::from_bytes(&header_bytes);

        // Validate magic
        if header.magic != MAGIC {
            return Err("Invalid magic bytes".to_string());
        }

        // Get string table (skip 4-byte length prefix)
        let string_table_start = header.string_table_offset as usize + 4;
        let string_table_end = header.trait_schema_offset as usize;
        let string_table_data = &data[string_table_start..string_table_end];

        // Parse trait schema
        let trait_schema_start = header.trait_schema_offset as usize;
        let trait_schema_end = header.trait_index_offset as usize;
        let trait_schema = TraitSchema::from_bytes(&data[trait_schema_start..trait_schema_end])
            .ok_or("Failed to parse trait schema")?;

        // Resolve trait names and values from string table
        let traits: Vec<TraitInfo> = trait_schema
            .traits
            .iter()
            .map(|t| {
                let name = read_string(string_table_data, t.name.0);
                let values = t
                    .values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| TraitValueInfo {
                        value: read_string(string_table_data, v.name.0),
                        count: v.count,
                        bit_index: t.bitmap_offset + i as u16,
                    })
                    .collect();
                TraitInfo { name, values }
            })
            .collect();

        // Parse sprite metadata
        let sprites_start = header.sprites_offset as usize;
        let sprite_bytes: [u8; 12] = data[sprites_start..sprites_start + 12]
            .try_into()
            .map_err(|_| "Failed to read sprite metadata")?;
        let sprite_meta = SpriteMetadata::from_bytes(&sprite_bytes);

        // Parse HCF metadata (if present)
        let hcf_info = if header.hcf_metadata_offset > 0 {
            let hcf_start = header.hcf_metadata_offset as usize;
            let hcf_bytes: [u8; 12] = data[hcf_start..hcf_start + 12]
                .try_into()
                .map_err(|_| "Failed to read HCF metadata")?;
            if let Some(hcf_meta) = HcfMetadata::from_bytes(&hcf_bytes) {
                tracing::info!(
                    shard_size = hcf_meta.shard_size,
                    shard_count = hcf_meta.shard_count,
                    image_format = ?hcf_meta.image_format,
                    "Parsed HCF metadata"
                );
                Some(HcfInfo {
                    shard_size: hcf_meta.shard_size,
                    shard_count: hcf_meta.shard_count,
                    image_format: hcf_meta.image_format.extension().to_string(),
                    content_type: hcf_meta.image_format.content_type().to_string(),
                })
            } else {
                None
            }
        } else {
            None
        };

        // Get HCF index size for reading locations
        let hcf_index_size = header.hcf_index_size();
        let hcf_index_start = header.hcf_index_offset as usize;

        // Parse tokens
        // Binary format (from tokens.rs):
        // - sprite_sheet: u16 (bytes 0-1)
        // - sprite_x: u8 (byte 2)
        // - sprite_y: u8 (byte 3)
        // - rarity_rank: u16 (bytes 4-5)
        // - rarity_score: u16 (bytes 6-7)
        // - name_ref: u16 (bytes 8-9)
        // - bitmap: N bytes
        let bitmap_size = header.bitmap_size().ok_or("Invalid bitmap size")?;
        let bitmap_bytes = bitmap_size.byte_size();
        let token_table_start = header.token_table_offset as usize;

        let token_fixed_size = 10; // 2+1+1+2+2+2 = 10 bytes fixed
        let token_entry_size = token_fixed_size + bitmap_bytes;

        // Get asset IDs from the asset ID index
        let asset_id_index_start = header.asset_id_index_offset as usize;

        let mut tokens = Vec::with_capacity(header.token_count as usize);
        for i in 0..header.token_count {
            let offset = token_table_start + (i as usize) * token_entry_size;
            let entry = &data[offset..offset + token_entry_size];

            let sprite_sheet = u16::from_le_bytes([entry[0], entry[1]]);
            let sprite_x = entry[2];
            let sprite_y = entry[3];
            let rarity_rank = u16::from_le_bytes([entry[4], entry[5]]);
            let _rarity_score = u16::from_le_bytes([entry[6], entry[7]]);
            let name_ref = u16::from_le_bytes([entry[8], entry[9]]);
            let trait_bitmap = entry[10..10 + bitmap_bytes].to_vec();

            // Read asset ID from asset ID index
            // Format: [offset_table: u32 * token_count][string_data]
            // Each offset (relative to asset_id_index_start) points to a null-terminated string
            let offset_entry = asset_id_index_start + (i as usize) * 4;
            let asset_id_str_offset = u32::from_le_bytes([
                data[offset_entry],
                data[offset_entry + 1],
                data[offset_entry + 2],
                data[offset_entry + 3],
            ]) as usize;
            let asset_id = read_string_at(&data[asset_id_index_start..], asset_id_str_offset);

            // Resolve name - high bit indicates custom name vs pattern
            let name = if name_ref & 0x8000 != 0 {
                // Custom name from string table
                read_string(string_table_data, (name_ref & 0x7FFF) as u32)
            } else {
                // Pattern: "#{n}" where n is the name_ref value
                format!("#{}", name_ref)
            };

            // Read HCF location if available
            let hcf_location = if let (Some(hcf), Some(idx_size)) = (&hcf_info, hcf_index_size) {
                let entry_size = idx_size.byte_size();
                let loc_offset = hcf_index_start + (i as usize) * entry_size;
                let loc_bytes = &data[loc_offset..loc_offset + entry_size];
                let (global_offset, length) = viewer_binary::read_hcf_location(loc_bytes, idx_size);

                // Calculate which shard based on global offset
                let shard = (global_offset / hcf.shard_size as u64) as u16;
                let offset_in_shard = global_offset % hcf.shard_size as u64;

                // Log first few tokens for debugging
                if i < 3 {
                    tracing::info!(
                        token_index = i,
                        global_offset = global_offset,
                        shard_size = hcf.shard_size,
                        calculated_shard = shard,
                        offset_in_shard = offset_in_shard,
                        length = length,
                        "Parsed HCF location"
                    );
                }

                Some(HcfLocation {
                    shard,
                    offset: offset_in_shard,
                    length,
                })
            } else {
                None
            };

            tokens.push(TokenInfo {
                index: i,
                name,
                asset_id,
                rarity_rank,
                sprite_sheet,
                sprite_x,
                sprite_y,
                trait_bitmap,
                hcf_location,
            });
        }

        Ok(CollectionData {
            slug,
            raw: Arc::new(data),
            token_count: header.token_count,
            trait_count: header.trait_count,
            hide_rarity: header.hide_rarity(),
            sprite_thumb_width: sprite_meta.thumb_width,
            sprite_thumb_height: sprite_meta.thumb_height,
            sprite_grid_columns: sprite_meta.grid_columns,
            sprite_grid_rows: sprite_meta.grid_rows,
            sprite_sheet_count: sprite_meta.sheet_count,
            hcf: hcf_info,
            traits,
            tokens,
        })
    }

    /// Check if a token matches the given filter bitmap
    pub fn token_matches_filter(&self, token: &TokenInfo, filter_bitmap: &[u8]) -> bool {
        // Token matches if it has ALL bits set that are in the filter
        // (filter_bitmap & token_bitmap) == filter_bitmap
        if filter_bitmap.len() != token.trait_bitmap.len() {
            return false;
        }
        for (f, t) in filter_bitmap.iter().zip(token.trait_bitmap.iter()) {
            if (f & t) != *f {
                return false;
            }
        }
        true
    }

    /// Get images per sprite sheet
    pub fn images_per_sheet(&self) -> u32 {
        self.sprite_grid_columns as u32 * self.sprite_grid_rows as u32
    }
}

/// Read a null-terminated string from the string table
fn read_string(table: &[u8], offset: u32) -> String {
    read_string_at(table, offset as usize)
}

/// Read a null-terminated string at a given byte offset
fn read_string_at(data: &[u8], offset: usize) -> String {
    if offset >= data.len() {
        return String::new();
    }
    let end = data[offset..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| offset + p)
        .unwrap_or(data.len());
    String::from_utf8_lossy(&data[offset..end]).to_string()
}

/// Shared cache for collection data
#[derive(Clone, Default)]
pub struct CollectionCache {
    cache: std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, CollectionData>>>,
}

impl CollectionCache {
    pub fn get(&self, slug: &str) -> Option<CollectionData> {
        self.cache.read().ok()?.get(slug).cloned()
    }

    pub fn set(&self, slug: &str, data: CollectionData) {
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(slug.to_string(), data);
        }
    }
}

/// Fetch and parse collection data
pub async fn fetch_collection(
    slug: &str,
    cache: &CollectionCache,
) -> Result<CollectionData, String> {
    // Check cache first
    if let Some(data) = cache.get(slug) {
        return Ok(data);
    }

    // Fetch collection.bin
    let url = collection_url(slug, "collection.bin");
    let response = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch: {e}"))?;

    if !response.ok() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response
        .binary()
        .await
        .map_err(|e| format!("Failed to read body: {e}"))?;

    // Parse binary data
    let data = CollectionData::parse(slug.to_string(), bytes)?;

    // Cache it
    cache.set(slug, data.clone());

    Ok(data)
}

/// Fetch a full-resolution image from the HCF bundle using a range request.
/// Returns an object URL that can be used as an img src.
/// Optionally accepts an AbortSignal for cancellation.
pub async fn fetch_hcf_image(
    slug: &str,
    hcf: &HcfInfo,
    location: &HcfLocation,
) -> Result<String, String> {
    fetch_hcf_image_with_signal(slug, hcf, location, None).await
}

/// Fetch a full-resolution image from the HCF bundle with optional abort signal.
pub async fn fetch_hcf_image_with_signal(
    slug: &str,
    hcf: &HcfInfo,
    location: &HcfLocation,
    abort_signal: Option<&web_sys::AbortSignal>,
) -> Result<String, String> {
    // Build the URL to the correct shard
    let shard_url = format!(
        "{}/{}/hcf/images_{:03}.hcf",
        FILES_BASE_URL, slug, location.shard
    );

    // Calculate byte range (inclusive end)
    let range_start = location.offset;
    let range_end = location.offset + location.length as u64 - 1;
    let range_header = format!("bytes={}-{}", range_start, range_end);

    tracing::info!(
        shard_url = %shard_url,
        range = %range_header,
        offset = location.offset,
        length = location.length,
        shard = location.shard,
        shard_size = hcf.shard_size,
        shard_count = hcf.shard_count,
        content_type = %hcf.content_type,
        "Fetching HCF image"
    );

    // Fetch with range header and optional abort signal
    let mut request = gloo_net::http::Request::get(&shard_url)
        .header("Range", &range_header);

    if let Some(signal) = abort_signal {
        request = request.abort_signal(Some(signal));
    }

    let response = request
        .send()
        .await
        .map_err(|e| {
            // Check if this was an abort
            if e.to_string().contains("abort") {
                return "Request aborted".to_string();
            }
            format!("Failed to fetch image: {e}")
        })?;

    tracing::info!(
        status = response.status(),
        "HCF fetch response"
    );

    // Should get 206 Partial Content
    if response.status() != 206 && response.status() != 200 {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response
        .binary()
        .await
        .map_err(|e| format!("Failed to read image body: {e}"))?;

    tracing::info!(
        bytes_len = bytes.len(),
        expected_len = location.length,
        first_bytes = format!("{:02x}{:02x}{:02x}{:02x}",
            bytes.get(0).copied().unwrap_or(0),
            bytes.get(1).copied().unwrap_or(0),
            bytes.get(2).copied().unwrap_or(0),
            bytes.get(3).copied().unwrap_or(0),
        ),
        "HCF image bytes received"
    );

    // Create blob URL
    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(&bytes);

    let blob_parts = js_sys::Array::new();
    blob_parts.push(&array.buffer());

    let options = web_sys::BlobPropertyBag::new();
    options.set_type(&hcf.content_type);

    let blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(&blob_parts, &options)
        .map_err(|e| format!("Failed to create blob: {:?}", e))?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|e| format!("Failed to create object URL: {:?}", e))?;

    tracing::info!(blob_url = %url, "Created blob URL for HCF image");

    Ok(url)
}

/// WASM entry point
#[wasm_bindgen(start)]
pub fn main() {
    // Initialize tracing
    ui_core::init_widget_with_level(tracing::Level::INFO);

    // Mount the app
    leptos::mount::mount_to_body(App);
}

/// Main application component
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    // Provide shared cache for collection data
    provide_context(CollectionCache::default());

    view! {
        <Html {..} lang="en" data-bs-theme="dark"/>
        <Title text="NFT Collection Viewer"/>
        <Meta name="description" content="View NFT collections with trait filtering"/>
        <Meta name="viewport" content="width=device-width, initial-scale=1.0"/>

        <Router>
            <Routes fallback=|| view! { <NotFound/> }>
                <Route path=path!("/") view=HomePage/>
                <Route path=path!("/debug/:slug") view=DebugPage/>
                <Route path=path!("/:slug") view=GalleryPage/>
                <Route path=path!("/:slug/:id") view=DetailPage/>
            </Routes>
        </Router>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    view! {
        <div class="error-message">
            <h2>"404 - Page Not Found"</h2>
            <p>"The page you're looking for doesn't exist."</p>
        </div>
    }
}
