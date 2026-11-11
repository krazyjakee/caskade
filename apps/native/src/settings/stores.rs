//! Settings → Store accounts (#104), as the webview's
//! `StoreAccountsSettings` and `AchievementSettings`: connect a store, see
//! each account's health, refresh, reconnect, disconnect and remove;
//! background refreshes and play time; achievements; what accounts do.
//! Admins only (the sections list hides it from others).

use std::collections::BTreeMap;

use caskade_core::store_accounts::{
    AuthFlow, AuthInput, AuthKind, ProviderCapabilities, RemovalPreview, StoreAccountSummary,
    StoreConnectionState,
};
use caskade_ui_data::ErrorReport;
use caskade_ui_data::stores::{Stores, StoresSnapshot};
use caskade_ui_i18n::{format, t, tn, tp};
use dioxus_native::prelude::*;
use serde_json::Value;

use super::state::{SettingsLoading, SettingsStatus, use_settings_section};
use crate::data::{change, use_stores, use_stores_query};
use crate::ui::icons::{EXTERNAL_LINK, KEY_ROUND, LINK_2, REFRESH_CW, STORE, TROPHY};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Dialog, DotTone, FailureNotice, Link, NoticeVariant, Select,
    SelectOption, SettingRow, SettingsSection, StatusDot, Switch, TextField,
};

fn state_label(state: StoreConnectionState) -> String {
    match state {
        StoreConnectionState::NotConnected => tp!("store account", "Disconnected"),
        StoreConnectionState::Authenticating => tp!("store account", "Signing in…"),
        StoreConnectionState::Connected => tp!("store account", "Connected"),
        StoreConnectionState::Syncing => tp!("store account", "Refreshing…"),
        StoreConnectionState::Offline => tp!("store account", "Offline"),
        StoreConnectionState::Expired => tp!("store account", "Sign-in expired"),
        StoreConnectionState::Unsupported => tp!("store account", "Not available"),
    }
}

fn state_tone(state: StoreConnectionState) -> DotTone {
    match state {
        StoreConnectionState::Connected | StoreConnectionState::Syncing => DotTone::Ok,
        StoreConnectionState::Authenticating | StoreConnectionState::Offline => DotTone::Warning,
        StoreConnectionState::Expired => DotTone::Danger,
        StoreConnectionState::NotConnected | StoreConnectionState::Unsupported => DotTone::Off,
    }
}

/// A field's input type: a secret is hidden as typed.
fn field_kind(secret: bool) -> &'static str {
    if secret { "password" } else { "text" }
}

fn when(ms: i64) -> String {
    format::date_time(crate::library::local_date_time(ms))
}

/// Runs a store action on the app's runtime, then reads the accounts again.
fn act<F, Fut>(
    stores: Option<Stores>,
    mut snapshot: Resource<Result<StoresSnapshot, ErrorReport>>,
    mut pending: Signal<bool>,
    mut failed: Signal<Option<ErrorReport>>,
    work: F,
) where
    F: FnOnce(Stores) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<(), ErrorReport>> + Send + 'static,
{
    let Some(stores) = stores else {
        return;
    };
    pending.set(true);
    failed.set(None);
    spawn(async move {
        let outcome = change(work(stores)).await;
        pending.set(false);
        if let Err(error) = outcome {
            failed.set(Some(error));
        }
        snapshot.restart();
    });
}

