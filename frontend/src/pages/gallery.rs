use crate::{fetch_asset_details, AssetDetailsCache, InfiniteGrid};
use viewer_format::{AttributeValue, TokenDetails, TraitSummary};
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use std::collections::{BTreeMap, HashMap};

/// Active filters: trait_name -> value
pub type Filters = HashMap<String, String>;

/// A searchable trait-value pair
#[derive(Clone, Debug, PartialEq)]
pub struct TraitValueOption {
    pub trait_name: String,
    pub value: String,
    pub display: String, // "Trait: Value"
    pub count: usize,
    pub percentage: f64,
}

/// Build a filter query string from filters map
fn build_filter_string(filters: &Filters) -> String {
    filters
        .iter()
        .map(|(k, v)| format!("{}:{}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse filters from a query string value
/// Format: TraitName:Value or TraitName:Value,OtherTrait:OtherValue
fn parse_filter_string(filter_str: &str) -> Filters {
    let mut filters = HashMap::new();

    for part in filter_str.split(',') {
        if let Some((trait_name, value)) = part.split_once(':') {
            let decoded_name = urlencoding::decode(trait_name)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| trait_name.to_string());
            let decoded_value = urlencoding::decode(value)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| value.to_string());
            filters.insert(decoded_name, decoded_value);
        }
    }

    filters
}

/// Build searchable options from trait summary
fn build_trait_options(trait_summary: &BTreeMap<String, TraitSummary>) -> Vec<TraitValueOption> {
    let mut options = Vec::new();

    for (trait_name, summary) in trait_summary {
        for (value, stats) in &summary.values {
            options.push(TraitValueOption {
                trait_name: trait_name.clone(),
                value: value.clone(),
                display: format!("{}: {}", trait_name, value),
                count: stats.count,
                percentage: stats.percentage,
            });
        }
    }

    // Sort by trait name, then value
    options.sort_by(|a, b| {
        a.trait_name
            .cmp(&b.trait_name)
            .then_with(|| a.value.cmp(&b.value))
    });

    options
}

/// Context for sharing filter state across components
#[derive(Clone, Copy)]
pub struct FilterContext {
    pub filters: RwSignal<Filters>,
}

impl FilterContext {
    pub fn new() -> Self {
        Self {
            filters: RwSignal::new(HashMap::new()),
        }
    }

    pub fn set_filters(&self, filters: Filters) {
        self.filters.set(filters);
    }

    pub fn add_filter(&self, trait_name: String, value: String) {
        self.filters.update(|f| {
            f.insert(trait_name, value);
        });
    }

    pub fn remove_filter(&self, trait_name: &str) {
        self.filters.update(|f| {
            f.remove(trait_name);
        });
    }

    pub fn clear_filters(&self) {
        self.filters.set(HashMap::new());
    }

    pub fn is_active(&self) -> bool {
        !self.filters.get().is_empty()
    }

    pub fn get_filters(&self) -> Filters {
        self.filters.get()
    }
}

impl Default for FilterContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a token matches all active filters
fn token_matches_filters(token: &TokenDetails, filters: &Filters) -> bool {
    filters.iter().all(|(trait_name, filter_value)| {
        token
            .attributes
            .get(trait_name)
            .map_or(false, |attr_value| match attr_value {
                AttributeValue::Single(val) => val == filter_value,
                AttributeValue::Multiple(vals) => vals.contains(filter_value),
            })
    })
}

#[component]
pub fn GalleryPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let cache = expect_context::<AssetDetailsCache>();

    // Provide filter context for this gallery (Copy so no ownership issues)
    let filter_ctx = FilterContext::new();
    provide_context(filter_ctx);

    // Sync filters from URL query params reactively
    Effect::new(move || {
        let query_map = query.read();
        let filter_param = query_map.get("filter");
        let filters = match filter_param {
            Some(param) if !param.is_empty() => parse_filter_string(&param),
            _ => HashMap::new(),
        };
        filter_ctx.set_filters(filters);
    });

    let project = move || params.read().get("project").unwrap_or_default();
    let seed = move || params.read().get("seed").unwrap_or_default();

    // LocalResource for client-side only fetching, uses shared cache
    let details_resource = LocalResource::new(move || {
        let proj = project();
        let s = seed();
        let cache = cache.clone();
        async move {
            if proj.is_empty() || s.is_empty() {
                return Err("Missing project or seed".to_string());
            }

            fetch_asset_details(&proj, &s, &cache).await
        }
    });

    view! {
        <div class="gallery-view">
            <Suspense fallback=|| view! {
                <GalleryHeader
                    project="".to_string()
                    seed="".to_string()
                    total_count=0
                    filtered_count=0
                    trait_options=vec![]
                    loading=true
                />
                <div class="gallery-content">
                    <div class="loading">
                        <div class="spinner"></div>
                    </div>
                </div>
            }>
                {move || Suspend::new({
                    let details_resource = details_resource.clone();
                    async move {
                        match details_resource.await {
                            Ok(details) => {
                                let proj = project();
                                let s = seed();
                                let total = details.tokens.len();

                                // Build searchable trait options
                                let trait_options = build_trait_options(&details.trait_summary);

                                // Create indexed tokens for filtering
                                let all_tokens: Vec<(usize, TokenDetails)> = details.tokens
                                    .iter()
                                    .cloned()
                                    .enumerate()
                                    .collect();

                                // Derive filtered tokens reactively as a Signal
                                let filtered_tokens = {
                                    let all_tokens = all_tokens.clone();
                                    Signal::derive(move || {
                                        let filters = filter_ctx.get_filters();
                                        if filters.is_empty() {
                                            all_tokens.clone()
                                        } else {
                                            all_tokens
                                                .iter()
                                                .filter(|(_, token)| token_matches_filters(token, &filters))
                                                .cloned()
                                                .collect()
                                        }
                                    })
                                };

                                let filtered_count = {
                                    let all_tokens = all_tokens.clone();
                                    move || {
                                        let filters = filter_ctx.get_filters();
                                        if filters.is_empty() {
                                            all_tokens.len()
                                        } else {
                                            all_tokens
                                                .iter()
                                                .filter(|(_, token)| token_matches_filters(token, &filters))
                                                .count()
                                        }
                                    }
                                };

                                view! {
                                    <GalleryHeader
                                        project=proj.clone()
                                        seed=s.clone()
                                        total_count=total
                                        filtered_count=Signal::derive(filtered_count)
                                        trait_options=trait_options
                                        loading=false
                                    />
                                    <div class="gallery-content">
                                        <InfiniteGrid
                                            project=proj.clone()
                                            seed=s.clone()
                                            items=filtered_tokens
                                        />
                                    </div>
                                }.into_any()
                            }
                            Err(e) => view! {
                                <GalleryHeader
                                    project=project()
                                    seed=seed()
                                    total_count=0
                                    filtered_count=0
                                    trait_options=vec![]
                                    loading=false
                                />
                                <div class="gallery-content">
                                    <div class="error-message">
                                        <h2>"Error Loading Collection"</h2>
                                        <p>{e}</p>
                                    </div>
                                </div>
                            }.into_any(),
                        }
                    }
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn GalleryHeader(
    project: String,
    seed: String,
    total_count: usize,
    #[prop(into)] filtered_count: Signal<usize>,
    trait_options: Vec<TraitValueOption>,
    loading: bool,
) -> impl IntoView {
    let filter_ctx = use_context::<FilterContext>();

    let filters_active = move || filter_ctx.map(|ctx| ctx.is_active()).unwrap_or(false);
    let get_filters = move || filter_ctx.map(|ctx| ctx.get_filters()).unwrap_or_default();

    // Base path for URL navigation
    let base_path = format!("/{}/{}", project, seed);

    // Search state
    let (search_query, set_search_query) = signal(String::new());
    let (show_suggestions, set_show_suggestions) = signal(false);
    let (selected_index, set_selected_index) = signal(0usize);

    // Store trait options for reactive access
    let stored_options = StoredValue::new(trait_options);

    // Filter suggestions based on search query - use Memo for caching
    let suggestions = Memo::new(move |_| {
        let query = search_query.get().to_lowercase();
        let current_filters = get_filters();

        if query.is_empty() {
            return vec![];
        }

        stored_options.with_value(|opts| {
            opts.iter()
                .filter(|opt| {
                    // Don't suggest already-active filters
                    if current_filters.get(&opt.trait_name) == Some(&opt.value) {
                        return false;
                    }
                    // Match against display string
                    opt.display.to_lowercase().contains(&query)
                })
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    // Store base path for reuse in closures
    let stored_base_path = StoredValue::new(base_path.clone());

    // Helper to navigate to a URL (uses history API directly)
    let go_to = move |url: &str| {
        if let Some(window) = web_sys::window() {
            let _ = window
                .history()
                .and_then(|h| h.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(url)));
            // Dispatch popstate to trigger router update
            let _ = window.dispatch_event(&web_sys::Event::new("popstate").unwrap());
        }
    };

    // Helper to build URL from filters and navigate (preserves access token)
    let navigate_to_filters = move |filters: &Filters| {
        let url = stored_base_path.with_value(|base| {
            let access_token = crate::get_access_token();
            match (filters.is_empty(), access_token) {
                (true, None) => base.clone(),
                (true, Some(t)) => format!("{}?token={}", base, t),
                (false, None) => format!("{}?filter={}", base, build_filter_string(filters)),
                (false, Some(t)) => format!(
                    "{}?token={}&filter={}",
                    base,
                    t,
                    build_filter_string(filters)
                ),
            }
        });
        go_to(&url);
    };

    // Handle keyboard navigation
    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        let current_suggestions = suggestions.get();

        match key.as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                if !current_suggestions.is_empty() {
                    set_selected_index.update(|i| *i = (*i + 1) % current_suggestions.len());
                }
            }
            "ArrowUp" => {
                ev.prevent_default();
                if !current_suggestions.is_empty() {
                    let len = current_suggestions.len();
                    set_selected_index.update(|i| {
                        *i = if *i == 0 { len - 1 } else { *i - 1 };
                    });
                }
            }
            "Enter" => {
                ev.prevent_default();
                let idx = selected_index.get();
                if let Some(opt) = current_suggestions.get(idx) {
                    let mut filters = get_filters();
                    filters.insert(opt.trait_name.clone(), opt.value.clone());
                    navigate_to_filters(&filters);
                    set_search_query.set(String::new());
                    set_show_suggestions.set(false);
                    set_selected_index.set(0);
                }
            }
            "Escape" => {
                set_show_suggestions.set(false);
                set_search_query.set(String::new());
            }
            _ => {}
        }
    };

    view! {
        <div class="gallery-header">
            <div class="gallery-header-main">
                <div class="gallery-header-left">
                    <h1 class="gallery-title">{project.clone()}</h1>
                    <span class="gallery-seed">{format!("Seed: {seed}")}</span>
                </div>
                <div class="gallery-header-right">
                    {if loading {
                        view! { <div class="spinner spinner-small"></div> }.into_any()
                    } else {
                        view! {
                            <span class="gallery-count">
                                {move || {
                                    let filtered = filtered_count.get();
                                    if filters_active() && filtered != total_count {
                                        format!("{} / {} tokens", filtered, total_count)
                                    } else {
                                        format!("{} tokens", total_count)
                                    }
                                }}
                            </span>
                        }.into_any()
                    }}
                </div>
            </div>

            // Search input row - always visible
            <div class="gallery-search">
                <div class="search-container">
                    <input
                        type="text"
                        class="search-input"
                        placeholder="Filter by trait..."
                        prop:value=move || search_query.get()
                        on:input=move |ev| {
                            let value = event_target_value(&ev);
                            set_search_query.set(value);
                            set_show_suggestions.set(true);
                            set_selected_index.set(0);
                        }
                        on:focus=move |_| set_show_suggestions.set(true)
                        on:blur=move |_| {
                            // Delay to allow click on suggestion
                            use gloo_timers::callback::Timeout;
                            let timeout = Timeout::new(150, move || {
                                set_show_suggestions.set(false);
                            });
                            timeout.forget();
                        }
                        on:keydown=on_keydown
                    />

                    // Suggestions dropdown
                    <Show when=move || show_suggestions.get() && !suggestions.get().is_empty()>
                        <div class="search-suggestions">
                            {move || {
                                let current_suggestions = suggestions.get();
                                let selected = selected_index.get();

                                current_suggestions.into_iter().enumerate().map(|(idx, opt)| {
                                    let is_selected = idx == selected;
                                    let opt_trait = opt.trait_name.clone();
                                    let opt_value = opt.value.clone();

                                    view! {
                                        <div
                                            class="search-suggestion"
                                            class:selected=is_selected
                                            on:mousedown=move |_| {
                                                let mut filters = get_filters();
                                                filters.insert(opt_trait.clone(), opt_value.clone());
                                                navigate_to_filters(&filters);
                                                set_search_query.set(String::new());
                                                set_show_suggestions.set(false);
                                                set_selected_index.set(0);
                                            }
                                        >
                                            <span class="suggestion-trait">{opt.trait_name.clone()}</span>
                                            <span class="suggestion-value">{opt.value.clone()}</span>
                                            <span class="suggestion-pct">{format!("{:.1}%", opt.percentage)}</span>
                                        </div>
                                    }
                                }).collect::<Vec<_>>()
                            }}
                        </div>
                    </Show>
                </div>
            </div>

            // Filter chips row - only show when filters are active
            <Show when=filters_active>
                {move || {
                    let filters_vec: Vec<(String, String)> = get_filters().into_iter().collect();

                    view! {
                        <div class="gallery-filters">
                            {filters_vec.into_iter().map(|(trait_name, value)| {
                                let trait_name_display = trait_name.clone();
                                let trait_name_for_remove = trait_name.clone();

                                view! {
                                    <span class="filter-chip">
                                        <span class="filter-chip-label">{trait_name_display}</span>
                                        <span class="filter-chip-value">{value}</span>
                                        <button
                                            class="filter-chip-remove"
                                            on:click=move |_| {
                                                let mut filters = get_filters();
                                                filters.remove(&trait_name_for_remove);
                                                navigate_to_filters(&filters);
                                            }
                                        >
                                            "×"
                                        </button>
                                    </span>
                                }
                            }).collect::<Vec<_>>()}
                            <button
                                class="filter-clear"
                                on:click=move |_| {
                                    navigate_to_filters(&HashMap::new());
                                }
                            >
                                "Clear all"
                            </button>
                        </div>
                    }
                }}
            </Show>
        </div>
    }
}
