use crate::{collection_url, CollectionConfig, TokenInfo};
use leptos::prelude::*;
use leptos_router::components::A;

/// Get rarity tier class based on rank and total count
fn rarity_class(rank: u16, total: u32) -> &'static str {
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

#[component]
pub fn NftCard(slug: String, token: TokenInfo) -> impl IntoView {
    // Get collection config from context (provided by GalleryPage)
    let config = use_context::<CollectionConfig>();
    let hide_rarity = config.map(|c| c.hide_rarity).unwrap_or(false);
    let total_tokens = config.map(|c| c.total_tokens).unwrap_or(0);

    let id = token.asset_id.clone();
    let name = token.name.clone();
    let anchor_id = format!("token-{}", token.index);
    let detail_url = format!("/{slug}/{id}");

    // Sprite sheet URL (4-digit format)
    let sprite_url = collection_url(&slug, &format!("sprites/{:04}.webp", token.sprite_sheet));

    // Use CSS custom properties to pass col/row to CSS
    let bg_style = format!(
        "background-image: url('{}'); --sprite-col: {}; --sprite-row: {};",
        sprite_url, token.sprite_x, token.sprite_y
    );

    let rank_class = rarity_class(token.rarity_rank, total_tokens);

    view! {
        <A href=detail_url attr:class="nft-card" attr:id=anchor_id>
            <div
                class="nft-card-sprite"
                style=bg_style
                role="img"
                aria-label=format!("NFT {name}")
            ></div>
            <div class="nft-card-info">
                <strong>{name}</strong>
                {(!hide_rarity && token.rarity_rank > 0).then(|| view! {
                    <span class={format!("rank {rank_class}")}>{format!("Rank #{}", token.rarity_rank)}</span>
                })}
            </div>
        </A>
    }
}