#[component]
pub fn StoreAccountsSettings() -> Element {
    let snapshot = use_stores_query(|stores| async move { stores.snapshot().await });
    let read = snapshot.read().clone();
    let Some(Ok(current)) = read else {
        let error = match read {
            Some(Err(error)) => Some(error),
            _ => None,
        };
        return rsx! {
            SettingsLoading { error, on_reload: move |_| { let mut snapshot = snapshot; snapshot.restart(); } }
        };
    };
    let unsupported: Vec<StoreAccountSummary> = current
        .accounts
        .iter()
        .filter(|summary| summary.capabilities.is_none())
        .cloned()
        .collect();
    rsx! {
        div { "data-settings": "stores", class: "flex flex-col gap-6",
            if current.providers.is_empty() {
                SettingsSection { icon: Some(STORE), title: t!("Store accounts"),
                    p { class: "text-sm text-fg-muted",
                        {t!("No store can be connected in this version of Caskade yet. Games your launchers have installed are still found by Library Discovery.")}
                    }
                }
            }
            for provider in current.providers.clone() {
                ProviderSection {
                    key: "{provider.id}",
                    accounts: current
                        .accounts
                        .iter()
                        .filter(|summary| summary.account.provider == provider.id)
                        .cloned()
                        .collect::<Vec<_>>(),
                    flow: current
                        .flows
                        .iter()
                        .find(|flow| flow.provider == provider.id && flow.reconnect.is_none())
                        .cloned(),
                    flows: current.flows.clone(),
                    snapshot,
                    provider,
                }
            }
            if !unsupported.is_empty() {
                SettingsSection {
                    icon: Some(STORE),
                    title: t!("Other accounts"),
                    description: t!("Their stores aren't available in this version of Caskade. Their games stay in your library."),
                    for summary in unsupported {
                        AccountRow {
                            key: "{summary.account.id}",
                            summary,
                            flows: current.flows.clone(),
                            snapshot,
                        }
                    }
                }
            }
            RefreshSettings {}
            AchievementSettings {}
            HelpSection {}
        }
    }
}

#[component]
fn ProviderSection(
    provider: ProviderCapabilities,
    accounts: Vec<StoreAccountSummary>,
    flow: Option<AuthFlow>,
    flows: Vec<AuthFlow>,
    snapshot: Resource<Result<StoresSnapshot, ErrorReport>>,
) -> Element {
    let stores = Some(use_stores());
    let pending = use_signal(|| false);
    let failed = use_signal(|| None::<ErrorReport>);
    let name = provider.name.clone();
    let description = if !provider.library {
        t!(
            "Shows your {name} achievements on the games in your library.",
            name = name.clone()
        )
    } else if provider.auth == AuthKind::Launcher {
        t!(
            "Reads the games the {name} launcher on this computer knows you own.",
            name = name.clone()
        )
    } else {
        t!(
            "Brings in every game your {name} account owns, installed or not.",
            name = name.clone()
        )
    };
    let begin = {
        let id = provider.id.clone();
        move |_| {
            let id = id.clone();
            act(
                stores.clone(),
                snapshot,
                pending,
                failed,
                move |stores| async move { stores.begin(&id, None).await.map(|_| ()) },
            );
        }
    };
    let connect_label = if accounts.is_empty() {
        t!("Connect")
    } else {
        t!("Connect another")
    };
    let aside = flow.is_none().then(|| {
        rsx! {
            Button {
                icon: Some(LINK_2),
                disabled: pending(),
                on_press: begin,
                "{connect_label}"
            }
        }
    });
    rsx! {
        SettingsSection {
            icon: Some(STORE),
            title: name.clone(),
            description,
            "data-part": "store-provider",
            "data-provider": "{provider.id}",
            aside,
            if let Some(flow) = flow.clone() {
                SignIn { key: "{flow.flow_id}", flow, provider: provider.clone(), snapshot }
            }
            if let Some(error) = failed() {
                FailureNotice { error, variant: NoticeVariant::Inline }
            }
            for summary in accounts.clone() {
                AccountRow { key: "{summary.account.id}", summary, flows: flows.clone(), snapshot }
            }
            if accounts.is_empty() && flow.is_none() {
                p { class: "text-sm text-fg-muted",
                    {t!("No {name} account is connected.", name = name.clone())}
                }
            }
        }
    }
}

