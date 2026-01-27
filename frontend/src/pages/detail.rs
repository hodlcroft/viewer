use crate::{api_url, fetch_asset_details, AssetDetailsCache};
use viewer_format::{AssetDetails, AttributeValue, TokenDetails, TraitSummary};
use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use std::collections::BTreeMap;

#[component]
pub fn DetailPage() -> impl IntoView {
    let params = use_params_map();
    let cache = expect_context::<AssetDetailsCache>();

    let project = move || params.read().get("project").unwrap_or_default();
    let seed = move || params.read().get("seed").unwrap_or_default();
    let id = move || params.read().get("id").unwrap_or_default();

    // Signal to store loaded details - persists across ID changes
    let (details, set_details) = signal::<Option<Result<AssetDetails, String>>>(None);

    // Load data on mount or when project/seed changes
    Effect::new(move || {
        let proj = project();
        let s = seed();
        let cache = cache.clone();

        if proj.is_empty() || s.is_empty() {
            set_details.set(Some(Err("Missing project or seed".to_string())));
            return;
        }

        // Spawn async fetch
        leptos::task::spawn_local(async move {
            let result = fetch_asset_details(&proj, &s, &cache).await;
            set_details.set(Some(result));
        });
    });

    // Build back URL with token preserved
    let back_url = move || {
        let base = format!("/{}/{}", project(), seed());
        if let Some(token) = crate::get_access_token() {
            format!("{base}?token={token}")
        } else {
            base
        }
    };

    view! {
        <Show
            when=move || details.get().is_some()
            fallback=move || view! {
                <div class="tweakpane-view">
                    <div class="image-viewport">
                        <div class="loading">
                            <div class="spinner"></div>
                        </div>
                    </div>
                    <div class="tp-panel">
                        <div class="tp-header">
                            <span class="tp-back"><A href=back_url()>"Gallery"</A></span>
                            <span class="tp-title">"Loading..."</span>
                        </div>
                    </div>
                </div>
            }
        >
            {move || {
                let current_id = id();
                let proj = project();
                let s = seed();

                match details.get() {
                    Some(Ok(asset_details)) => {
                        // Find the token and its index
                        let token_index = asset_details.tokens.iter()
                            .position(|t| t.id == current_id);

                        match token_index {
                            Some(idx) => {
                                let token = asset_details.tokens[idx].clone();
                                let total = asset_details.tokens.len();

                                let prev_id = if idx > 0 {
                                    Some(asset_details.tokens[idx - 1].id.clone())
                                } else {
                                    None
                                };

                                let next_id = if idx < total - 1 {
                                    Some(asset_details.tokens[idx + 1].id.clone())
                                } else {
                                    None
                                };

                                view! {
                                    <TokenDetail
                                        token=token
                                        project=proj
                                        seed=s
                                        current_index=idx
                                        total_count=total
                                        prev_id=prev_id
                                        next_id=next_id
                                        trait_summary=asset_details.trait_summary.clone()
                                    />
                                }.into_any()
                            }
                            None => view! {
                                <div class="tweakpane-view">
                                    <div class="image-viewport">
                                        <div class="error-message">
                                            <h2>"Token Not Found"</h2>
                                            <p>{format!("No token with ID {} found", current_id)}</p>
                                        </div>
                                    </div>
                                </div>
                            }.into_any()
                        }
                    }
                    Some(Err(e)) => view! {
                        <div class="tweakpane-view">
                            <div class="image-viewport">
                                <div class="error-message">
                                    <h2>"Error Loading Token"</h2>
                                    <p>{e}</p>
                                </div>
                            </div>
                        </div>
                    }.into_any(),
                    None => view! { <div></div> }.into_any(),
                }
            }}
        </Show>
    }
}

/// Get rarity tier class based on percentile
fn rarity_tier_class(percentile: f64) -> &'static str {
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

/// Build a URL for filtering by a specific trait value
fn build_filter_url(project: &str, seed: &str, trait_name: &str, value: &str) -> String {
    let base = format!("/{project}/{seed}");
    let filter_param = format!(
        "filter={}:{}",
        urlencoding::encode(trait_name),
        urlencoding::encode(value)
    );

    if let Some(token) = crate::get_access_token() {
        format!("{base}?token={token}&{filter_param}")
    } else {
        format!("{base}?{filter_param}")
    }
}

