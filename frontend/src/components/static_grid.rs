use crate::{NftCard, TokenInfo};
use gloo_timers::callback::Timeout;
use leptos::prelude::*;

/// First paint — one full sprite sheet (4x4 grid)
const INITIAL_BATCH: usize = 16;
/// Delay between staggered batches (ms)
const STAGGER_MS: u32 = 50;

/// Static grid with exponentially ramping render.
///
/// 1. Renders first 16 cards immediately → fast LCP on first sprite sheet.
/// 2. Remaining cards mount in exponentially growing batches (2, 4, 8, 16...)
///    sprite sheets every 50ms — gentle ramp that avoids disrupting LCP.
/// 3. If filter/sort activates before full mount, dumps everything immediately.
#[component]
pub fn StaticGrid(
    slug: String,
    tokens: Vec<TokenInfo>,
    filter_css: Signal<String>,
    sort_css: Signal<String>,
    #[prop(optional, into)] owned_css: Option<Signal<String>>,
) -> impl IntoView {
    let total = tokens.len();
    let split = total.min(INITIAL_BATCH);

    let displayed = RwSignal::new(tokens[..split].to_vec());
    let remaining = StoredValue::new(tokens[split..].to_vec());
    let appended = RwSignal::new(split);
    let all_mounted = RwSignal::new(split >= total);

    // Mount all remaining cards when filter or sort becomes active
    Effect::new(move |_| {
        let fcss = filter_css.get();
        let scss = sort_css.get();
        if (!fcss.is_empty() || !scss.is_empty()) && !all_mounted.get_untracked() {
            mount_all(remaining, displayed, appended, total, all_mounted);
        }
    });

    // Start exponential ramp: 2 sheets, 4 sheets, 8 sheets...
    if split < total {
        schedule_batch(remaining, displayed, appended, total, all_mounted, 32);
    }

    view! {
        <style>{move || filter_css.get()}</style>
        <style>{move || sort_css.get()}</style>
        {owned_css.map(|sig| view! { <style>{move || sig.get()}</style> })}
        <div class="static-grid">
            <For
                each=move || displayed.get()
                key=|token| token.index
                children=move |token| {
                    let s = slug.clone();
                    view! { <NftCard slug=s token=token /> }
                }
            />
        </div>
    }
}

/// Schedule next batch with exponentially growing size
fn schedule_batch(
    remaining: StoredValue<Vec<TokenInfo>>,
    displayed: RwSignal<Vec<TokenInfo>>,
    appended: RwSignal<usize>,
    total: usize,
    all_mounted: RwSignal<bool>,
    batch_size: usize,
) {
    Timeout::new(STAGGER_MS, move || {
        if all_mounted.get_untracked() {
            return;
        }
        append_n(
            remaining,
            displayed,
            appended,
            total,
            all_mounted,
            batch_size,
        );
        if !all_mounted.get_untracked() {
            // Double the batch size each round
            schedule_batch(
                remaining,
                displayed,
                appended,
                total,
                all_mounted,
                batch_size * 2,
            );
        }
    })
    .forget();
}

/// Append N cards from remaining
fn append_n(
    remaining: StoredValue<Vec<TokenInfo>>,
    displayed: RwSignal<Vec<TokenInfo>>,
    appended: RwSignal<usize>,
    total: usize,
    all_mounted: RwSignal<bool>,
    count: usize,
) {
    let start = appended.get_untracked();
    if start >= total {
        all_mounted.set(true);
        return;
    }
    let rem_len = remaining.with_value(|r| r.len());
    let offset = start - (total - rem_len);
    let end = (offset + count).min(rem_len);
    remaining.with_value(|rem| {
        let chunk = &rem[offset..end];
        displayed.update(|v| v.extend_from_slice(chunk));
    });
    let new_appended = start + (end - offset);
    appended.set(new_appended);
    if new_appended >= total {
        all_mounted.set(true);
    }
}

/// Mount all remaining cards at once — needed when filter/sort activates
fn mount_all(
    remaining: StoredValue<Vec<TokenInfo>>,
    displayed: RwSignal<Vec<TokenInfo>>,
    appended: RwSignal<usize>,
    total: usize,
    all_mounted: RwSignal<bool>,
) {
    let start = appended.get_untracked();
    if start >= total {
        all_mounted.set(true);
        return;
    }
    let rem_len = remaining.with_value(|r| r.len());
    let offset = start - (total - rem_len);
    remaining.with_value(|rem| {
        let chunk = &rem[offset..];
        displayed.update(|v| v.extend_from_slice(chunk));
    });
    appended.set(total);
    all_mounted.set(true);
}
