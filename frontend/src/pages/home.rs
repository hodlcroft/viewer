use leptos::prelude::*;

#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <div class="home-page">
            <h1>"NFT Preview Viewer"</h1>
            <p>"View generated NFT collections from Cloudflare R2 storage."</p>

            <h2>"Usage"</h2>
            <p>"Navigate to a collection using the URL format:"</p>
            <code>"/[project]/[seed]?token=[access_token]"</code>

            <h2>"Example"</h2>
            <code>"/hodlcroft/000000000000002a?token=your-secret-token"</code>

            <p style="margin-top: 2rem; color: var(--text-muted);">
                "Collections must be published to R2 using the compositor CLI."
            </p>
        </div>
    }
}
