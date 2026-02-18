use crate::{CollectionCache, HcfInfo, RarityAlgorithm, TokenInfo, TraitInfo, collection_url, fetch_collection, fetch_hcf_image_with_signal};
use super::SortContext;
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::cell::RefCell;

/// Tracks active loading requests (Send+Sync for context)
#[derive(Clone)]
struct LoadingTracker {
    /// Current request ID (incremented on each new request)
    current_request_id: Arc<AtomicU64>,
    /// Completed request ID (when this equals current, loading is done)
    completed_request_id: RwSignal<u64>,
}

impl LoadingTracker {
    fn new() -> Self {
        Self {
            current_request_id: Arc::new(AtomicU64::new(0)),
            completed_request_id: RwSignal::new(0),
        }
    }

    /// Start a new loading request, returns request_id
    fn start_request(&self) -> u64 {
        self.current_request_id.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Complete a request - only updates state if request ID matches current
    fn complete_request(&self, request_id: u64) {
        let current = self.current_request_id.load(Ordering::SeqCst);
        if request_id == current {
            self.completed_request_id.set(request_id);
        }
    }

    /// Check if there's an active request
    fn is_loading(&self) -> bool {
        let current = self.current_request_id.load(Ordering::SeqCst);
        let completed = self.completed_request_id.get();
        current > 0 && completed != current
    }

    /// Get the current request ID (for checking if a request is stale)
    fn current_id(&self) -> u64 {
        self.current_request_id.load(Ordering::SeqCst)
    }
}

// Thread-local storage for the current AbortController
// This works because WASM is single-threaded
thread_local! {
    static CURRENT_ABORT_CONTROLLER: RefCell<Option<web_sys::AbortController>> = const { RefCell::new(None) };
}

/// Cancel any in-flight HCF request and create a new abort signal
fn new_abort_signal() -> web_sys::AbortSignal {
    CURRENT_ABORT_CONTROLLER.with(|cell| {
        // Cancel existing request if any
        if let Some(old) = cell.borrow_mut().take() {
            old.abort();
            tracing::debug!("Cancelled previous HCF request");
        }

        // Create new controller
        let controller = web_sys::AbortController::new()
            .expect("AbortController should be available");
        let signal = controller.signal();
        *cell.borrow_mut() = Some(controller);
        signal
    })
}

/// Clear the current abort controller (call when request completes)
fn clear_abort_controller() {
    CURRENT_ABORT_CONTROLLER.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

#[component]
pub fn DetailPage() -> impl IntoView {
    let params = use_params_map();
    let cache = expect_context::<CollectionCache>();
    let sort_ctx = use_context::<SortContext>();

    let slug = move || params.read().get("slug").unwrap_or_default();
    let id = move || params.read().get("id").unwrap_or_default();

    // Create loading tracker at DetailPage level so it persists across token changes
    let loading_tracker = LoadingTracker::new();
    provide_context(loading_tracker);

    // Fetch collection data
    let collection_resource = LocalResource::new(move || {
        let s = slug();
        let cache = cache.clone();
        async move {
            if s.is_empty() {
                return Err("Missing collection slug".to_string());
            }
            fetch_collection(&s, &cache).await
        }
    });

    view! {
        <Suspense fallback=|| view! {
            <div class="tweakpane-view">
                <div class="image-viewport">
                    <div class="loading">
                        <div class="spinner"></div>
                    </div>
                </div>
                <div class="tp-panel">
                    <div class="tp-header">
                        <span class="tp-back">"Gallery"</span>
                        <span class="tp-title">"Loading..."</span>
                    </div>
                </div>
            </div>
        }>
            {move || Suspend::new({
                    let collection_resource = collection_resource.clone();
                    async move {
                        match collection_resource.await {
                            Ok(collection) => {
                                let s = slug();
                                let current_id = id();

                                // Find the token by asset_id
                                let token = collection.tokens.iter()
                                    .find(|t| t.asset_id == current_id)
                                    .cloned();

                                match token {
                                    Some(token) => {
                                        let total = collection.tokens.len();

                                        view! {
                                            <TokenDetail
                                                token=token
                                                slug=s
                                                total_count=total
                                                sort_ctx=sort_ctx
                                                traits=collection.traits.clone()
                                                hcf=collection.hcf.clone()
                                                hide_rarity=collection.hide_rarity
                                            />
                                        }.into_any()
                                    }
                                    None => view! {
                                        <div class="tweakpane-view">
                                            <div class="image-viewport">
                                                <div class="error-message">
                                                    <h2>"Token Not Found"</h2>
                                                    <p>{format!("No token with ID '{}' found", current_id)}</p>
                                                </div>
                                            </div>
                                        </div>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! {
                                <div class="tweakpane-view">
                                    <div class="image-viewport">
                                        <div class="error-message">
                                            <h2>"Error Loading Collection"</h2>
                                            <p>{e}</p>
                                        </div>
                                    </div>
                                </div>
                            }.into_any(),
                        }
                    }
                })}
        </Suspense>
    }
}

/// Get rarity tier class based on rank and total count
fn rarity_tier_class(rank: u16, total: usize) -> &'static str {
    if total == 0 {
        return "rarity-common";
    }
    let percentile = 100.0 - (rank as f64 / total as f64 * 100.0);
    if percentile >= 95.0 {
        "rarity-legendary"
    } else if percentile >= 85.0 {
        "rarity-epic"
    } else if percentile >= 65.0 {
        "rarity-rare"
    } else if percentile >= 35.0 {
        "rarity-uncommon"
    } else {
        "rarity-common"
    }
}

/// Get rarity tier class for trait percentage (inverted - lower % = rarer)
fn trait_rarity_class(pct: f64) -> &'static str {
    if pct <= 2.0 {
        "rarity-legendary"
    } else if pct <= 5.0 {
        "rarity-epic"
    } else if pct <= 15.0 {
        "rarity-rare"
    } else if pct <= 35.0 {
        "rarity-uncommon"
    } else {
        "rarity-common"
    }
}

/// Decoded trait with percentage
struct DecodedTrait {
    trait_name: String,
    value: String,
    percentage: f64,
}

/// Decode token traits from bitmap
fn decode_token_traits(token: &TokenInfo, traits: &[TraitInfo], total_tokens: u32) -> Vec<DecodedTrait> {
    let mut result = Vec::new();

    for trait_info in traits {
        for value_info in &trait_info.values {
            let byte_idx = value_info.bit_index as usize / 8;
            let bit_idx = value_info.bit_index as usize % 8;

            if byte_idx < token.trait_bitmap.len() {
                if token.trait_bitmap[byte_idx] & (1 << bit_idx) != 0 {
                    // Token has this trait value
                    let percentage = if total_tokens > 0 {
                        (value_info.count as f64 / total_tokens as f64) * 100.0
                    } else {
                        0.0
                    };
                    result.push(DecodedTrait {
                        trait_name: trait_info.name.clone(),
                        value: value_info.value.clone(),
                        percentage,
                    });
                }
            }
        }
    }

    result
}



#[component]
fn TokenDetail(
    token: TokenInfo,
    slug: String,
    total_count: usize,
    sort_ctx: Option<SortContext>,
    traits: Vec<TraitInfo>,
    hcf: Option<HcfInfo>,
    hide_rarity: bool,
) -> impl IntoView {
    let back_url = format!("/{slug}#token-{}", token.index);

    // Get prev/next from sort context, or fall back to no navigation
    let current_id = token.asset_id.clone();
    let (prev_url, next_url, current_index) = if let Some(ctx) = &sort_ctx {
        let prev = ctx.prev_id(&current_id).map(|id| format!("/{slug}/{id}"));
        let next = ctx.next_id(&current_id).map(|id| format!("/{slug}/{id}"));
        let pos = ctx.position(&current_id).map(|(p, _)| p).unwrap_or(0);
        (prev, next, pos)
    } else {
        (None, None, token.index as usize)
    };

    let has_prev = prev_url.is_some();
    let has_next = next_url.is_some();

    // Decode traits from bitmap
    let token_traits = decode_token_traits(&token, &traits, total_count as u32);

    let rarity_algo = use_context::<RwSignal<RarityAlgorithm>>();
    let source_rank = token.source_rank;
    let me_rank = token.me_rank;
    let ic_rank = token.ic_rank;
    let active_rank = move || {
        let algo = rarity_algo.map(|s| s.get()).unwrap_or_default();
        match algo {
            RarityAlgorithm::Source => source_rank,
            RarityAlgorithm::MagicEden => me_rank,
            RarityAlgorithm::InformationContent => ic_rank,
        }
    };

    // NodeRef to the main image element - we'll manipulate it directly
    let img_ref = NodeRef::<leptos::html::Img>::new();

    // Get loading tracker from context (persists across token navigations)
    let loading_tracker = expect_context::<LoadingTracker>();

    // Fetch full image from HCF if available
    if let (Some(hcf_info), Some(location)) = (hcf.clone(), token.hcf_location.clone()) {
        let slug_for_fetch = slug.clone();
        let img_ref_clone = img_ref.clone();
        let tracker = loading_tracker.clone();

        // Cancel any previous request and get new abort signal
        let abort_signal = new_abort_signal();

        // Start tracking this request
        let request_id = tracker.start_request();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_hcf_image_with_signal(&slug_for_fetch, &hcf_info, &location, Some(&abort_signal)).await {
                Ok(blob_url) => {
                    // Check if this request is still current before updating DOM
                    if tracker.current_id() == request_id {
                        if let Some(img) = img_ref_clone.get() {
                            img.set_src(&blob_url);
                            let _ = img.class_list().add_1("loaded");
                        }
                        clear_abort_controller();
                    }
                    // Always complete the request to update pending count
                    tracker.complete_request(request_id);
                }
                Err(e) => {
                    // Don't log aborted requests as warnings
                    if !e.contains("abort") {
                        tracing::warn!("Failed to fetch HCF image: {}", e);
                    }
                    tracker.complete_request(request_id);
                }
            }
        });
    }

    let loading_tracker_for_view = loading_tracker.clone();

    let navigate = use_navigate();

    // Keyboard navigation
    let on_keydown = {
        let navigate = navigate.clone();
        let prev_url = prev_url.clone();
        let next_url = next_url.clone();

        move |ev: web_sys::KeyboardEvent| {
            match ev.key().as_str() {
                "ArrowLeft" | "a" | "A" => {
                    if let Some(ref url) = prev_url {
                        ev.prevent_default();
                        navigate(url, Default::default());
                    }
                }
                "ArrowRight" | "d" | "D" => {
                    if let Some(ref url) = next_url {
                        ev.prevent_default();
                        navigate(url, Default::default());
                    }
                }
                _ => {}
            }
        }
    };

    // Sprite as fallback/placeholder while loading
    let sprite_url = collection_url(&slug, &format!("sprites/{:04}.webp", token.sprite_sheet));
    let sprite_style = format!(
        "background-image: url('{}'); --sprite-col: {}; --sprite-row: {};",
        sprite_url, token.sprite_x, token.sprite_y
    );

    let token_name_for_img = token.name.clone();
    let token_name_for_sprite = token.name.clone();

    view! {
        <div
            class="tweakpane-view"
            tabindex="0"
            on:keydown=on_keydown
        >
            // Image viewport
            <div class="image-viewport">
                // Always show sprite as background placeholder
                <div
                    class="detail-sprite"
                    style=sprite_style.clone()
                    role="img"
                    aria-label=format!("NFT {} (loading)", token_name_for_sprite)
                ></div>

                // Full-res image - src is set directly via NodeRef when blob URL is ready
                // Hidden initially (no src), shown when loaded class is added
                <img
                    class="detail-image"
                    node_ref=img_ref
                    alt=format!("NFT {}", token_name_for_img)
                />
            </div>

            // Floating panel
            <div class="tp-panel">
                <div class="tp-header">
                    <span class="tp-back"><A href=back_url>"Gallery"</A></span>
                    <span class="tp-title">{token.name.clone()}</span>
                    <div
                        class="tp-header-spinner"
                        class:hidden=move || !loading_tracker_for_view.is_loading()
                    >
                        <div class="spinner"></div>
                    </div>
                </div>

                // Navigation
                <div class="tp-section">
                    <div class="tp-nav">
                        {prev_url.map(|url| view! {
                            <span class="tp-nav-btn"><A href=url>"◀"</A></span>
                        })}
                        {(!has_prev).then(|| view! {
                            <span class="tp-nav-btn disabled">"◀"</span>
                        })}

                        <span class="tp-nav-label">{format!("{} / {}", current_index + 1, total_count)}</span>

                        {next_url.map(|url| view! {
                            <span class="tp-nav-btn"><A href=url>"▶"</A></span>
                        })}
                        {(!has_next).then(|| view! {
                            <span class="tp-nav-btn disabled">"▶"</span>
                        })}
                    </div>
                </div>

                // Rarity (only show if we have rarity data and it's not hidden)
                {(!hide_rarity).then(|| view! {
                    {move || {
                        let rank = active_rank();
                        (rank > 0).then(|| {
                            let rarity_class = rarity_tier_class(rank, total_count);
                            let percentile = if total_count > 0 {
                                100.0 - (rank as f64 / total_count as f64 * 100.0)
                            } else {
                                0.0
                            };
                            view! {
                                <div class="tp-section">
                                    <div class="tp-row">
                                        <span class="tp-label">"Rank"</span>
                                        <span class={format!("tp-value tp-rank {rarity_class}")}>
                                            {format!("#{rank}")}
                                        </span>
                                    </div>
                                    <div class="tp-row">
                                        <span class="tp-label">"Percentile"</span>
                                        <span class={format!("tp-value {rarity_class}")}>
                                            {format!("Top {:.1}%", 100.0 - percentile)}
                                        </span>
                                    </div>
                                </div>
                            }
                        })
                    }}
                })}

                // Attributes
                <div class="tp-section tp-attributes">
                    <div class="tp-section-title">"Attributes"</div>
                    {token_traits.into_iter().map(|t| {
                        let filter_url = format!("/{slug}?filter={}:{}",
                            urlencoding::encode(&t.trait_name),
                            urlencoding::encode(&t.value)
                        );
                        let trait_class = trait_rarity_class(t.percentage);

                        view! {
                            <div class="tp-row tp-trait">
                                <span class="tp-label">{t.trait_name}</span>
                                <div class="tp-trait-value">
                                    <A href=filter_url attr:class="tp-trait-link">
                                        <span class={format!("tp-value {}", trait_class)}>{t.value}</span>
                                        <span class={format!("tp-pct {}", trait_class)}>{format!("{:.1}%", t.percentage)}</span>
                                    </A>
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </div>
        </div>
    }
}