#[component]
fn TokenDetail(
    token: TokenDetails,
    project: String,
    seed: String,
    current_index: usize,
    total_count: usize,
    prev_id: Option<String>,
    next_id: Option<String>,
    trait_summary: BTreeMap<String, TraitSummary>,
) -> impl IntoView {
    let image_url = api_url(&format!("/{project}/{seed}/images/{}", token.id));

    // Build nav URLs with token preserved
    let access_token = crate::get_access_token();

    let back_url = {
        let base = format!("/{project}/{seed}");
        let anchor = format!("#token-{}", token.id);
        if let Some(ref t) = access_token {
            format!("{base}?token={t}{anchor}")
        } else {
            format!("{base}{anchor}")
        }
    };

    let prev_url = prev_id.as_ref().map(|id| {
        let base = format!("/{project}/{seed}/{id}");
        if let Some(ref t) = access_token {
            format!("{base}?token={t}")
        } else {
            base
        }
    });

    let next_url = next_id.as_ref().map(|id| {
        let base = format!("/{project}/{seed}/{id}");
        if let Some(ref t) = access_token {
            format!("{base}?token={t}")
        } else {
            base
        }
    });

    let has_prev = prev_url.is_some();
    let has_next = next_url.is_some();

    // Collect attributes into a Vec for iteration
    let attributes: Vec<(String, AttributeValue)> = token.attributes.clone().into_iter().collect();

    let rarity_class = rarity_tier_class(token.rarity.percentile);

    // Track image loading state
    let (is_loading, set_is_loading) = signal(true);

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

    view! {
        <div class="tweakpane-view">
            // Full viewport image
            <div
                class="image-viewport"
                on:pointerdown=on_pointer_down
                on:pointerup=on_pointer_up
            >
                <img
                    src=image_url
                    alt=format!("Token #{}", token.id)
                    on:load=move |_| set_is_loading.set(false)
                />
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

                // Rarity
                <div class="tp-section">
                    <div class="tp-row">
                        <span class="tp-label">"Rank"</span>
                        <span class={format!("tp-value tp-rank {}", rarity_class)}>
                            {format!("#{}", token.rarity.rank)}
                        </span>
                    </div>
                    <div class="tp-row">
                        <span class="tp-label">"Percentile"</span>
                        <span class={format!("tp-value {}", rarity_class)}>
                            {format!("Top {:.1}%", token.rarity.percentile)}
                        </span>
                    </div>
                    <div class="tp-row">
                        <span class="tp-label">"Score"</span>
                        <span class="tp-value">{format!("{:.2}", token.rarity.score)}</span>
                    </div>
                </div>

                // Attributes
                <div class="tp-section tp-attributes">
                    <div class="tp-section-title">"Attributes"</div>
                    <For
                        each=move || attributes.clone()
                        key=|(name, _)| name.clone()
                        children={
                            let trait_summary = trait_summary.clone();
                            let project = project.clone();
                            let seed = seed.clone();
                            move |(trait_name, value)| {
                                let summary_for_trait = trait_summary.get(&trait_name).cloned();
                                let project = project.clone();
                                let seed = seed.clone();

                                match value {
                                    AttributeValue::Single(val) => {
                                        let rarity_pct = summary_for_trait
                                            .as_ref()
                                            .and_then(|s| s.values.get(&val).map(|stats| stats.percentage));
                                        let trait_class = rarity_pct.map(|p| trait_rarity_class(p)).unwrap_or("rarity-common");
                                        let filter_url = build_filter_url(&project, &seed, &trait_name, &val);

                                        view! {
                                            <div class="tp-row tp-trait">
                                                <span class="tp-label">{trait_name}</span>
                                                <div class="tp-trait-value">
                                                    <A href=filter_url attr:class="tp-trait-link">
                                                        <span class={format!("tp-value {}", trait_class)}>{val}</span>
                                                        {rarity_pct.map(|pct| view! {
                                                            <span class={format!("tp-pct {}", trait_class)}>{format!("{:.1}%", pct)}</span>
                                                        })}
                                                    </A>
                                                </div>
                                            </div>
                                        }.into_any()
                                    }
                                    AttributeValue::Multiple(values) => {
                                        // First value inline with label, rest indented below
                                        let mut values_iter = values.into_iter();
                                        let first_val = values_iter.next();
                                        let rest: Vec<_> = values_iter.collect();

                                        let first_view = first_val.map({
                                            let summary_for_trait = summary_for_trait.clone();
                                            let project = project.clone();
                                            let seed = seed.clone();
                                            let trait_name = trait_name.clone();
                                            move |val| {
                                                let rarity_pct = summary_for_trait
                                                    .as_ref()
                                                    .and_then(|s| s.values.get(&val).map(|stats| stats.percentage));
                                                let trait_class = rarity_pct.map(|p| trait_rarity_class(p)).unwrap_or("rarity-common");
                                                let filter_url = build_filter_url(&project, &seed, &trait_name, &val);

                                                view! {
                                                    <div class="tp-row tp-trait">
                                                        <span class="tp-label">{trait_name.clone()}</span>
                                                        <div class="tp-trait-value">
                                                            <A href=filter_url attr:class="tp-trait-link">
                                                                <span class={format!("tp-value {}", trait_class)}>{val}</span>
                                                                {rarity_pct.map(|pct| view! {
                                                                    <span class={format!("tp-pct {}", trait_class)}>{format!("{:.1}%", pct)}</span>
                                                                })}
                                                            </A>
                                                        </div>
                                                    </div>
                                                }
                                            }
                                        });

                                        let rest_views = rest.into_iter().map({
                                            let summary_for_trait = summary_for_trait.clone();
                                            let project = project.clone();
                                            let seed = seed.clone();
                                            let trait_name = trait_name.clone();
                                            move |val| {
                                                let rarity_pct = summary_for_trait
                                                    .as_ref()
                                                    .and_then(|s| s.values.get(&val).map(|stats| stats.percentage));
                                                let trait_class = rarity_pct.map(|p| trait_rarity_class(p)).unwrap_or("rarity-common");
                                                let filter_url = build_filter_url(&project, &seed, &trait_name, &val);

                                                view! {
                                                    <div class="tp-row tp-trait tp-trait-continuation">
                                                        <span class="tp-label"></span>
                                                        <div class="tp-trait-value">
                                                            <A href=filter_url attr:class="tp-trait-link">
                                                                <span class={format!("tp-value {}", trait_class)}>{val}</span>
                                                                {rarity_pct.map(|pct| view! {
                                                                    <span class={format!("tp-pct {}", trait_class)}>{format!("{:.1}%", pct)}</span>
                                                                })}
                                                            </A>
                                                        </div>
                                                    </div>
                                                }
                                            }
                                        }).collect::<Vec<_>>();

                                        view! {
                                            <div class="tp-trait-group">
                                                {first_view}
                                                {rest_views}
                                            </div>
                                        }.into_any()
                                    }
                                }
                            }
                        }
                    />
                </div>
            </div>
        </div>
    }
}
