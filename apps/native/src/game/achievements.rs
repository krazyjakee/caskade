//! A game's achievements on its page (#100, the webview's
//! `components/achievements/GameAchievements.tsx`): one store account's set
//! at a time, its counts and how its last fetch went, Refresh (or Stop
//! refreshing), and every achievement with its state, progress, unlock
//! time, hardcore unlock, rarity and points where the service gives them.
//! Hidden achievements stay concealed until unlocked or revealed. Icons
//! are drawn (a trophy, a lock, a concealed mark); the services' own
//! pictures aren't fetched natively yet.

use super::data::{AchievementIcon, use_answer, use_page_data};
use crate::input::use_input;
use crate::router::{Route, use_router};
use crate::ui::{Button, ButtonSize, ButtonVariant, Segment, SegmentedControl, Svg, Switch, icons};
use caskade_achievements::achievements::{Achievement, AchievementSet, SetStatus};
use caskade_ui_data::Key as DataKey;
use caskade_ui_i18n::{format, t, tn, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;
use std::collections::HashSet;
use std::rc::Rc;

/// The achievements' heading, which the hero's summary moves to: the page
/// provides it, the panel fills it in.
#[derive(Clone, Copy)]
pub struct AchievementsHeading(pub Signal<Option<Rc<MountedData>>>);

/// A game's achievement counts in its hero, across its sets; pressing it
/// moves to the achievements.
#[component]
pub fn AchievementSummary(game_id: String) -> Element {
    let data = use_page_data();
    let heading = try_use_context::<AchievementsHeading>();
    let sets = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.achievements)(game_id.clone())
        })
    };
    let counted: Vec<AchievementSet> = sets
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|set| set.total > 0)
        .collect();
    if counted.is_empty() {
        return rsx! {};
    }
    let unlocked: u32 = counted.iter().map(|set| set.unlocked).sum();
    let total: u32 = counted.iter().map(|set| set.total).sum();
    rsx! {
        button {
            r#type: "button",
            "data-cs": "achievement-summary",
            "data-spatial": "focusable",
            "aria-label": t!("{unlocked} of {total} achievements unlocked. Go to achievements", unlocked = unlocked.to_string(), total = total.to_string()),
            class: "inline-flex items-center gap-1.5 rounded-control px-1 text-fg-muted outline-none hover:text-fg data-focus-visible:focus-ring",
            onclick: move |_| {
                let Some(mounted) = heading.and_then(|heading| heading.0.peek().clone()) else {
                    return;
                };
                spawn(async move {
                    let _ = mounted.scroll_to(ScrollBehavior::Instant).await;
                    let _ = mounted.set_focus(true).await;
                });
            },
            Svg { icon: icons::TROPHY, class: "size-4" }
            span { {t!("{unlocked}/{total} achievements", unlocked = unlocked.to_string(), total = total.to_string())} }
        }
    }
}

fn set_key(set: &AchievementSet) -> String {
    format!("{}:{}", set.account.store_account_id, set.product_id)
}

fn status_name(status: SetStatus) -> &'static str {
    match status {
        SetStatus::NotFetched => "not_fetched",
        SetStatus::Ready => "ready",
        SetStatus::NoAchievements => "no_achievements",
        SetStatus::Unsupported => "unsupported",
        SetStatus::Private => "private",
        SetStatus::SignInNeeded => "sign_in_needed",
    }
}

