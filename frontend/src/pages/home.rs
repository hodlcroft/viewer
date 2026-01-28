use leptos::prelude::*;

#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <div class="home-page">
            <h1>"NFT Collection Viewer"</h1>
            <p>"Browse and filter NFT collections with trait-based search."</p>

            <h2>"Usage"</h2>
            <p>"Navigate to a collection using the URL format:"</p>
            <code>"/{slug}"</code>

            <h2>"Example"</h2>
            <p><a href="/blackflag">"/blackflag"</a></p>

            <p style="margin-top: 2rem; color: var(--text-muted);">
                "Collections are served from Cloudflare R2 storage."
            </p>
        </div>
    }
}
