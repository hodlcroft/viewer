use crate::TokenInfo;
use leptos::prelude::*;
use web_sys::wasm_bindgen::JsCast;

/// How many items to load per batch
const BATCH_SIZE: usize = 128;

/// Infinite scrolling grid that loads items in batches
#[component]
pub fn InfiniteGrid(
    /// Collection slug
    slug: String,
    /// All items to display
    items: Signal<Vec<TokenInfo>>,
) -> impl IntoView {
    // Track how many items we've rendered into the DOM
    let (visible_count, set_visible_count) = signal(BATCH_SIZE);

    // Sentinel element ref for intersection observer
    let sentinel_ref = NodeRef::<leptos::html::Div>::new();

    let visible_items = move || {
        let all = items.get();
        let count = visible_count.get().min(all.len());
        all[..count].to_vec()
    };

    let has_more = move || visible_count.get() < items.get().len();

    // Setup intersection observer
    Effect::new(move || {
        let Some(sentinel) = sentinel_ref.get() else {
            return;
        };

        let element: &web_sys::Element = sentinel.as_ref();

        let callback = wasm_bindgen::closure::Closure::wrap(Box::new(
            move |entries: js_sys::Array, _observer: web_sys::IntersectionObserver| {
                for entry in entries.iter() {
                    if let Some(entry) = entry.dyn_ref::<web_sys::IntersectionObserverEntry>() {
                        if entry.is_intersecting() {
                            set_visible_count.update(|c| *c += BATCH_SIZE);
                        }
                    }
                }
            },
        )
            as Box<dyn Fn(js_sys::Array, web_sys::IntersectionObserver)>);

        let options = web_sys::IntersectionObserverInit::new();
        options.set_root_margin("200px");

        let observer = web_sys::IntersectionObserver::new_with_options(
            callback.as_ref().unchecked_ref(),
            &options,
        )
        .expect("IntersectionObserver should be available");

        observer.observe(element);
        callback.forget();
    });

    view! {
        <div class="infinite-grid">
            {move || {
                let tokens = visible_items();
                let slug = slug.clone();
                tokens.into_iter().map(|token| {
                    let s = slug.clone();
                    view! {
                        <crate::NftCard
                            slug=s
                            token=token
                        />
                    }
                }).collect::<Vec<_>>()
            }}
            <Show when=has_more>
                <div class="infinite-grid-sentinel" node_ref=sentinel_ref>
                    <div class="spinner"></div>
                </div>
            </Show>
        </div>
    }
}
