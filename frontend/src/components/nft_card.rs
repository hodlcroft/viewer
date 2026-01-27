use crate::{api_url, SPRITES_PER_SHEET, SPRITE_COLUMNS};
use viewer_format::TokenDetails;
use leptos::prelude::*;
use leptos_router::components::A;

/// Get rarity tier class based on percentile
fn rarity_class(percentile: f64) -> &'static str {
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
pub fn NftCard(project: String, seed: String, token: TokenDetails, index: usize) -> impl IntoView {
    let id = token.id.clone();
    let anchor_id = format!("token-{id}");
    let rank_class = rarity_class(token.rarity.percentile);
    let detail_url = format!("/{project}/{seed}/{id}");

    // Preserve token in detail URL
    let detail_url = if let Some(access_token) = crate::get_access_token() {
        format!("{detail_url}?token={access_token}")
    } else {
        detail_url
    };

    // Calculate sprite position from index
    let sheet = index as u32 / SPRITES_PER_SHEET;
    let pos_in_sheet = index as u32 % SPRITES_PER_SHEET;
    let col = pos_in_sheet % SPRITE_COLUMNS;
    let row = pos_in_sheet / SPRITE_COLUMNS;

    // Sprite sheet URL
    let sprite_url = api_url(&format!("/{project}/{seed}/sprites/{:03}", sheet));

    // Use CSS custom properties to pass col/row to CSS
    // The actual positioning is done in CSS using calc()
    let bg_style = format!(
        "background-image: url('{}'); --sprite-col: {}; --sprite-row: {};",
        sprite_url, col, row
    );

    view! {
        <A href=detail_url attr:class="nft-card" attr:id=anchor_id>
            <div
                class="nft-card-sprite"
                style=bg_style
                role="img"
                aria-label=format!("NFT #{id}")
            ></div>
            <div class="nft-card-info">
                <strong>{format!("#{id}")}</strong>
                <span class={format!("rank {}", rank_class)}>{format!("Rank #{}", token.rarity.rank)}</span>
            </div>
        </A>
    }
}
