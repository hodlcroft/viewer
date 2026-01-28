//! # NFT Collection Viewer Frontend
//!
//! A Leptos frontend for viewing NFT collections with trait filtering.
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

/// Build URL for collection assets
pub fn collection_url(slug: &str, path: &str) -> String {
    format!("{FILES_BASE_URL}/{slug}/{path}")
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
    /// Sprite metadata
    pub sprite_thumb_width: u16,
    pub sprite_thumb_height: u16,
    pub sprite_grid_columns: u8,
    pub sprite_grid_rows: u8,
    pub sprite_sheet_count: u16,
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
}

impl CollectionData {
    /// Parse collection.bin data
    pub fn parse(slug: String, data: Vec<u8>) -> Result<Self, String> {
        use viewer_binary::{HEADER_SIZE, Header, MAGIC, SpriteMetadata, TraitSchema};

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

        // Get string table
        let string_table_start = header.string_table_offset as usize;
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

        // Parse tokens
        let bitmap_size = header.bitmap_size().ok_or("Invalid bitmap size")?;
        let bitmap_bytes = bitmap_size.byte_size();
        let token_table_start = header.token_table_offset as usize;

        // Token entry: name_ref(2) + asset_id_ref(2) + rarity_rank(2) + sprite(4) + bitmap(N)
        let token_fixed_size = 2 + 2 + 2 + 4; // 10 bytes fixed
        let token_entry_size = token_fixed_size + bitmap_bytes;

        let mut tokens = Vec::with_capacity(header.token_count as usize);
        for i in 0..header.token_count {
            let offset = token_table_start + (i as usize) * token_entry_size;
            let entry = &data[offset..offset + token_entry_size];

            let name_ref = u16::from_le_bytes([entry[0], entry[1]]);
            let asset_id_ref = u16::from_le_bytes([entry[2], entry[3]]);
            let rarity_rank = u16::from_le_bytes([entry[4], entry[5]]);
            let sprite_sheet = u16::from_le_bytes([entry[6], entry[7]]);
            let sprite_x = entry[8];
            let sprite_y = entry[9];
            let trait_bitmap = entry[10..10 + bitmap_bytes].to_vec();

            tokens.push(TokenInfo {
                index: i,
                name: read_string(string_table_data, name_ref),
                asset_id: read_string(string_table_data, asset_id_ref),
                rarity_rank,
                sprite_sheet,
                sprite_x,
                sprite_y,
                trait_bitmap,
            });
        }

        Ok(CollectionData {
            slug,
            raw: Arc::new(data),
            token_count: header.token_count,
            trait_count: header.trait_count,
            sprite_thumb_width: sprite_meta.thumb_width,
            sprite_thumb_height: sprite_meta.thumb_height,
            sprite_grid_columns: sprite_meta.grid_columns,
            sprite_grid_rows: sprite_meta.grid_rows,
            sprite_sheet_count: sprite_meta.sheet_count,
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
fn read_string(table: &[u8], offset: u16) -> String {
    let start = offset as usize;
    if start >= table.len() {
        return String::new();
    }
    let end = table[start..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| start + p)
        .unwrap_or(table.len());
    String::from_utf8_lossy(&table[start..end]).to_string()
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