/// A sign-in under way: what to do, for the provider's way of signing in.
#[component]
fn SignIn(
    flow: AuthFlow,
    provider: ProviderCapabilities,
    snapshot: Resource<Result<StoresSnapshot, ErrorReport>>,
) -> Element {
    let stores = Some(use_stores());
    let mut code = use_signal(String::new);
    let mut values = use_signal(BTreeMap::<String, String>::new);
    let pending = use_signal(|| false);
    let failed = use_signal(|| None::<ErrorReport>);
    let fields = flow.challenge.fields.clone();
    let filled = fields.iter().all(|field| {
        !values
            .read()
            .get(&field.id)
            .map(|value| value.trim())
            .unwrap_or_default()
            .is_empty()
    });
    let flow_id = flow.flow_id.clone();
    let complete = use_callback({
        let stores = stores.clone();
        let flow_id = flow_id.clone();
        move |input: AuthInput| {
            let flow_id = flow_id.clone();
            act(
                stores.clone(),
                snapshot,
                pending,
                failed,
                move |stores| async move { stores.complete(&flow_id, input).await },
            );
        }
    });
    // A launcher's sign-in needs nothing from the user; a device sign-in is
    // checked every few seconds until it's done.
    let kind = flow.kind;
    use_hook(move || {
        if kind == AuthKind::Launcher {
            complete(AuthInput::Launcher);
        }
        if kind == AuthKind::Device {
            spawn(async move {
                loop {
                    caskade_app::runtime::spawn(tokio::time::sleep(
                        std::time::Duration::from_secs(5),
                    ))
                    .await
                    .ok();
                    complete(AuthInput::Poll);
                }
            });
        }
    });
    let name = provider.name.clone();
    let cancel = {
        let stores = stores.clone();
        let flow_id = flow_id.clone();
        move |_| {
            if let Some(stores) = &stores {
                stores.cancel(&flow_id);
            }
            let mut snapshot = snapshot;
            snapshot.restart();
        }
    };
    // A device poll that isn't done yet says so as `busy`: not a failure.
    let shown_error = failed().filter(|error| error.code != caskade_ui_data::ErrorCode::Busy);
    let link_label = match kind {
        AuthKind::Device => t!("Open the activation page"),
        AuthKind::ApiKey => t!("Get a key"),
        _ => t!("Open the sign-in page"),
    };
    let code_label = if kind == AuthKind::ApiKey {
        t!("API key")
    } else if flow.challenge.instructions.is_some() {
        t!("Address or code")
    } else {
        t!("Or paste the code the page shows")
    };
    let submit_fields = move || {
        if filled {
            complete(AuthInput::Fields {
                fields: values.peek().clone(),
            });
        }
    };
    let submit_code = move || {
        let typed = code.peek().trim().to_owned();
        if !typed.is_empty() {
            complete(AuthInput::Code { code: typed });
        }
    };
    rsx! {
        section {
            "aria-label": t!("Sign in to {name}", name = name.clone()),
            "data-part": "sign-in",
            class: "mb-3 flex flex-col gap-3 rounded-control border border-border bg-surface-base/30 p-4",
            if let Some(instructions) = &flow.challenge.instructions {
                p { class: "text-sm", "{instructions}" }
            }
            if kind == AuthKind::Browser && flow.challenge.instructions.is_none() {
                p { class: "text-sm",
                    {t!("Sign in on {name}'s page in your browser. Caskade continues when the page sends you back.", name = name.clone())}
                }
            }
            if kind == AuthKind::Device {
                div { class: "flex flex-col gap-2 text-sm",
                    {t!("Open {name}'s activation page and enter this code:", name = name.clone())}
                    span { "data-part": "user-code", class: "font-mono text-2xl tracking-widest",
                        {flow.challenge.user_code.clone().unwrap_or_default()}
                    }
                }
            }
            if let Some(url) = flow.challenge.url.clone() {
                Link {
                    href: url.clone(),
                    class: "inline-flex items-center gap-2 text-sm text-accent-alt underline data-focus-visible:focus-ring",
                    on_press: move |_| {
                        if let Ok(url) = url::Url::parse(&url) {
                            caskade_app::navigation::open_in_browser(&url);
                        }
                    },
                    crate::ui::Svg { icon: EXTERNAL_LINK, class: "size-4" }
                    "{link_label}"
                }
            }
            if !fields.is_empty() {
                div { class: "flex flex-col gap-3",
                    for field in fields.clone() {
                        TextField {
                            key: "{field.id}",
                            label: field.label.clone(),
                            description: field.description.clone(),
                            kind: field_kind(field.secret),
                            icon: field.secret.then_some(KEY_ROUND),
                            value: values.read().get(&field.id).cloned().unwrap_or_default(),
                            on_input: {
                                let id = field.id.clone();
                                move |next: String| {
                                    values.write().insert(id.clone(), next);
                                }
                            },
                            on_submit: move |_| submit_fields(),
                        }
                    }
                    div {
                        Button {
                            variant: ButtonVariant::Primary,
                            disabled: pending() || !filled,
                            on_press: move |_| submit_fields(),
                            {t!("Connect")}
                        }
                    }
                }
            }
            if fields.is_empty() && matches!(kind, AuthKind::ApiKey | AuthKind::Browser) {
                div { class: "flex items-end gap-2",
                    TextField {
                        class: "flex-1",
                        label: code_label,
                        kind: "password",
                        icon: Some(KEY_ROUND),
                        value: code(),
                        on_input: move |next: String| code.set(next),
                        on_submit: move |_| submit_code(),
                    }
                    Button {
                        variant: ButtonVariant::Primary,
                        disabled: pending() || code().trim().is_empty(),
                        on_press: move |_| submit_code(),
                        {t!("Connect")}
                    }
                }
            }
            if kind == AuthKind::Device {
                p { role: "status", class: "text-xs text-fg-muted",
                    {t!("Waiting for you to finish on the other device…")}
                }
            }
            if let Some(error) = shown_error {
                FailureNotice { error, variant: NoticeVariant::Inline }
            }
            div {
                Button { size: ButtonSize::Sm, variant: ButtonVariant::Ghost, on_press: cancel,
                    {t!("Cancel sign-in")}
                }
            }
            p { class: "text-xs text-fg-muted",
                {t!(
                    "The sign-in expires {0}. Caskade keeps it in your system's keychain, never in its settings or backups.",
                    0 = when(flow.expires_at_ms)
                )}
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Asking {
    Disconnect,
    Remove,
}

#[component]
fn AccountRow(
    summary: StoreAccountSummary,
    flows: Vec<AuthFlow>,
    snapshot: Resource<Result<StoresSnapshot, ErrorReport>>,
) -> Element {
    let stores = Some(use_stores());
    let pending = use_signal(|| false);
    let failed = use_signal(|| None::<ErrorReport>);
    let mut dialog = use_signal(|| None::<Asking>);
    let account = summary.account.clone();
    let state = summary.state;
    let flow = flows
        .iter()
        .find(|flow| flow.reconnect.as_deref() == Some(&account.id))
        .cloned();
    let provider = summary.capabilities.clone();
    let supported = provider.is_some();
    let description = match account.sync.last_ok_ms {
        None => t!("Not refreshed yet."),
        Some(at) if provider.as_ref().is_some_and(|provider| !provider.library) => {
            t!("Achievements last refreshed {0}.", 0 = when(at))
        }
        Some(at) => t!(
            "{owned, plural, one {# game owned} other {# games owned}}, {installed} installed. Last refreshed {0}.",
            owned = summary.owned,
            installed = summary.installed,
            0 = when(at)
        ),
    };
    let id = account.id.clone();
    let refresh = {
        let stores = stores.clone();
        let id = id.clone();
        move |_| {
            let id = id.clone();
            act(
                stores.clone(),
                snapshot,
                pending,
                failed,
                move |stores| async move { stores.refresh(&id).await },
            );
        }
    };
    let stop = {
        let stores = stores.clone();
        let id = id.clone();
        move |_| {
            if let Some(stores) = &stores {
                stores.cancel_refresh(&id);
            }
        }
    };
    let reconnect = {
        let stores = stores.clone();
        let provider = account.provider.clone();
        let id = id.clone();
        move |_| {
            let (provider, id) = (provider.clone(), id.clone());
            act(
                stores.clone(),
                snapshot,
                pending,
                failed,
                move |stores| async move { stores.begin(&provider, Some(id)).await.map(|_| ()) },
            );
        }
    };
    rsx! {
        div { "data-part": "store-account", class: "border-t border-border py-3 first-of-type:border-t-0",
            SettingRow { label: account.display_name.clone(), description,
                StatusDot { tone: state_tone(state), {state_label(state)} }
                if state == StoreConnectionState::Syncing {
                    Button { size: ButtonSize::Sm, on_press: stop, {t!("Stop")} }
                } else {
                    Button {
                        size: ButtonSize::Sm,
                        icon: Some(REFRESH_CW),
                        "aria-label": t!("Refresh {0}", 0 = account.display_name.clone()),
                        disabled: !supported || !account.enabled || pending(),
                        on_press: refresh,
                        {t!("Refresh")}
                    }
                }
                if supported
                    && matches!(state, StoreConnectionState::Expired | StoreConnectionState::NotConnected)
                    && flow.is_none()
                {
                    Button {
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Primary,
                        disabled: pending(),
                        on_press: reconnect,
                        {t!("Reconnect")}
                    }
                }
                if account.enabled {
                    Button {
                        size: ButtonSize::Sm,
                        variant: ButtonVariant::Ghost,
                        on_press: move |_| dialog.set(Some(Asking::Disconnect)),
                        {t!("Disconnect…")}
                    }
                }
                Button {
                    size: ButtonSize::Sm,
                    variant: ButtonVariant::Ghost,
                    on_press: move |_| dialog.set(Some(Asking::Remove)),
                    {t!("Remove…")}
                }
            }
            if summary.session_only {
                p { class: "text-xs text-warning",
                    {t!("Your system has no keychain Caskade can use, so this sign-in lasts until Caskade closes.")}
                }
            }
            if summary.lost > 0 {
                p { class: "text-xs text-fg-muted",
                    {tn!(
                        "# game is no longer in this account; it stays in your library.",
                        "# games are no longer in this account; they stay in your library.",
                        summary.lost as usize
                    )}
                }
            }
            if let Some(error) = account.sync.error.clone().filter(|_| state != StoreConnectionState::NotConnected) {
                FailureNotice { error, variant: NoticeVariant::Inline }
            }
            if let (Some(flow), Some(provider)) = (flow.clone(), provider.clone()) {
                SignIn { key: "{flow.flow_id}", flow, provider, snapshot }
            }
            if let Some(error) = failed() {
                FailureNotice { error, variant: NoticeVariant::Inline }
            }
            match dialog() {
                Some(Asking::Disconnect) => rsx! {
                    DisconnectDialog {
                        summary: summary.clone(),
                        snapshot,
                        on_close: move |_| dialog.set(None),
                    }
                },
                Some(Asking::Remove) => rsx! {
                    RemoveDialog {
                        summary: summary.clone(),
                        snapshot,
                        on_close: move |_| dialog.set(None),
                    }
                },
                None => rsx! {},
            }
        }
    }
}

#[component]
fn DisconnectDialog(
    summary: StoreAccountSummary,
    snapshot: Resource<Result<StoresSnapshot, ErrorReport>>,
    on_close: EventHandler<()>,
) -> Element {
    let stores = Some(use_stores());
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<ErrorReport>);
    let name = summary.account.display_name.clone();
    let id = summary.account.id.clone();
    let disconnect = move |_| {
        let Some(stores) = stores.clone() else {
            return;
        };
        let id = id.clone();
        pending.set(true);
        failed.set(None);
        spawn(async move {
            let outcome = change(async move { stores.disconnect(&id).await }).await;
            pending.set(false);
            let mut snapshot = snapshot;
            snapshot.restart();
            match outcome {
                Ok(()) => on_close.call(()),
                Err(error) => failed.set(Some(error)),
            }
        });
    };
    rsx! {
        Dialog {
            title: t!("Disconnect {name}?", name = name.clone()),
            alert: true,
            on_close: move |_| {
                if !pending() {
                    on_close.call(());
                }
            },
            p { class: "text-sm text-fg-muted",
                {t!("Caskade signs out of it and stops refreshing it. Its games stay in your library, and connecting it again brings it back as it was. Remove it to take its games out.")}
            }
            if let Some(error) = failed() {
                FailureNotice { error, variant: NoticeVariant::Inline }
            }
            div { class: "flex justify-end gap-2",
                Button { "data-focus-default": "true", disabled: pending(), on_press: move |_| on_close.call(()), {t!("Cancel")} }
                Button { variant: ButtonVariant::Danger, disabled: pending(), on_press: disconnect, {t!("Disconnect")} }
            }
        }
    }
}

#[component]
fn RemoveDialog(
    summary: StoreAccountSummary,
    snapshot: Resource<Result<StoresSnapshot, ErrorReport>>,
    on_close: EventHandler<()>,
) -> Element {
    let stores = Some(use_stores());
    let id = summary.account.id.clone();
    let preview = use_resource({
        let stores = stores.clone();
        let id = id.clone();
        move || {
            let stores = stores.clone().expect("the data source's store accounts");
            let id = id.clone();
            change(async move { stores.removal_preview(&id).await })
        }
    });
    let mut pending = use_signal(|| false);
    let mut failed = use_signal(|| None::<ErrorReport>);
    let name = summary.account.display_name.clone();
    let remove = move |_| {
        let Some(stores) = stores.clone() else {
            return;
        };
        let id = id.clone();
        pending.set(true);
        failed.set(None);
        spawn(async move {
            let outcome = change(async move { stores.remove(&id).await }).await;
            pending.set(false);
            let mut snapshot = snapshot;
            snapshot.restart();
            match outcome {
                Ok(_) => on_close.call(()),
                Err(error) => failed.set(Some(error)),
            }
        });
    };
    let read: Option<Result<RemovalPreview, ErrorReport>> = preview.read().clone();
    let error = failed().or(match &read {
        Some(Err(error)) => Some(error.clone()),
        _ => None,
    });
    let ready = matches!(read, Some(Ok(_)));
    rsx! {
        Dialog {
            title: t!("Remove {name}?", name = name.clone()),
            alert: true,
            on_close: move |_| {
                if !pending() {
                    on_close.call(());
                }
            },
            match read {
                Some(Ok(preview)) => rsx! {
                    div { class: "flex flex-col gap-2 text-sm text-fg-muted",
                        p {
                            {tn!(
                                "# game only this account brought leaves your library.",
                                "# games only this account brought leave your library.",
                                preview.removed.len()
                            )}
                            " "
                            {tn!(
                                "# game stays, because it's installed, added another way or owned by another account.",
                                "# games stay, because they're installed, added another way or owned by another account.",
                                preview.kept.len()
                            )}
                        }
                        if !preview.removed.is_empty() {
                            ul {
                                "aria-label": t!("Games that leave the library"),
                                class: "max-h-40 overflow-y-auto pl-5 text-xs",
                                for game in preview.removed.clone() {
                                    li { key: "{game.game_id}", class: "list-disc", "{game.title}" }
                                }
                            }
                        }
                    }
                },
                _ => rsx! {
                    p { class: "text-sm text-fg-muted", {t!("Checking what would change…")} }
                },
            }
            if let Some(error) = error {
                FailureNotice { error, variant: NoticeVariant::Inline }
            }
            div { class: "flex justify-end gap-2",
                Button { "data-focus-default": "true", disabled: pending(), on_press: move |_| on_close.call(()), {t!("Cancel")} }
                Button { variant: ButtonVariant::Danger, disabled: !ready || pending(), on_press: remove, {t!("Remove")} }
            }
        }
    }
}

#[component]
fn RefreshSettings() -> Element {
    let preferences = use_settings_section("core.storeAccounts");
    let title = t!("Refreshing");
    let description = t!(
        "Refresh connected accounts in the background to bring in games you buy. Failures show on the account; nothing pops up."
    );
    // Loading says nothing; a failed load says why, with Try again (#55).
    let Some(settings) = preferences.value() else {
        return if preferences.load_error().is_some() {
            rsx! {
                SettingsSection { icon: Some(REFRESH_CW), title: title.clone(), description,
                    SettingsStatus { section: preferences, what: title }
                }
            }
        } else {
            rsx! {}
        };
    };
    let background = settings
        .get("backgroundRefresh")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let hours = settings
        .get("intervalHours")
        .and_then(Value::as_u64)
        .unwrap_or(24);
    let mut options: Vec<SelectOption> = [6u64, 12, 24, 48, 168]
        .into_iter()
        .map(|every| {
            SelectOption::new(
                every.to_string(),
                if every == 168 {
                    t!("Weekly")
                } else {
                    t!("Every {hours} hours", hours = every)
                },
            )
        })
        .collect();
    if !options
        .iter()
        .any(|option| option.value == hours.to_string())
    {
        options.push(SelectOption::new(
            hours.to_string(),
            t!("Every {0} hours", 0 = hours),
        ));
    }
    let playtime = vec![
        SelectOption::new("always", t!("On every refresh")),
        SelectOption::new("newGames", t!("For new games only")),
        SelectOption::new("never", tp!("import play time", "Never")),
    ];
    rsx! {
        SettingsSection { icon: Some(REFRESH_CW), title: title.clone(), description,
            SettingsStatus { section: preferences, what: title }
            SettingRow { label: t!("Refresh in the background"),
                Switch {
                    checked: background,
                    on_change: move |on: bool| preferences.set_field("backgroundRefresh", Value::Bool(on)),
                }
            }
            SettingRow { label: t!("How often"),
                Select {
                    disabled: !background,
                    options,
                    value: Some(hours.to_string()),
                    on_change: move |next: String| {
                        if let Ok(hours) = next.parse::<u64>() {
                            preferences.set_field("intervalHours", Value::from(hours));
                        }
                    },
                    class: "w-44",
                }
            }
            // After the two refresh rows it doesn't belong between (#54).
            SettingRow {
                label: t!("Import play time"),
                description: t!("Play time a store reports is added to what Caskade tracked, counting time played through Caskade once. A store that lowers its total never lowers yours."),
                Select {
                    options: playtime,
                    value: Some(
                        settings
                            .get("playtimeImport")
                            .and_then(Value::as_str)
                            .unwrap_or("always")
                            .to_owned(),
                    ),
                    on_change: move |next: String| preferences.set_field("playtimeImport", Value::String(next)),
                    class: "w-52",
                }
            }
        }
    }
}

/// Achievements (`core.achievements`, #185/#190): whether to read them,
/// refresh them in the background, and notify new unlocks.
#[component]
fn AchievementSettings() -> Element {
    let section = use_settings_section("core.achievements");
    let Some(settings) = section.value() else {
        return rsx! {};
    };
    let flag = |key: &str| settings.get(key).and_then(Value::as_bool).unwrap_or(false);
    let enabled = flag("enabled");
    rsx! {
        SettingsSection {
            icon: Some(TROPHY),
            title: t!("Achievements"),
            description: t!("Game pages show the achievements of connected accounts. Caskade only reads them; nothing is unlocked or changed at a store."),
            SettingRow { label: t!("Show achievements"),
                Switch { checked: enabled, on_change: move |on: bool| section.set_field("enabled", Value::Bool(on)) }
            }
            SettingRow {
                label: t!("Refresh in the background"),
                description: t!("After a game exits, and now and then for games played lately."),
                Switch {
                    checked: flag("background"),
                    disabled: !enabled,
                    on_change: move |on: bool| section.set_field("background", Value::Bool(on)),
                }
            }
            SettingRow {
                label: t!("Notify new unlocks"),
                description: t!("A notification for achievements unlocked since the last refresh. The first refresh of a game brings its history in without notifications."),
                Switch {
                    checked: flag("notifyUnlocks"),
                    disabled: !enabled,
                    on_change: move |on: bool| section.set_field("notifyUnlocks", Value::Bool(on)),
                }
            }
        }
    }
}

#[component]
fn HelpSection() -> Element {
    rsx! {
        SettingsSection { icon: Some(KEY_ROUND), title: t!("About store accounts"),
            ul { class: "flex list-disc flex-col gap-2 pl-5 text-sm text-fg-muted",
                li { {t!("Games an account owns join your library even when they aren't installed. When a launcher installs one, Caskade shows it as the same game.")} }
                li { {t!("Sign-ins are kept in your system's keychain, never in Caskade's settings or backups. After restoring a backup or moving computer, reconnect each account.")} }
                li { {t!("Disconnecting keeps the account's games. Removing it takes out the games only it brought; installed games stay.")} }
            }
        }
    }
}
