use crate::{collection_url, CollectionConfig, RarityAlgorithm, TokenInfo};
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

/// Format rank with appropriate width based on collection size
fn format_rank(rank: u16, total: u32) -> String {
    let width = total.max(1).ilog10() as usize + 1;
    format!("{:0width$}", rank)
}

#[component]
pub fn NftCard(slug: String, token: TokenInfo) -> impl IntoView {
    // Get collection config from context (provided by GalleryPage)
    let config = use_context::<CollectionConfig>();
    let has_source_rarity = config.map(|c| c.has_source_rarity).unwrap_or(false);
    let total_tokens = config.map(|c| c.total_tokens).unwrap_or(0);
    let rarity_algo = use_context::<RwSignal<RarityAlgorithm>>();

    let source_rank = token.source_rank;
    let me_rank = token.me_rank;
    let ic_rank = token.ic_rank;
    let id = token.asset_id.clone();
    let name = token.name.clone();
    let anchor_id = format!("token-{}", token.index);
    let detail_url = format!("/{slug}/{id}");

    // Build static CSS class string with trait bit classes (b0 b3 b7 etc.)
    let mut card_class = String::from("nft-card");
    for (byte_idx, &byte) in token.trait_bitmap.iter().enumerate() {
        for bit in 0..8u8 {
            if byte & (1 << bit) != 0 {
                use std::fmt::Write;
                let _ = write!(card_class, " b{}", byte_idx * 8 + bit as usize);
            }
        }
    }

    // Sprite sheet URL (4-digit format)
    let sprite_url = collection_url(&slug, &format!("sprites/{:04}.webp", token.sprite_sheet));

    // Use CSS custom properties to pass col/row to CSS
    let bg_style = format!(
        "background-image: url('{}'); --sprite-col: {}; --sprite-row: {};",
        sprite_url, token.sprite_x, token.sprite_y
    );

    let active_rank = move || {
        let algo = rarity_algo.map(|s| s.get()).unwrap_or_default();
        match algo {
            RarityAlgorithm::Source => source_rank,
            RarityAlgorithm::MagicEden => me_rank,
            RarityAlgorithm::InformationContent => ic_rank,
        }
    };

    view! {
        <A href=detail_url attr:class=card_class attr:id=anchor_id>
            <div class="nft-card-image">
                <div
                    class="nft-card-sprite"
                    style=bg_style
                    role="img"
                    aria-label=format!("NFT {name}")
                ></div>
                {move || {
                    let algo = rarity_algo.map(|s| s.get()).unwrap_or_default();
                    // Hide rank when using Source algorithm without source data
                    let show = match algo {
                        RarityAlgorithm::Source => has_source_rarity && source_rank > 0,
                        _ => true,
                    };
                    let rank = active_rank();
                    (show && rank > 0).then(|| {
                        let rank_class = rarity_class(rank, total_tokens);
                        let formatted = format_rank(rank, total_tokens);
                        view! {
                            <span class={format!("rank-badge {rank_class}")}>
                                {formatted}
                            </span>
                        }
                    })
                }}
            </div>
            <div class="nft-card-info">
                <strong>{name}</strong>
            </div>
        </A>
    }
}
