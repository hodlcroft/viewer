use leptos::prelude::*;
use wallet_leptos::{ConnectionState, WalletProviderEnum, try_use_wallet};

/// Wallet connect/disconnect button for the gallery header.
///
/// Shows a dropdown of available wallets when disconnected,
/// and a truncated address with disconnect when connected.
#[component]
pub fn WalletButton() -> impl IntoView {
    let Some(wallet) = try_use_wallet() else {
        return view! {}.into_any();
    };

    let (show_dropdown, set_show_dropdown) = signal(false);

    let toggle_dropdown = move |_| {
        set_show_dropdown.update(|v| *v = !*v);
    };

    let close_dropdown = move || {
        set_show_dropdown.set(false);
    };

    view! {
        <div class="wallet-button-container">
            {move || {
                let state = wallet.connection_state.get();
                let wallet = wallet.clone();
                match state {
                    ConnectionState::Disconnected => {
                        let wallets = wallet.available_wallets.get();
                        if wallets.is_empty() {
                            view! {
                                <button class="wallet-btn wallet-btn-disabled" disabled=true>
                                    "No Wallets"
                                </button>
                            }.into_any()
                        } else {
                            view! {
                                <div class="wallet-dropdown-wrapper">
                                    <button class="wallet-btn" on:click=toggle_dropdown>
                                        "Connect Wallet"
                                    </button>
                                    <Show when=move || show_dropdown.get()>
                                        <div class="wallet-dropdown">
                                            {wallets.iter().map(|info| {
                                                let api_name = info.api_name.clone();
                                                let display_name = info.name.clone();
                                                let icon = info.icon.clone();
                                                let wallet = wallet.clone();
                                                view! {
                                                    <button
                                                        class="wallet-dropdown-item"
                                                        on:click=move |_| {
                                                            if let Some(provider) = WalletProviderEnum::from_api_name(&api_name) {
                                                                wallet.connect(provider);
                                                                close_dropdown();
                                                            }
                                                        }
                                                    >
                                                        <img class="wallet-icon" src=icon.clone() alt=display_name.clone() />
                                                        <span>{display_name.clone()}</span>
                                                    </button>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    </Show>
                                </div>
                            }.into_any()
                        }
                    }
                    ConnectionState::Connecting => {
                        view! {
                            <button class="wallet-btn wallet-btn-connecting" disabled=true>
                                <span class="spinner spinner-tiny"></span>
                                " Connecting..."
                            </button>
                        }.into_any()
                    }
                    ConnectionState::Connected { .. } => {
                        // Prefer ADA handle > stake address > truncated hex
                        let display_addr = wallet.handle.get()
                            .or_else(|| wallet.stake_address.get())
                            .unwrap_or_else(|| {
                                wallet.address.get_untracked()
                                    .map(|a| if a.len() > 16 {
                                        format!("{}...{}", &a[..8], &a[a.len()-8..])
                                    } else {
                                        a
                                    })
                                    .unwrap_or_default()
                            });
                        let full_addr = display_addr.clone();
                        let short_addr = if display_addr.len() > 20 {
                            format!("{}...{}", &display_addr[..12], &display_addr[display_addr.len()-6..])
                        } else {
                            display_addr
                        };
                        view! {
                            <div class="wallet-connected">
                                <span class="wallet-address" title=full_addr>{short_addr}</span>
                                <button class="wallet-btn wallet-btn-disconnect" on:click=move |_| wallet.disconnect()>
                                    "Disconnect"
                                </button>
                            </div>
                        }.into_any()
                    }
                    ConnectionState::Error(e) => {
                        view! {
                            <div class="wallet-error-state">
                                <span class="wallet-error-msg" title=e>"Wallet Error"</span>
                                <button class="wallet-btn" on:click=toggle_dropdown>
                                    "Retry"
                                </button>
                            </div>
                        }.into_any()
                    }
                }
            }}
        </div>
    }.into_any()
}
