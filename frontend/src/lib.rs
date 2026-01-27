//! # Preview Viewer Frontend
//!
//! A Leptos frontend for viewing generated NFT collections.
//!
//! Features:
//! - Gallery view with lazy-loaded images
//! - Detail view with trait information and rarity
//! - Token-based access control via URL parameter

mod components;
mod pages;

use viewer_format::AssetDetails;
use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::components::*;
use leptos_router::path;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use wasm_bindgen::prelude::*;

pub use components::*;
pub use pages::*;

/// Get access token from URL query parameter
pub fn get_access_token() -> Option<String> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get("token")
}

/// Build API URL with access token
pub fn api_url(path: &str) -> String {
    let api_path = format!("/api{path}");
    if let Some(token) = get_access_token() {
        format!("{api_path}?token={token}")
    } else {
        api_path
    }
}

/// Cache key for asset details (project + seed)
type CacheKey = (String, String);

/// Shared cache for asset details to avoid refetching on navigation
#[derive(Clone, Default)]
pub struct AssetDetailsCache {
    cache: Arc<RwLock<HashMap<CacheKey, AssetDetails>>>,
}

impl AssetDetailsCache {
    pub fn get(&self, project: &str, seed: &str) -> Option<AssetDetails> {
        let cache = self.cache.read().ok()?;
        cache.get(&(project.to_string(), seed.to_string())).cloned()
    }

    pub fn set(&self, project: &str, seed: &str, details: AssetDetails) {
        if let Ok(mut cache) = self.cache.write() {
            cache.insert((project.to_string(), seed.to_string()), details);
        }
    }
}

/// Sprite sheet constants (must match bundle generation)
pub const SPRITE_THUMB_SIZE: u32 = 300;
pub const SPRITE_COLUMNS: u32 = 10;
pub const SPRITE_ROWS: u32 = 10;
pub const SPRITES_PER_SHEET: u32 = SPRITE_COLUMNS * SPRITE_ROWS; // 100

/// Fetch asset details, using cache if available
pub async fn fetch_asset_details(
    project: &str,
    seed: &str,
    cache: &AssetDetailsCache,
) -> Result<AssetDetails, String> {
    // Check cache first
    if let Some(details) = cache.get(project, seed) {
        return Ok(details);
    }

    // Fetch from API
    let url = api_url(&format!("/{project}/{seed}/asset_details.json"));

    let response = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch: {e}"))?;

    if response.status() == 401 {
        return Err("Unauthorized: Invalid or missing access token".to_string());
    }

    if !response.ok() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let details: AssetDetails = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse: {e}"))?;

    // Store in cache
    cache.set(project, seed, details.clone());

    Ok(details)
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

    // Provide shared cache for asset details
    provide_context(AssetDetailsCache::default());

    view! {
        <Html {..} lang="en" data-bs-theme="dark"/>
        <Title text="NFT Preview Viewer"/>
        <Meta name="description" content="View generated NFT collections"/>
        <Meta name="viewport" content="width=device-width, initial-scale=1.0"/>

        <Router>
            <Routes fallback=|| view! { <NotFound/> }>
                <Route path=path!("/") view=HomePage/>
                <Route path=path!("/:project/:seed") view=GalleryPage/>
                <Route path=path!("/:project/:seed/:id") view=DetailPage/>
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
