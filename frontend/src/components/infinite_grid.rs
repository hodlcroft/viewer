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
    // Track how many items to show
    let (visible_count, set_visible_count) = signal(BATCH_SIZE);

    // Sentinel element ref for intersection observer
    let sentinel_ref = NodeRef::<leptos::html::Div>::new();

    // Derive the visible items
    let visible_items = move || {
        let all = items.get();
        let count = visible_count.get().min(all.len());
        all[..count].to_vec()
    };

    // Check if there are more items to load
    let has_more = move || {
        let all_count = items.get().len();
        let shown = visible_count.get();
        shown < all_count
    };

    // Setup intersection observer
    Effect::new(move || {
        let Some(sentinel) = sentinel_ref.get() else {
            return;
        };

        let element: &web_sys::Element = sentinel.as_ref();

        // Callback when sentinel becomes visible
        let callback = wasm_bindgen::closure::Closure::wrap(Box::new(
            move |entries: js_sys::Array, _observer: web_sys::IntersectionObserver| {
                for entry in entries.iter() {
                    if let Some(entry) = entry.dyn_ref::<web_sys::IntersectionObserverEntry>() {
                        if entry.is_intersecting() {
                            // Load more items
                            set_visible_count.update(|c| *c += BATCH_SIZE);
                        }
                    }
                }
            },
        )
            as Box<dyn Fn(js_sys::Array, web_sys::IntersectionObserver)>);

        // Create observer with some root margin to trigger slightly before visible
        let options = web_sys::IntersectionObserverInit::new();
        options.set_root_margin("200px");

        let observer = web_sys::IntersectionObserver::new_with_options(
            callback.as_ref().unchecked_ref(),
            &options,
        )
        .expect("IntersectionObserver should be available");

        observer.observe(element);

        // Keep callback alive
        callback.forget();
    });

    // Reset visible count when items change (e.g., filter applied)
    Effect::new(move || {
        // Track items signal
        let _ = items.get();
        // Reset to initial batch
        set_visible_count.set(BATCH_SIZE);
    });

    view! {
        <div class="infinite-grid">
            <For
                each=visible_items
                key=|token| token.index
                children={
                    let slug = slug.clone();
                    move |token| {
                        view! {
                            <crate::NftCard
                                slug=slug.clone()
                                token=token
                            />
                        }
                    }
                }
            />
            // Sentinel for loading more
            <Show when=has_more>
                <div class="infinite-grid-sentinel" node_ref=sentinel_ref>
                    <div class="spinner"></div>
                </div>
            </Show>
        </div>
    }
}
