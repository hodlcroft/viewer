use crate::{CollectionCache, CollectionData, RarityAlgorithm, StaticGrid, Theme, TraitInfo, TokenInfo, WalletButton, fetch_collection};
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use std::collections::HashMap;
use wasm_bindgen::JsValue;

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

/// Sort order for gallery display
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Default,
    Rank,
}

/// Context for sharing sort order and sorted tokens across components
#[derive(Clone, Copy)]
pub struct SortContext {
    pub order: RwSignal<SortOrder>,
    /// Sorted list of asset_ids in current sort order
    sorted_ids: RwSignal<Vec<String>>,
}

impl SortContext {
    pub fn new() -> Self {
        Self {
            order: RwSignal::new(SortOrder::Default),
            sorted_ids: RwSignal::new(Vec::new()),
        }
    }

    pub fn get(&self) -> SortOrder {
        self.order.get()
    }

    pub fn set(&self, order: SortOrder) {
        self.order.set(order);
    }

    /// Update the sorted token list based on current sort order and rarity algorithm
    pub fn update_tokens(&self, tokens: &[TokenInfo], algo: RarityAlgorithm) {
        let order = self.order.get();
        let mut sorted: Vec<_> = tokens.iter().collect();

        match order {
            SortOrder::Default => {
                // Already in default order (by index)
            }
            SortOrder::Rank => {
                sorted.sort_by_key(|t| t.rank_for(algo));
            }
        }

        self.sorted_ids.set(sorted.iter().map(|t| t.asset_id.clone()).collect());
    }

    /// Get the previous asset_id in the sorted list
    pub fn prev_id(&self, current_id: &str) -> Option<String> {
        let ids = self.sorted_ids.get();
        let pos = ids.iter().position(|id| id == current_id)?;
        if pos > 0 {
            Some(ids[pos - 1].clone())
        } else {
            None
        }
    }

    /// Get the next asset_id in the sorted list
    pub fn next_id(&self, current_id: &str) -> Option<String> {
        let ids = self.sorted_ids.get();
        let pos = ids.iter().position(|id| id == current_id)?;
        if pos + 1 < ids.len() {
            Some(ids[pos + 1].clone())
        } else {
            None
        }
    }

    /// Get current position and total count
    pub fn position(&self, current_id: &str) -> Option<(usize, usize)> {
        let ids = self.sorted_ids.get();
        let pos = ids.iter().position(|id| id == current_id)?;
        Some((pos, ids.len()))
    }
}

impl Default for SortContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Context for sharing collection configuration (display settings) across components
#[derive(Clone, Copy)]
pub struct CollectionConfig {
    /// Whether to hide rarity rankings in the UI
    pub hide_rarity: bool,
    /// Whether the binary includes source rarity data
    pub has_source_rarity: bool,
    /// Total token count (for percentile calculations)
    pub total_tokens: u32,
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

    // Get sort context from app-level provider
    let sort_ctx = expect_context::<SortContext>();

    // Parse initial filters from URL query param: ?filter=Trait:Value,Trait2:Value2
    Effect::new(move |_| {
        if let Some(filter_param) = query.read().get("filter") {
            for part in filter_param.split(',') {
                if let Some((trait_name, value)) = part.split_once(':') {
                    let trait_name = urlencoding::decode(trait_name)
                        .map(|s| s.into_owned())
                        .unwrap_or_else(|_| trait_name.to_string());
                    let value = urlencoding::decode(value)
                        .map(|s| s.into_owned())
                        .unwrap_or_else(|_| value.to_string());

                    filter_ctx.add_filter(trait_name, value);
                }
            }
        }
    });

