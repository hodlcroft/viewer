use crate::{CollectionCache, HcfInfo, TokenInfo, TraitInfo, collection_url, fetch_collection, fetch_hcf_image};
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
                                            hcf=collection.hcf.clone()
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
    current_index: usize,
    total_count: usize,
    prev_token: Option<TokenInfo>,
    next_token: Option<TokenInfo>,
    traits: Vec<TraitInfo>,
    hcf: Option<HcfInfo>,
) -> impl IntoView {
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
    let token_traits = decode_token_traits(&token, &traits, total_count as u32);

    let rarity_class = rarity_tier_class(token.rarity_rank, total_count);
    let percentile = if total_count > 0 {
        100.0 - (token.rarity_rank as f64 / total_count as f64 * 100.0)
    } else {
        0.0
    };

    // NodeRef to the main image element - we'll manipulate it directly
    let img_ref = NodeRef::<leptos::html::Img>::new();

    // Track loading state for spinner
    let (is_loading, set_is_loading) = signal(true);

    // Fetch full image from HCF if available
    if let (Some(hcf_info), Some(location)) = (hcf.clone(), token.hcf_location.clone()) {
        let slug_for_fetch = slug.clone();

        // Clone img_ref for the async block
        let img_ref_clone = img_ref.clone();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_hcf_image(&slug_for_fetch, &hcf_info, &location).await {
                Ok(blob_url) => {
                    tracing::info!(blob_url = %blob_url, "Got blob URL, setting on img element");

                    // Directly set src and show image (if component still mounted)
                    if let Some(img) = img_ref_clone.get() {
                        // Revoke previous blob URL if any
                        let old_src = img.src();
                        if old_src.starts_with("blob:") {
                            let _ = web_sys::Url::revoke_object_url(&old_src);
                        }
                        img.set_src(&blob_url);
                        let _ = img.class_list().add_1("loaded");
                        // Only update signal if component still exists
                        let _ = set_is_loading.try_set(false);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch HCF image: {}", e);
                    let _ = set_is_loading.try_set(false);
                }
            }
        });
    } else {
        set_is_loading.set(false);
    }

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
                        class:hidden=move || !is_loading.try_get().unwrap_or(false)
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