#[component]
pub fn GameAchievements(game_id: String) -> Element {
    let data = use_page_data();
    let sets = {
        let (data, game_id) = (data.clone(), game_id.clone());
        use_answer(DataKey::Game(game_id.clone()), move || {
            (data.achievements)(game_id.clone())
        })
    };
    let heading = try_use_context::<AchievementsHeading>();
    let mut chosen = use_signal(|| None::<String>);
    let mut refreshing = use_signal(|| false);
    let mut refresh_error = use_signal(|| None::<String>);
    let read = sets.read();
    let state: Option<Result<Vec<AchievementSet>, String>> = read
        .as_ref()
        .map(|result| result.clone().map_err(|report| report.summary));
    drop(read);
    let list = match state {
        None => {
            return rsx! {
                section { "data-cs": "achievements", "data-state": "loading", class: "mt-8",
                    p { role: "status", class: "text-sm text-fg-muted", {t!("Loading achievements…")} }
                }
            };
        }
        Some(Err(error)) => {
            return rsx! {
                section { "data-cs": "achievements", "data-state": "error", class: "mt-8",
                    p { role: "alert", class: "text-sm text-danger",
                        {t!("Achievements couldn't be read: {error}", error = error)}
                    }
                    Button {
                        on_press: move |_| {
                            let mut sets = sets;
                            sets.restart();
                        },
                        {t!("Try again")}
                    }
                }
            };
        }
        Some(Ok(list)) => list,
    };
    // No connected account offers this game's achievements.
    if list.is_empty() {
        return rsx! {};
    }
    let set = chosen()
        .and_then(|key| list.iter().find(|set| set_key(set) == key))
        .unwrap_or(&list[0])
        .clone();
    let refresh = {
        let (data, game_id) = (data.clone(), game_id.clone());
        move |_| {
            let (data, game_id) = (data.clone(), game_id.clone());
            refreshing.set(true);
            refresh_error.set(None);
            spawn(async move {
                if let Err(report) = (data.achievements_refresh)(game_id).await {
                    refresh_error.set(Some(report.summary));
                }
                refreshing.set(false);
            });
        }
    };
    let cancel = {
        let (data, game_id) = (data.clone(), game_id.clone());
        move |_| {
            let (data, game_id) = (data.clone(), game_id.clone());
            spawn(async move {
                let _ = (data.achievements_cancel)(game_id).await;
            });
        }
    };
    let status = status_name(set.sync.status);
    let current_key = set_key(&set);
    rsx! {
        section {
            "data-cs": "achievements",
            "data-state": status,
            "aria-labelledby": "achievements-heading",
            class: "mt-8 flex flex-col gap-4",
            div { class: "flex flex-wrap items-center gap-3",
                h2 {
                    id: "achievements-heading",
                    tabindex: "-1",
                    class: "flex-1 font-display text-xl font-semibold outline-none",
                    onmounted: move |event: Event<MountedData>| {
                        if let Some(heading) = heading {
                            let mut slot = heading.0;
                            slot.set(Some(event.data()));
                        }
                    },
                    {t!("Achievements")}
                }
                if refreshing() {
                    Button { "data-part": "cancel", "data-spatial": "focusable", on_press: cancel, {t!("Stop refreshing")} }
                } else {
                    Button {
                        "data-part": "refresh",
                        "data-spatial": "focusable",
                        variant: ButtonVariant::Secondary,
                        icon: Some(icons::REFRESH_CW),
                        on_press: refresh,
                        {t!("Refresh achievements")}
                    }
                }
            }
            if refreshing() {
                p { role: "status", class: "text-sm text-fg-muted", {t!("Refreshing achievements…")} }
            }
            if let Some(error) = refresh_error() {
                p { role: "alert", class: "text-sm text-danger", {t!("The refresh didn't finish: {error}", error = error)} }
            }
            if list.len() > 1 {
                SegmentedControl {
                    "data-part": "sets",
                    "aria-label": t!("Achievements of"),
                    value: current_key.clone(),
                    on_change: move |key: String| chosen.set(Some(key)),
                    options: list.iter().map(|each| Segment::new(
                        set_key(each),
                        t!("{provider} · {name}", provider = each.account.provider_name.clone(), name = each.account.display_name.clone()),
                    )).collect::<Vec<_>>(),
                }
            }
            for set in [set] {
                SetView { key: "{set_key(&set)}", game_id: game_id.clone(), set }
            }
        }
    }
}

