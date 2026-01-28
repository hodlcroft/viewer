use crate::{CollectionCache, TokenInfo, TraitInfo, collection_url, fetch_collection};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};

#[component]
pub fn DetailPage() -> impl IntoView {
    let params = use_params_map();
    let cache = expect_context::<CollectionCache>();

    let slug = move || params.read().get("slug").unwrap_or_default();
    let id = move || params.read().get("id").unwrap_or_default();

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
                            let token_index = collection.tokens.iter()
                                .position(|t| t.asset_id == current_id);

                            match token_index {
                                Some(idx) => {
                                    let token = collection.tokens[idx].clone();
                                    let total = collection.tokens.len();

                                    let prev_token = if idx > 0 {
                                        Some(collection.tokens[idx - 1].clone())
                                    } else {
                                        None
                                    };

                                    let next_token = if idx < total - 1 {
                                        Some(collection.tokens[idx + 1].clone())
                                    } else {
                                        None
                                    };

                                    view! {
                                        <TokenDetail
                                            token=token
                                            slug=s
                                            current_index=idx
                                            total_count=total
                                            prev_token=prev_token
                                            next_token=next_token
                                            traits=collection.traits.clone()
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

/// Decode token traits from bitmap
fn decode_token_traits(token: &TokenInfo, traits: &[TraitInfo]) -> Vec<(String, String, f64)> {
    let mut result = Vec::new();

    for trait_info in traits {
        for value_info in &trait_info.values {
            let byte_idx = value_info.bit_index as usize / 8;
            let bit_idx = value_info.bit_index as usize % 8;

            if byte_idx < token.trait_bitmap.len() {
                if token.trait_bitmap[byte_idx] & (1 << bit_idx) != 0 {
                    // Token has this trait value
                    // Calculate percentage (count is per-value)
                    let pct = value_info.count as f64; // We'd need total to calculate proper %
                    result.push((trait_info.name.clone(), value_info.value.clone(), pct));
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
    current_index: usize,
    total_count: usize,
    prev_token: Option<TokenInfo>,
    next_token: Option<TokenInfo>,
    traits: Vec<TraitInfo>,
) -> impl IntoView {
    // TODO: Fetch full image from HCF - for now use a placeholder or sprite
    // The HCF format needs a worker to extract individual images
    // For now, we could potentially show the sprite larger or add HCF support later

    let back_url = format!("/{slug}#token-{}", token.index);
    let prev_url = prev_token
        .as_ref()
        .map(|t| format!("/{slug}/{}", t.asset_id));
    let next_url = next_token
        .as_ref()
        .map(|t| format!("/{slug}/{}", t.asset_id));

    let has_prev = prev_url.is_some();
    let has_next = next_url.is_some();

    // Decode traits from bitmap
    let token_traits = decode_token_traits(&token, &traits);

    let rarity_class = rarity_tier_class(token.rarity_rank, total_count);
    let percentile = if total_count > 0 {
        100.0 - (token.rarity_rank as f64 / total_count as f64 * 100.0)
    } else {
        0.0
    };

    // Track image loading state
    let (is_loading, _set_is_loading) = signal(true);

    // Swipe/drag detection for mobile navigation
    let (pointer_start_x, set_pointer_start_x) = signal(0.0f64);
    let (pointer_start_y, set_pointer_start_y) = signal(0.0f64);
    let (is_dragging, set_is_dragging) = signal(false);

    let navigate = use_navigate();
    let prev_url_for_swipe = prev_url.clone();
    let next_url_for_swipe = next_url.clone();

    let on_pointer_down = move |ev: web_sys::PointerEvent| {
        set_pointer_start_x.set(ev.client_x() as f64);
        set_pointer_start_y.set(ev.client_y() as f64);
        set_is_dragging.set(true);
    };

    let on_pointer_up = {
        let navigate = navigate.clone();
        let prev_url = prev_url_for_swipe.clone();
        let next_url = next_url_for_swipe.clone();

        move |ev: web_sys::PointerEvent| {
            if !is_dragging.get() {
                return;
            }
            set_is_dragging.set(false);

            let end_x = ev.client_x() as f64;
            let end_y = ev.client_y() as f64;
            let dx = end_x - pointer_start_x.get();
            let dy = end_y - pointer_start_y.get();

            // Only trigger if horizontal swipe > 50px and greater than vertical movement
            if dx.abs() > 50.0 && dx.abs() > dy.abs() {
                if dx > 0.0 {
                    // Swipe right → previous
                    if let Some(ref url) = prev_url {
                        navigate(url, Default::default());
                    }
                } else {
                    // Swipe left → next
                    if let Some(ref url) = next_url {
                        navigate(url, Default::default());
                    }
                }
            }
        }
    };

    // For now, show the sprite sheet image as a placeholder
    // TODO: Add HCF image extraction
    let sprite_url = collection_url(&slug, &format!("sprites/{:04}.webp", token.sprite_sheet));
    let sprite_style = format!(
        "background-image: url('{}'); --sprite-col: {}; --sprite-row: {};",
        sprite_url, token.sprite_x, token.sprite_y
    );

    view! {
        <div class="tweakpane-view">
            // Image viewport - using sprite for now
            <div
                class="image-viewport"
                on:pointerdown=on_pointer_down
                on:pointerup=on_pointer_up
            >
                <div
                    class="detail-sprite"
                    style=sprite_style
                    role="img"
                    aria-label=format!("NFT {}", token.name)
                ></div>
            </div>

            // Floating panel
            <div class="tp-panel">
                <div class="tp-header">
                    <span class="tp-back"><A href=back_url>"Gallery"</A></span>
                    <span class="tp-title">{token.name.clone()}</span>
                    <div class="tp-header-spinner" class:hidden=move || !is_loading.get()>
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

                // Rarity (only show if we have rarity data)
                {(token.rarity_rank > 0).then(|| view! {
                    <div class="tp-section">
                        <div class="tp-row">
                            <span class="tp-label">"Rank"</span>
                            <span class={format!("tp-value tp-rank {rarity_class}")}>
                                {format!("#{}", token.rarity_rank)}
                            </span>
                        </div>
                        <div class="tp-row">
                            <span class="tp-label">"Percentile"</span>
                            <span class={format!("tp-value {rarity_class}")}>
                                {format!("Top {:.1}%", 100.0 - percentile)}
                            </span>
                        </div>
                    </div>
                })}

                // Attributes
                <div class="tp-section tp-attributes">
                    <div class="tp-section-title">"Attributes"</div>
                    {token_traits.into_iter().map(|(trait_name, value, count)| {
                        let filter_url = format!("/{slug}?filter={}:{}",
                            urlencoding::encode(&trait_name),
                            urlencoding::encode(&value)
                        );
                        // For now just show count, proper % would need total
                        let display_count = format!("{} have this", count as u16);

                        view! {
                            <div class="tp-row tp-trait">
                                <span class="tp-label">{trait_name}</span>
                                <div class="tp-trait-value">
                                    <A href=filter_url attr:class="tp-trait-link">
                                        <span class="tp-value">{value}</span>
                                        <span class="tp-count">{display_count}</span>
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
