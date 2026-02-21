use crate::{collection_url, fetch_hcf_image, CollectionCache, CollectionData, fetch_collection, FILES_BASE_URL};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

#[component]
pub fn DebugPage() -> impl IntoView {
    let params = use_params_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    let cache = expect_context::<CollectionCache>();

    let collection = LocalResource::new(move || {
        let slug = slug();
        let cache = cache.clone();
        async move { fetch_collection(&slug, &cache).await }
    });

    view! {
        <div class="debug-page">
            <h1>"Debug: " {slug}</h1>

            <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                {move || {
                    collection.get().map(|result| match result {
                        Ok(data) => view! { <DebugInfo data=data/> }.into_any(),
                        Err(e) => view! { <p class="error">"Error: " {e}</p> }.into_any(),
                    })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn DebugInfo(data: CollectionData) -> impl IntoView {
    let first_10_tokens = data.tokens.iter().take(10).cloned().collect::<Vec<_>>();
    let traits_summary: Vec<(String, usize, String)> = data.traits.iter().map(|t| {
        let preview = t.values.iter()
            .take(5)
            .map(|v| format!("{} ({})", v.value.clone(), v.count))
            .collect::<Vec<_>>()
            .join(", ");
        let ellipsis = if t.values.len() > 5 { "..." } else { "" };
        (t.name.clone(), t.values.len(), format!("{}{}", preview, ellipsis))
    }).collect();

    view! {
        <div>
            <section class="debug-section">
                <h2>"Header Info"</h2>
                <table class="debug-table">
                    <tbody>
                        <tr><td>"Token Count"</td><td>{data.token_count}</td></tr>
                        <tr><td>"Trait Count"</td><td>{data.trait_count}</td></tr>
                        <tr><td>"Hide Rarity"</td><td>{if data.hide_rarity { "true" } else { "false" }}</td></tr>
                        <tr><td>"Sprite Thumb Size"</td><td>{format!("{}x{}", data.sprite_thumb_width, data.sprite_thumb_height)}</td></tr>
                        <tr><td>"Sprite Grid"</td><td>{format!("{}x{}", data.sprite_grid_columns, data.sprite_grid_rows)}</td></tr>
                        <tr><td>"Sprite Sheet Count"</td><td>{data.sprite_sheet_count}</td></tr>
                    </tbody>
                </table>
            </section>

            <section class="debug-section">
                <h2>"Source Info"</h2>
                {
                    // Parse raw header to show source section details
                    let raw = &data.raw;
                    let sources_offset = if raw.len() >= 52 {
                        u32::from_le_bytes([raw[48], raw[49], raw[50], raw[51]])
                    } else { 0 };
                    let source_count = if raw.len() >= 16 { raw[15] } else { 0 };
                    let string_table_offset = if raw.len() >= 20 {
                        u32::from_le_bytes([raw[16], raw[17], raw[18], raw[19]])
                    } else { 0 };
                    let trait_schema_offset = if raw.len() >= 24 {
                        u32::from_le_bytes([raw[20], raw[21], raw[22], raw[23]])
                    } else { 0 };

                    // Read raw source bytes if offset is valid
                    let source_raw = if sources_offset > 0 && (sources_offset as usize) < raw.len() {
                        let start = sources_offset as usize;
                        let end = (start + 20).min(raw.len());
                        raw[start..end].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
                    } else {
                        "N/A".to_string()
                    };

                    view! {
                        <table class="debug-table">
                            <tbody>
                                <tr><td>"Chain"</td><td>{data.chain.clone().unwrap_or_else(|| "N/A".into())}</td></tr>
                                <tr><td>"Policy ID"</td><td class="mono">{data.policy_id.clone().unwrap_or_else(|| "N/A".into())}</td></tr>
                                <tr><td>"source_count (header[15])"</td><td>{source_count}</td></tr>
                                <tr><td>"sources_offset (header[48..52])"</td><td>{sources_offset}</td></tr>
                                <tr><td>"string_table_offset"</td><td>{string_table_offset}</td></tr>
                                <tr><td>"trait_schema_offset"</td><td>{trait_schema_offset}</td></tr>
                                <tr><td>"Raw bytes at sources_offset"</td><td class="mono">{source_raw}</td></tr>
                            </tbody>
                        </table>
                    }
                }
            </section>

            <section class="debug-section">
                <h2>"Traits (" {data.traits.len()} ")"</h2>
                <ul class="debug-traits">
                    {traits_summary.into_iter().map(|(name, count, preview)| {
                        view! {
                            <li>
                                <strong>{name}</strong> " (" {count} " values): " {preview}
                            </li>
                        }
                    }).collect::<Vec<_>>()}
                </ul>
            </section>

            <section class="debug-section">
                <h2>"First 10 Tokens"</h2>
                <table class="debug-table debug-tokens">
                    <thead>
                        <tr>
                            <th>"Index"</th>
                            <th>"Name"</th>
                            <th>"Asset ID"</th>
                            <th>"Rank"</th>
                            <th>"Sprite"</th>
                            <th>"Bitmap (hex)"</th>
                            <th>"Preview"</th>
                        </tr>
                    </thead>
                    <tbody>
                        {first_10_tokens.into_iter().map(|token| {
                            let sprite_url = collection_url(&data.slug, &format!("sprites/{:04}.webp", token.sprite_sheet));
                            let bitmap_hex = token.trait_bitmap.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                            let sprite_info = format!("sheet {} @ ({}, {})", token.sprite_sheet, token.sprite_x, token.sprite_y);
                            let name = token.name.clone();
                            let asset_id = token.asset_id.clone();
                            view! {
                                <tr>
                                    <td>{token.index}</td>
                                    <td>{name}</td>
                                    <td class="asset-id">{asset_id}</td>
                                    <td>{token.rarity_rank}</td>
                                    <td>{sprite_info}</td>
                                    <td class="bitmap">{bitmap_hex}</td>
                                    <td>
                                        <div
                                            class="debug-sprite-preview"
                                            style:background-image=format!("url('{}')", sprite_url)
                                            style:background-position=format!("{}px {}px", -(token.sprite_x as i32 * 64), -(token.sprite_y as i32 * 64))
                                        ></div>
                                    </td>
                                </tr>
                            }
                        }).collect::<Vec<_>>()}
                    </tbody>
                </table>
            </section>

            <section class="debug-section">
                <h2>"HCF Info"</h2>
                {if let Some(ref hcf) = data.hcf {
                    view! {
                        <table class="debug-table">
                            <tbody>
                                <tr><td>"Shard Size"</td><td>{format!("{} bytes ({:.1} MB)", hcf.shard_size, hcf.shard_size as f64 / 1024.0 / 1024.0)}</td></tr>
                                <tr><td>"Shard Count"</td><td>{hcf.shard_count}</td></tr>
                                <tr><td>"Image Format"</td><td>{hcf.image_format.clone()}</td></tr>
                                <tr><td>"Content Type"</td><td>{hcf.content_type.clone()}</td></tr>
                            </tbody>
                        </table>
                    }.into_any()
                } else {
                    view! { <p>"No HCF metadata"</p> }.into_any()
                }}
            </section>

            <section class="debug-section">
                <h2>"HCF Token Locations (first 10)"</h2>
                <table class="debug-table">
                    <thead>
                        <tr>
                            <th>"Token"</th>
                            <th>"Shard"</th>
                            <th>"Offset"</th>
                            <th>"Length"</th>
                            <th>"Range Header"</th>
                        </tr>
                    </thead>
                    <tbody>
                        {data.tokens.iter().take(10).map(|token| {
                            if let Some(ref loc) = token.hcf_location {
                                let range_end = loc.offset + loc.length as u64 - 1;
                                let range_header = format!("bytes={}-{}", loc.offset, range_end);
                                view! {
                                    <tr>
                                        <td>{token.index}</td>
                                        <td>{loc.shard}</td>
                                        <td>{loc.offset}</td>
                                        <td>{format!("{} ({:.1} KB)", loc.length, loc.length as f64 / 1024.0)}</td>
                                        <td class="mono">{range_header}</td>
                                    </tr>
                                }.into_any()
                            } else {
                                view! {
                                    <tr>
                                        <td>{token.index}</td>
                                        <td colspan="4">"No HCF location"</td>
                                    </tr>
                                }.into_any()
                            }
                        }).collect::<Vec<_>>()}
                    </tbody>
                </table>
            </section>

            <HcfRangeTest slug=data.slug.clone() data=data.clone() />

            <section class="debug-section">
                <h2>"Raw Bytes (first 100 of collection.bin)"</h2>
                <pre class="debug-hex">
                    {data.raw.iter().take(100).enumerate().map(|(i, b)| {
                        if i % 16 == 0 && i > 0 { format!("\n{:02x} ", b) } else { format!("{:02x} ", b) }
                    }).collect::<String>()}
                </pre>
            </section>
        </div>
    }
}

/// Interactive HCF range request tester
#[component]
fn HcfRangeTest(slug: String, data: CollectionData) -> impl IntoView {
    let (token_idx, set_token_idx) = signal(0usize);
    let (test_result, set_test_result) = signal(None::<String>);
    let (fetched_image, set_fetched_image) = signal(None::<String>);
    let (is_loading, set_is_loading) = signal(false);

    let data_for_test = data.clone();
    let slug_for_test = slug.clone();
    let data_for_curl = data.clone();
    let slug_for_curl = slug.clone();
    let token_count = data.tokens.len();

    let run_test = move |_| {
        let idx = token_idx.get();
        let data = data_for_test.clone();
        let slug = slug_for_test.clone();

        if idx >= data.tokens.len() {
            set_test_result.set(Some(format!("Invalid token index: {}", idx)));
            return;
        }

        let token = &data.tokens[idx];
        let hcf = match &data.hcf {
            Some(h) => h.clone(),
            None => {
                set_test_result.set(Some("No HCF metadata available".to_string()));
                return;
            }
        };
        let location = match &token.hcf_location {
            Some(l) => l.clone(),
            None => {
                set_test_result.set(Some(format!("Token {} has no HCF location", idx)));
                return;
            }
        };

        set_is_loading.set(true);
        set_test_result.set(Some(format!(
            "Fetching token {} from shard {} at offset {} (length {})...",
            idx, location.shard, location.offset, location.length
        )));

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_hcf_image(&slug, &hcf, &location).await {
                Ok(url) => {
                    set_fetched_image.set(Some(url));
                    set_test_result.set(Some(format!(
                        "Success! Token {} fetched from shard {} at offset {} ({} bytes)",
                        idx, location.shard, location.offset, location.length
                    )));
                }
                Err(e) => {
                    set_test_result.set(Some(format!("Error: {}", e)));
                }
            }
            set_is_loading.set(false);
        });
    };

    // Manual curl command for testing
    let curl_cmd = move || {
        let idx = token_idx.get();
        let data = data_for_curl.clone();
        let slug = slug_for_curl.clone();

        if idx >= data.tokens.len() {
            return "Invalid token index".to_string();
        }

        let token = &data.tokens[idx];
        if let (Some(_hcf), Some(loc)) = (&data.hcf, &token.hcf_location) {
            let shard_url = format!("{}/{}/hcf/images_{:03}.hcf", FILES_BASE_URL, slug, loc.shard);
            let range_end = loc.offset + loc.length as u64 - 1;
            format!(
                "curl -s -r {}-{} \"{}\" | xxd | head -5",
                loc.offset, range_end, shard_url
            )
        } else {
            "No HCF data".to_string()
        }
    };

    view! {
        <section class="debug-section">
            <h2>"HCF Range Request Tester"</h2>
            <div class="debug-controls">
                <label>
                    "Token Index: "
                    <input
                        type="number"
                        min="0"
                        max=token_count - 1
                        prop:value=move || token_idx.get().to_string()
                        on:input=move |ev| {
                            if let Ok(v) = event_target_value(&ev).parse::<usize>() {
                                set_token_idx.set(v);
                                set_fetched_image.set(None);
                            }
                        }
                    />
                </label>
                <button on:click=run_test disabled=move || is_loading.get()>
                    {move || if is_loading.get() { "Loading..." } else { "Test Range Request" }}
                </button>
            </div>

            <div class="debug-curl">
                <strong>"Test with curl: "</strong>
                <pre>{curl_cmd}</pre>
            </div>

            {move || test_result.get().map(|r| view! {
                <div class="debug-result">
                    <pre>{r}</pre>
                </div>
            })}

            {move || fetched_image.get().map(|url| view! {
                <div class="debug-image-preview">
                    <h3>"Fetched Image:"</h3>
                    <img src=url style="max-width: 512px; max-height: 512px;" />
                </div>
            })}
        </section>
    }
}