#[component]
fn SetView(game_id: String, set: AchievementSet) -> Element {
    let router = use_router();
    let provider = set.account.provider_name.clone();
    let name = set.account.display_name.clone();
    let sync = set.sync.clone();
    let mut account = t!(
        "{provider} account {name}",
        provider = provider.clone(),
        name = name.clone()
    );
    if let Some(at) = sync.fetched_at_ms {
        account.push_str(&format!(
            " · {}",
            t!(
                "updated {when}",
                when = format::date_time(crate::library::local_date_time(at))
            )
        ));
    }
    rsx! {
        p { "data-part": "account", class: "text-sm text-fg-muted", "{account}" }
        if let Some(error) = sync.error.clone() {
            p { "data-part": "stale", role: "status", class: "text-sm text-warning",
                match sync.fetched_at_ms {
                    None => t!("The last refresh failed: {reason}", reason = error.summary),
                    Some(at) => t!(
                        "Showing achievements from {when}. The last refresh failed: {reason}",
                        when = format::date_time(crate::library::local_date_time(at)),
                        reason = error.summary
                    ),
                }
            }
        }
        if let Some(at) = sync.retry_at_ms {
            p { "data-part": "wait", class: "text-sm text-fg-muted",
                {t!("{provider} asked Caskade to wait; refreshing works again after {when}.", provider = provider.clone(), when = format::date_time(crate::library::local_date_time(at)))}
            }
        }
        match sync.status {
            SetStatus::NotFetched => rsx! { p { "data-part": "notice", class: "text-sm text-fg-muted", {t!("Not fetched yet. Refresh achievements to see them.")} } },
            SetStatus::NoAchievements => rsx! { p { "data-part": "notice", class: "text-sm text-fg-muted", {t!("{provider} lists no achievements for this game.", provider = provider.clone())} } },
            SetStatus::Unsupported => rsx! { p { "data-part": "notice", class: "text-sm text-fg-muted", {t!("{provider} doesn't offer this game's achievements.", provider = provider.clone())} } },
            SetStatus::Private => rsx! {
                p { "data-part": "notice", class: "text-sm text-fg-muted",
                    {t!("{name}'s achievements are private on {provider}. Make your game details public there, then refresh.", name = name.clone(), provider = provider.clone())}
                }
            },
            SetStatus::SignInNeeded => rsx! {
                div { "data-part": "notice", class: "flex flex-wrap items-center gap-3",
                    p { class: "text-sm text-fg-muted", {t!("Sign in to {provider} as {name} again to see these achievements.", provider = provider.clone(), name = name.clone())} }
                    Button {
                        "data-part": "reconnect",
                        size: ButtonSize::Sm,
                        on_press: move |_| router.navigate(&Route::Settings { tab: "stores", section: None }.href()),
                        {t!("Open store accounts")}
                    }
                }
            },
            SetStatus::Ready => rsx! { Achievements { game_id: game_id.clone(), set: set.clone() } },
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Show {
    All,
    Unlocked,
    Locked,
}

#[component]
fn Achievements(game_id: String, set: AchievementSet) -> Element {
    let mut show = use_signal(|| Show::All);
    let mut reveal_all = use_signal(|| false);
    let mut revealed = use_signal(HashSet::<String>::new);
    let capabilities = set.capabilities.clone();
    let hidden_locked = set
        .achievements
        .iter()
        .filter(|each| each.definition.hidden && !each.state.unlocked)
        .count();
    let points = capabilities.points.then(|| {
        set.achievements
            .iter()
            .fold((0u32, 0u32), |(earned, total), each| {
                let points = each.definition.points.unwrap_or(0);
                (
                    earned + if each.state.unlocked { points } else { 0 },
                    total + points,
                )
            })
    });
    let shown: Vec<Achievement> = set
        .achievements
        .iter()
        .filter(|each| match show() {
            Show::All => true,
            Show::Unlocked => each.state.unlocked,
            Show::Locked => !each.state.unlocked,
        })
        .cloned()
        .collect();
    let percent = (set.unlocked * 100 + set.total.max(1) / 2) / set.total.max(1);
    let mut summary = t!(
        "{unlocked} of {total} unlocked ({percent}%)",
        unlocked = set.unlocked.to_string(),
        total = set.total.to_string(),
        percent = percent.to_string()
    );
    if let Some(hardcore) = set.hardcore_unlocked {
        summary.push_str(&format!(
            " · {}",
            t!("{count} in hardcore", count = hardcore.to_string())
        ));
    }
    if let Some((earned, total)) = points {
        summary.push_str(&format!(
            " · {}",
            t!(
                "{earned} of {total} points",
                earned = earned.to_string(),
                total = total.to_string()
            )
        ));
    }
    let width = format!("{}%", set.unlocked * 100 / set.total.max(1));
    rsx! {
        div { "data-part": "summary", class: "flex flex-col gap-1.5",
            p { class: "text-sm", "{summary}" }
            div {
                role: "meter",
                "aria-label": t!("Achievements unlocked"),
                "aria-valuemin": "0",
                "aria-valuemax": "{set.total.max(1)}",
                "aria-valuenow": "{set.unlocked}",
                class: "h-1.5 w-full max-w-md overflow-hidden rounded-full bg-surface-hover",
                div { class: "h-full rounded-full bg-accent", style: "width: {width}" }
            }
        }
        div { class: "flex flex-wrap items-center gap-3",
            SegmentedControl {
                "data-part": "filter",
                "aria-label": t!("Show"),
                value: match show() { Show::All => "all", Show::Unlocked => "unlocked", Show::Locked => "locked" },
                on_change: move |id: String| show.set(match id.as_str() { "unlocked" => Show::Unlocked, "locked" => Show::Locked, _ => Show::All }),
                options: vec![
                    Segment::new("all", tp!("achievements", "All")),
                    Segment::new("unlocked", tp!("achievements", "Unlocked")),
                    Segment::new("locked", tp!("achievements", "Locked")),
                ],
            }
            if hidden_locked > 0 {
                Switch {
                    "data-part": "reveal-all",
                    checked: reveal_all(),
                    label: tn!("Show # hidden achievement", "Show # hidden achievements", hidden_locked),
                    on_change: move |on: bool| reveal_all.set(on),
                }
            }
        }
        if shown.is_empty() {
            p { class: "text-sm text-fg-subtle",
                if show() == Show::Unlocked { {t!("Nothing unlocked yet.")} } else { {t!("Everything is unlocked.")} }
            }
        } else {
            ul { "data-part": "list", "aria-label": t!("Achievements"), class: "flex flex-col gap-2",
                for achievement in shown {
                    Row {
                        key: "{achievement.definition.id}",
                        icon: AchievementIcon {
                            game_id: game_id.clone(),
                            store_account_id: set.account.store_account_id.clone(),
                            product_id: set.product_id.clone(),
                            achievement_id: achievement.definition.id.clone(),
                            locked: !achievement.state.unlocked,
                        },
                        provider: set.account.provider_name.clone(),
                        capabilities: capabilities.clone(),
                        revealed: reveal_all() || revealed.read().contains(&achievement.definition.id),
                        on_reveal: {
                            let id = achievement.definition.id.clone();
                            move |_| { revealed.write().insert(id.clone()); }
                        },
                        achievement,
                    }
                }
            }
        }
    }
}

#[component]
fn Row(
    achievement: Achievement,
    /// Its picture's request.
    icon: AchievementIcon,
    provider: String,
    capabilities: caskade_achievements::achievements::AchievementCapabilities,
    revealed: bool,
    on_reveal: EventHandler<()>,
) -> Element {
    let definition = &achievement.definition;
    let state = &achievement.state;
    let concealed = definition.hidden && !state.unlocked && !revealed;
    // The service's picture once the row nears the screen; the drawn icon
    // when there is none, it can't load, or it would give a hidden one
    // away.
    let wanted = !concealed
        && if state.unlocked {
            definition.icon_url.is_some()
        } else {
            definition.locked_icon_url.is_some() || definition.icon_url.is_some()
        };
    let (near, mut row) = use_near();
    let data = use_page_data();
    let picture = use_resource(use_reactive!(|(wanted, icon)| {
        let data = data.clone();
        let go = wanted && near();
        async move {
            if go {
                (data.achievement_icon)(icon).await.ok().flatten()
            } else {
                None
            }
        }
    }));
    let picture = picture.read().clone().flatten().filter(|_| wanted);
    let greyed = !state.unlocked && definition.locked_icon_url.is_none();
    let name = if concealed {
        t!("Hidden achievement")
    } else {
        definition.name.clone()
    };
    let description = if concealed {
        Some(t!(
            "Its name and description are a spoiler until you unlock it."
        ))
    } else {
        definition.description.clone().or_else(|| {
            (definition.hidden && !state.unlocked).then(|| {
                t!(
                    "{provider} shows its description once it's unlocked.",
                    provider = provider.clone()
                )
            })
        })
    };
    let hardcore = if capabilities.hardcore {
        state.hardcore.clone()
    } else {
        None
    };
    let when = |ms: i64| format::date_time(crate::library::local_date_time(ms));
    let unlocked_line = if state.unlocked {
        match state.unlocked_at_ms.filter(|_| capabilities.unlock_times) {
            Some(at) => t!("Unlocked {when}", when = when(at)),
            None => t!("Unlocked"),
        }
    } else {
        t!("Locked")
    };
    let hardcore_line = hardcore.as_ref().and_then(|hardcore| {
        if hardcore.unlocked {
            Some(match hardcore.unlocked_at_ms {
                Some(at) => t!("Hardcore {when}", when = when(at)),
                None => t!("Hardcore"),
            })
        } else {
            state.unlocked.then(|| t!("Softcore only"))
        }
    });
    let rarity = definition
        .rarity_percent
        .filter(|_| capabilities.rarity)
        .map(|percent| {
            t!(
                "{percent}% of players",
                percent = format::number(f64::from(percent), 0, 1)
            )
        });
    let points = definition
        .points
        .filter(|_| capabilities.points)
        .map(|points| tn!("# point", "# points", points as usize));
    let progress = (capabilities.progress && !state.unlocked)
        .then_some(())
        .and(definition.progress_max.zip(state.progress));
    let drawn = if concealed {
        icons::EYE_OFF
    } else if state.unlocked {
        icons::TROPHY
    } else {
        icons::LOCK
    };
    let label = (!concealed).then(|| {
        format!(
            "{name}, {}",
            if state.unlocked {
                t!("unlocked")
            } else {
                t!("locked")
            }
        )
    });
    rsx! {
        li {
            "data-part": "achievement",
            "data-state": if state.unlocked { "unlocked" } else { "locked" },
            "data-concealed": concealed.then_some("true"),
            "data-hardcore": hardcore.as_ref().is_some_and(|hardcore| hardcore.unlocked).then_some("true"),
            "aria-label": label,
            class: "flex items-start gap-3 rounded-control border border-border bg-surface-raised p-3 outline-none data-[state=locked]:text-fg-muted",
            onmounted: move |event| row.set(event.data().downcast::<NodeHandle>().cloned()),
            span { "aria-hidden": "true", "data-part": "icon", class: "grid size-12 shrink-0 place-items-center overflow-hidden rounded-control border border-border bg-surface-base text-fg-subtle",
                if let Some(url) = picture {
                    img { src: "{url}", alt: "", class: if greyed { "size-full object-cover grayscale" } else { "size-full object-cover" } }
                } else {
                    Svg { icon: drawn, class: "size-5" }
                }
            }
            div { class: "min-w-0 flex-1",
                p { class: "font-semibold", "{name}" }
                if let Some(description) = description {
                    p { class: "text-sm text-fg-muted", "{description}" }
                }
                p { "data-part": "details", class: "mt-1 flex flex-wrap gap-x-3 text-xs text-fg-subtle",
                    span { "{unlocked_line}" }
                    if let Some(line) = hardcore_line {
                        span { "data-part": "hardcore", "{line}" }
                    }
                    if let Some(rarity) = rarity {
                        span { "{rarity}" }
                    }
                    if let Some(points) = points {
                        span { "{points}" }
                    }
                }
                if let Some((max, value)) = progress {
                    div { class: "mt-1.5 flex items-center gap-2 text-xs text-fg-subtle",
                        div {
                            role: "meter",
                            "aria-label": t!("Progress"),
                            class: "h-1.5 w-full max-w-md overflow-hidden rounded-full bg-surface-hover",
                            div { class: "h-full rounded-full bg-accent", style: "width: {(value / max.max(1.0) * 100.0).min(100.0)}%" }
                        }
                        span { {format!("{}/{}", format::number(value, 0, 0), format::number(max, 0, 0))} }
                    }
                }
            }
            if concealed {
                Button {
                    "data-part": "reveal",
                    "data-spatial": "focusable",
                    size: ButtonSize::Sm,
                    on_press: move |_| on_reveal.call(()),
                    {t!("Reveal")}
                }
            }
        }
    }
}

/// The game page moved (a wheel turned over it): rows waiting to come near
/// the screen look again. The input layer's own scrolls say so too.
#[derive(Clone, Copy)]
pub struct PageMoved(pub Signal<u64>);

/// Whether the element mounted into the signal returned has come within
/// 400 px of the window; it stays true once it has.
fn use_near() -> (Signal<bool>, Signal<Option<NodeHandle>>) {
    let input = use_input();
    let moved = try_use_context::<PageMoved>();
    let mut near = use_signal(|| false);
    let row = use_signal(|| None::<NodeHandle>);
    use_effect(move || {
        let _ = input.scrolled();
        if let Some(moved) = moved {
            let _ = (moved.0)();
        }
        let Some(handle) = row.read().clone() else {
            return;
        };
        if *near.peek() {
            return;
        }
        let doc = handle.doc();
        let viewport = doc.viewport();
        let height = f64::from(viewport.window_size.1) / viewport.scale_f64();
        if let Some(rect) = doc.get_client_bounding_rect(handle.node_id())
            && rect.y < height + 400.0
            && rect.y + rect.height > -400.0
        {
            near.set(true);
        }
    });
    (near, row)
}