    // Sync filter state to URL query params
    Effect::new(move |_| {
        let filters = filter_ctx.get_filters();
        let slug = params.read().get("slug").unwrap_or_default();
        let path = format!("/{slug}");
        let url = if filters.is_empty() {
            path
        } else {
            let filter_param: Vec<String> = filters
                .iter()
                .map(|(k, v)| format!("{}:{}", urlencoding::encode(k), urlencoding::encode(v)))
                .collect();
            format!("{path}?filter={}", filter_param.join(","))
        };
        if let Some(window) = web_sys::window() {
            let _ = window.history().and_then(|h| {
                h.replace_state_with_url(
                    &JsValue::NULL,
                    "",
                    Some(&url),
                )
            });
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
                    collection_name=None
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

                                // Provide collection config context for child components
                                provide_context(CollectionConfig {
                                    hide_rarity: collection.hide_rarity,
                                    has_source_rarity: collection.has_source_rarity,
                                    total_tokens: collection.token_count,
                                });

                                // Default to MagicEden algorithm when no source rarity is available
                                if !collection.has_source_rarity {
                                    let rarity_algo = expect_context::<RwSignal<RarityAlgorithm>>();
                                    if rarity_algo.get() == RarityAlgorithm::Source {
                                        rarity_algo.set(RarityAlgorithm::MagicEden);
                                    }
                                }

                                // Get rarity algorithm signal
                                let rarity_algo = expect_context::<RwSignal<RarityAlgorithm>>();

                                let has_source_rarity = collection.has_source_rarity;
                                let collection_name = collection.name.clone();
                                let traits = collection.traits.clone();
                                let is_cardano = collection.chain.as_deref() == Some("cardano");
                                let policy_id = collection.policy_id.clone();

                                // Compute rarity before rendering
                                let mut tokens = collection.tokens.clone();
                                CollectionData::recompute_rarity(&mut tokens, &collection.traits);
                                let token_count = tokens.len();

                                // Derive filter CSS — one rule per active filter using bit classes
                                // e.g. ".static-grid .nft-card:not(.b42) { display: none; }"
                                let traits_for_filter = traits.clone();
                                let filter_css = Signal::derive(move || {
                                    let filters = filter_ctx.get_filters();
                                    if filters.is_empty() {
                                        return String::new();
                                    }
                                    let mut css = String::new();
                                    for (trait_name, value) in &filters {
                                        if let Some(ti) = traits_for_filter.iter().find(|t| &t.name == trait_name) {
                                            if let Some(vi) = ti.values.iter().find(|v| &v.value == value) {
                                                use std::fmt::Write;
                                                let _ = writeln!(css,
                                                    ".static-grid .nft-card:not(.b{}) {{ display: none; }}",
                                                    vi.bit_index
                                                );
                                            }
                                        }
                                    }
                                    css
                                });

                                // Derive sort CSS — sets order on each card by #token-{index}
                                let tokens_for_sort = tokens.clone();
                                let traits_for_sort = traits.clone();
                                let sort_css = Signal::derive(move || {
                                    let sort = sort_ctx.get();
                                    let algo = rarity_algo.get();

                                    let css = match sort {
                                        SortOrder::Default => String::new(),
                                        SortOrder::Rank => {
                                            let mut indices: Vec<usize> = (0..token_count).collect();
                                            indices.sort_by_key(|&i| tokens_for_sort[i].rank_for(algo));
                                            let mut css = String::with_capacity(token_count * 30);
                                            for (position, &idx) in indices.iter().enumerate() {
                                                use std::fmt::Write;
                                                let _ = writeln!(css,
                                                    "#token-{idx} {{ order: {position}; }}"
                                                );
                                            }
                                            css
                                        }
                                    };

                                    // Update sort context for detail page navigation
                                    let fb = filter_ctx.get_filters();
                                    let filter_bm = if fb.is_empty() {
                                        vec![]
                                    } else {
                                        build_filter_bitmap(&fb, &traits_for_sort, bitmap_size)
                                    };
                                    let mut nav_tokens: Vec<&TokenInfo> = if filter_bm.is_empty() {
                                        tokens_for_sort.iter().collect()
                                    } else {
                                        tokens_for_sort.iter()
                                            .filter(|t| CollectionData::bitmap_matches(&filter_bm, &t.trait_bitmap))
                                            .collect()
                                    };
                                    match sort {
                                        SortOrder::Default => {}
                                        SortOrder::Rank => {
                                            nav_tokens.sort_by_key(|t| t.rank_for(algo));
                                        }
                                    }
                                    sort_ctx.sorted_ids.set(nav_tokens.iter().map(|t| t.asset_id.clone()).collect());

                                    css
                                });

                                // Filtered count for header display
                                let tokens_for_count = tokens.clone();
                                let traits_for_count = traits.clone();
                                let filtered_count = Signal::derive(move || {
                                    let filters = filter_ctx.get_filters();
                                    if filters.is_empty() {
                                        token_count
                                    } else {
                                        let fb = build_filter_bitmap(&filters, &traits_for_count, bitmap_size);
                                        tokens_for_count.iter()
                                            .filter(|t| CollectionData::bitmap_matches(&fb, &t.trait_bitmap))
                                            .count()
                                    }
                                });

                                // Wallet ownership: derive owned asset IDs from wallet balance
                                let owned_css = if is_cardano {
                                    // Build asset_id → token index lookup
                                    let asset_id_to_index: HashMap<String, u32> = tokens.iter()
                                        .map(|t| (t.asset_id.clone(), t.index))
                                        .collect();

                                    // Fetch balance when wallet connects
                                    let wallet_ctx = wallet_leptos::try_use_wallet();
                                    if let Some(ref w) = wallet_ctx {
                                        let w2 = w.clone();
                                        Effect::new(move |_| {
                                            if w2.is_connected() {
                                                w2.fetch_balance();
                                            }
                                        });
                                    }

                                    let pid = policy_id.clone();
                                    Some(Signal::derive(move || {
                                        let Some(ref wallet) = wallet_ctx else { return String::new() };
                                        let Some(balance) = wallet.balance.get() else { return String::new() };
                                        let Some(ref pid) = pid else { return String::new() };

                                        // Find owned tokens under this policy_id
                                        let Some(policy_assets) = balance.assets.get(pid.as_str()) else {
                                            return String::new();
                                        };

                                        let mut css = String::new();
                                        let mut selectors = Vec::new();
                                        for asset_name_hex in policy_assets.keys() {
                                            let full_id = format!("{pid}{asset_name_hex}");
                                            if let Some(&idx) = asset_id_to_index.get(&full_id) {
                                                selectors.push(format!("#token-{idx}"));
                                            }
                                        }

                                        if !selectors.is_empty() {
                                            use std::fmt::Write;
                                            let _ = write!(
                                                css,
                                                "{} {{ outline: 2px solid #ffd700; outline-offset: -2px; }}",
                                                selectors.join(", ")
                                            );
                                        }
                                        css
                                    }))
                                } else {
                                    None
                                };

                                view! {
                                    <GalleryHeader
                                        slug=s.clone()
                                        collection_name=collection_name
                                        total_count=total
                                        filtered_count=filtered_count
                                        trait_options=trait_options
                                        loading=false
                                        has_source_rarity=has_source_rarity
                                        sort_ctx=sort_ctx
                                        is_cardano=is_cardano
                                    />
                                    <div class="gallery-content">
                                        {if let Some(owned_css) = owned_css {
                                            view! {
                                                <StaticGrid
                                                    slug=s.clone()
                                                    tokens=tokens
                                                    filter_css=filter_css
                                                    sort_css=sort_css
                                                    owned_css=owned_css
                                                />
                                            }.into_any()
                                        } else {
                                            view! {
                                                <StaticGrid
                                                    slug=s.clone()
                                                    tokens=tokens
                                                    filter_css=filter_css
                                                    sort_css=sort_css
                                                />
                                            }.into_any()
                                        }}
                                    </div>
                                }.into_any()
                            }
                            Err(e) => view! {
                                <GalleryHeader
                                    slug=slug()
                                    collection_name=None
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
    collection_name: Option<String>,
    total_count: usize,
    #[prop(into)] filtered_count: Signal<usize>,
    trait_options: Vec<TraitValueOption>,
    loading: bool,
    #[prop(optional)] has_source_rarity: bool,
    #[prop(optional)] sort_ctx: Option<SortContext>,
    #[prop(optional)] is_cardano: bool,
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

    // Use collection name if available, otherwise fall back to slug
    let display_name = collection_name.unwrap_or_else(|| slug.clone());

    view! {
        <div class="gallery-header">
            <div class="gallery-header-main">
                <div class="gallery-header-left">
                    <h1 class="gallery-title">{display_name}</h1>
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
                            // Rarity controls
                            {sort_ctx.is_some().then(|| {
                                let ctx = sort_ctx.unwrap();
                                let rarity_algo = expect_context::<RwSignal<RarityAlgorithm>>();
                                view! {
                                    <div class="rarity-algo-toggle">
                                        {has_source_rarity.then(|| view! {
                                            <button
                                                class="algo-btn"
                                                class:active=move || rarity_algo.get() == RarityAlgorithm::Source
                                                on:click=move |_| rarity_algo.set(RarityAlgorithm::Source)
                                            >"Source"</button>
                                        })}
                                        <button
                                            class="algo-btn"
                                            class:active=move || rarity_algo.get() == RarityAlgorithm::MagicEden
                                            on:click=move |_| rarity_algo.set(RarityAlgorithm::MagicEden)
                                        >"Statistical"</button>
                                        <button
                                            class="algo-btn"
                                            class:active=move || rarity_algo.get() == RarityAlgorithm::InformationContent
                                            on:click=move |_| rarity_algo.set(RarityAlgorithm::InformationContent)
                                        >"IC"</button>
                                    </div>
                                    <select
                                        class="sort-select"
                                        on:change=move |ev| {
                                            let value = event_target_value(&ev);
                                            let order = match value.as_str() {
                                                "rank" => SortOrder::Rank,
                                                _ => SortOrder::Default,
                                            };
                                            ctx.set(order);
                                        }
                                    >
                                        <option value="default" selected=move || ctx.get() == SortOrder::Default>"Default"</option>
                                        <option value="rank" selected=move || ctx.get() == SortOrder::Rank>"Rank"</option>
                                    </select>
                                }
                            })}
                            {move || {
                                let theme = expect_context::<RwSignal<Theme>>();
                                view! {
                                    <select
                                        class="theme-toggle"
                                        on:change=move |ev| {
                                            let value = event_target_value(&ev);
                                            theme.set(Theme::from_str(&value));
                                        }
                                    >
                                        <option value="dark" selected=move || theme.get() == Theme::Default>"Default"</option>
                                        <option value="brutalist" selected=move || theme.get() == Theme::Brutalist>"Brutalist"</option>
                                    </select>
                                }
                            }}
                            {is_cardano.then(|| view! { <WalletButton /> })}
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
