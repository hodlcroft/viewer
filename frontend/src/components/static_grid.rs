use crate::{NftCard, TokenInfo};
use leptos::prelude::*;

/// Static grid that renders all tokens upfront.
/// Filtering is handled by CSS class selectors — each card has trait bit classes
/// (b0, b3, b7, etc.) and a dynamic `<style>` element hides non-matching cards.
/// Sorting is handled via CSS `order` set by a dynamic `<style>` element.
#[component]
pub fn StaticGrid(
    slug: String,
    tokens: Vec<TokenInfo>,
    filter_css: Signal<String>,
    sort_css: Signal<String>,
) -> impl IntoView {
    view! {
        <style>{move || filter_css.get()}</style>
        <style>{move || sort_css.get()}</style>
        <div class="static-grid">
            {tokens.into_iter().map(|token| {
                let s = slug.clone();
                view! {
                    <NftCard slug=s token=token />
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}
