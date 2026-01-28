use crate::{CollectionCache, InfiniteGrid, TraitInfo, fetch_collection};
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use std::collections::HashMap;

/// Active filters: trait_name -> value
pub type Filters = HashMap<String, String>;

/// Build a filter bitmap from active filters
fn build_filter_bitmap(filters: &Filters, traits: &[TraitInfo], bitmap_size: usize) -> Vec<u8> {
    let mut bitmap = vec![0u8; bitmap_size];

    for (trait_name, value) in filters {
        // Find the trait
        if let Some(trait_info) = traits.iter().find(|t| &t.name == trait_name) {
            // Find the value
            if let Some(value_info) = trait_info.values.iter().find(|v| &v.value == value) {
                // Set the bit
                let byte_idx = value_info.bit_index as usize / 8;
                let bit_idx = value_info.bit_index as usize % 8;
                if byte_idx < bitmap.len() {
                    bitmap[byte_idx] |= 1 << bit_idx;
                }
            }
        }
    }

    bitmap
}

/// A searchable trait-value pair
#[derive(Clone, Debug, PartialEq)]
pub struct TraitValueOption {
    pub trait_name: String,
    pub value: String,
    pub display: String,
    pub count: usize,
    pub percentage: f64,
}

/// Build searchable options from trait info
fn build_trait_options(traits: &[TraitInfo], total_tokens: usize) -> Vec<TraitValueOption> {
    let mut options = Vec::new();

    for trait_info in traits {
        for value_info in &trait_info.values {
            let percentage = if total_tokens > 0 {
                (value_info.count as f64 / total_tokens as f64) * 100.0
            } else {
                0.0
            };
            options.push(TraitValueOption {
                trait_name: trait_info.name.clone(),
                value: value_info.value.clone(),
                display: format!("{}: {}", trait_info.name, value_info.value),
                count: value_info.count as usize,
                percentage,
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

#[component]
pub fn GalleryPage() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let cache = expect_context::<CollectionCache>();

    // Provide filter context for this gallery
    let filter_ctx = FilterContext::new();
    provide_context(filter_ctx);

    // Parse initial filter from URL query param: ?filter=Trait:Value
    Effect::new(move |_| {
        if let Some(filter_param) = query.read().get("filter") {
            if let Some((trait_name, value)) = filter_param.split_once(':') {
                // URL decode the values
                let trait_name = urlencoding::decode(trait_name)
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| trait_name.to_string());
                let value = urlencoding::decode(value)
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| value.to_string());

                filter_ctx.add_filter(trait_name, value);
            }
        }
    });

    let slug = move || params.read().get("slug").unwrap_or_default();

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
        <div class="gallery-view">
            <Suspense fallback=|| view! {
                <GalleryHeader
                    slug="".to_string()
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
                    let collection_resource = collection_resource.clone();
                    async move {
                        match collection_resource.await {
                            Ok(collection) => {
                                let s = slug();
                                let total = collection.token_count as usize;
                                let trait_options = build_trait_options(&collection.traits, total);
                                let bitmap_size = collection.tokens.first()
                                    .map(|t| t.trait_bitmap.len())
                                    .unwrap_or(0);

                                // Store collection for filtering
                                let collection = StoredValue::new(collection);

                                // Derive filtered tokens reactively
                                let filtered_tokens = Signal::derive(move || {
                                    let filters = filter_ctx.get_filters();
                                    collection.with_value(|c| {
                                        if filters.is_empty() {
                                            c.tokens.clone()
                                        } else {
                                            let filter_bitmap = build_filter_bitmap(&filters, &c.traits, bitmap_size);
                                            c.tokens
                                                .iter()
                                                .filter(|token| c.token_matches_filter(token, &filter_bitmap))
                                                .cloned()
                                                .collect()
                                        }
                                    })
                                });

                                let filtered_count = Signal::derive(move || filtered_tokens.get().len());

                                view! {
                                    <GalleryHeader
                                        slug=s.clone()
                                        total_count=total
                                        filtered_count=filtered_count
                                        trait_options=trait_options
                                        loading=false
                                    />
                                    <div class="gallery-content">
                                        <InfiniteGrid
                                            slug=s.clone()
                                            items=filtered_tokens
                                        />
                                    </div>
                                }.into_any()
                            }
                            Err(e) => view! {
                                <GalleryHeader
                                    slug=slug()
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
    slug: String,
    total_count: usize,
    #[prop(into)] filtered_count: Signal<usize>,
    trait_options: Vec<TraitValueOption>,
    loading: bool,
) -> impl IntoView {
    let filter_ctx = use_context::<FilterContext>();

    let filters_active = move || filter_ctx.map(|ctx| ctx.is_active()).unwrap_or(false);
    let get_filters = move || filter_ctx.map(|ctx| ctx.get_filters()).unwrap_or_default();

    // Search state
    let (search_query, set_search_query) = signal(String::new());
    let (show_suggestions, set_show_suggestions) = signal(false);
    let (selected_index, set_selected_index) = signal(0usize);

    // Store trait options for reactive access
    let stored_options = StoredValue::new(trait_options);

    // Filter suggestions based on search query
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
                    if let Some(ctx) = filter_ctx {
                        ctx.add_filter(opt.trait_name.clone(), opt.value.clone());
                    }
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
                    <h1 class="gallery-title">{slug.clone()}</h1>
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

            // Search input row
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
                                                if let Some(ctx) = filter_ctx {
                                                    ctx.add_filter(opt_trait.clone(), opt_value.clone());
                                                }
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

            // Filter chips row
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
                                                if let Some(ctx) = filter_ctx {
                                                    ctx.remove_filter(&trait_name_for_remove);
                                                }
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
                                    if let Some(ctx) = filter_ctx {
                                        ctx.clear_filters();
                                    }
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
