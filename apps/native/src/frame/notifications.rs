//! Activity's frame and its notification feed (`views/activity/
//! ActivityView.tsx`, `NotificationFeed.tsx`, #96): the header, the Jobs
//! and Notifications tabs, and the feed. The jobs panel is #101's; until
//! then its tab says it isn't built yet.
//!
//! Showing the feed marks nothing read: an item is read once it's opened,
//! or marked read.

use crate::data::status::{
    NOTIFICATION_PAGE, ReadChange, change_read, is_active, use_jobs, use_notification_members,
    use_notifications, use_unread_notifications,
};
use crate::library::local_date_time;
use crate::router::{ActivityTab, Route, RouteLink, href::Href, use_router};
use crate::ui::{Button, ButtonVariant, Svg, icons};
use caskade_achievements::notifications::{
    MemberKind, MemberState, Notification, NotificationCategory, NotificationMember,
    NotificationTarget, Severity,
};
use caskade_core::install::JobStatus;
use caskade_ui_i18n::format;
use caskade_ui_i18n::intl::Clock;
use caskade_ui_i18n::{t, tn};
use dioxus_native::prelude::*;

/// Where Back from a game opened here goes.
const FROM: &str = "/activity?tab=notifications";

fn category_label(category: NotificationCategory) -> String {
    match category {
        NotificationCategory::GamesAdded => {
            t!(note = "A notification's category", "Games added")
        }
        NotificationCategory::JobFailed => t!(note = "A notification's category", "Failed job"),
        NotificationCategory::UpdatesAvailable => {
            t!(note = "A notification's category", "Updates")
        }
        NotificationCategory::PluginDisabled => t!(note = "A notification's category", "Plugin"),
        NotificationCategory::ScriptFailed => t!(note = "A notification's category", "Script"),
        NotificationCategory::AchievementsUnlocked => {
            t!(note = "A notification's category", "Achievements")
        }
        NotificationCategory::ShareReceived | NotificationCategory::ShareArrived => {
            t!(note = "A notification's category", "Shared")
        }
    }
}

/// When a batch's additions happened: one time, a span within a day, or
/// across days. Never "the last hour": the range stays true later.
pub fn time_range(first: i64, last: i64) -> String {
    let day = |ms: i64| format::day(local_date_time(ms));
    let time = |ms: i64| format::time(local_date_time(ms), Clock::System, false);
    let at = |ms: i64| format!("{}, {}", day(ms), time(ms));
    if last - first < 60_000 {
        at(first)
    } else if day(first) == day(last) {
        format!("{}–{}", at(first), time(last))
    } else {
        format!("{} – {}", at(first), at(last))
    }
}

/// Activity: its jobs and its notifications.
#[component]
pub fn Activity(route: Route) -> Element {
    let Route::Activity { tab, unread, open } = route else {
        return rsx! {};
    };
    let jobs = use_jobs();
    let unread_count = use_unread_notifications();
    let jobs = jobs
        .read()
        .as_ref()
        .and_then(|jobs| jobs.as_ref().ok().cloned().flatten())
        .unwrap_or_default();
    let unread_count = unread_count
        .read()
        .as_ref()
        .and_then(|count| count.as_ref().ok().copied().flatten())
        .unwrap_or(0);
    let active = jobs.iter().filter(|job| is_active(job)).count();
    let waiting = jobs
        .iter()
        .filter(|job| job.status == JobStatus::NeedsUser)
        .count();
    let tabs = [
        (
            ActivityTab::Jobs,
            "jobs",
            t!(
                note = "An Activity tab: installs and other background jobs",
                "Jobs"
            ),
            active,
            if waiting > 0 {
                tn!(
                    "{active} active, # needs you",
                    "{active} active, # need you",
                    waiting,
                    active = active
                )
            } else {
                tn!("# active", "# active", active)
            },
        ),
        (
            ActivityTab::Notifications,
            "notifications",
            t!(note = "An Activity tab", "Notifications"),
            unread_count as usize,
            tn!("# unread", "# unread", unread_count),
        ),
    ];
    let panel_label = tabs
        .iter()
        .find(|(id, ..)| *id == tab)
        .map(|(_, _, label, ..)| label.clone())
        .unwrap_or_default();
    rsx! {
        div { "data-view": "activity", "data-focus-scope": "activity", class: "mt-4",
            header { class: "mb-6 flex flex-wrap items-center justify-between gap-4",
                div {
                    p { class: "font-mono text-xs tracking-widest text-fg-subtle uppercase",
                        {t!("Jobs and notifications")}
                    }
                    h1 { class: "mt-1 font-display text-3xl font-semibold", {t!("Activity")} }
                    p { class: "mt-2 text-sm text-fg-muted",
                        {tn!("# active job", "# active jobs", active)}
                        " · "
                        {tn!("# unread notification", "# unread notifications", unread_count)}
                    }
                }
            }
            div { role: "tablist", "aria-label": t!("Activity"), class: "mb-6 flex flex-wrap gap-2",
                for (id, name, label, count, detail) in tabs {
                    RouteLink {
                        key: "{name}",
                        to: "/activity?tab={name}",
                        current: id == tab,
                        role: "tab",
                        "aria-selected": if id == tab { "true" } else { "false" },
                        "aria-label": t!(
                            note = "An Activity tab's name and what it holds: Jobs, 2 active",
                            "{label}, {detail}",
                            label = label,
                            detail = detail
                        ),
                        "data-selected": (id == tab).then_some("true"),
                        class: "rounded-full border border-border bg-surface-raised px-4 py-1.5 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:text-fg data-focus-visible:focus-ring data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg",
                        "{label} "
                        span { class: "font-mono text-xs text-fg-subtle", "{count}" }
                    }
                }
            }
            div { role: "tabpanel", "aria-label": "{panel_label}",
                match tab {
                    ActivityTab::Jobs => super::views::placeholder(t!("Jobs")),
                    ActivityTab::Notifications => rsx! {
                        Feed { key: "{unread}", unread_only: unread, open }
                    },
                }
            }
        }
    }
}

/// The notification feed, beside the jobs in Activity.
#[component]
fn Feed(unread_only: bool, open: Option<String>) -> Element {
    let mut offset = use_signal(|| 0u32);
    let unread_signal = use_signal(|| unread_only);
    let feed = use_notifications(unread_signal.into(), offset.into());
    let mut action_error = use_signal(|| None::<String>);
    let mut pending = use_signal(|| false);
    let page = feed.read().clone();
    let (page, error, loading) = match page {
        // No services (fixtures mode): an empty feed.
        Some(Ok(page)) => (
            Some(
                page.unwrap_or(caskade_achievements::notifications::NotificationPage {
                    items: Vec::new(),
                    total: 0,
                    unread: 0,
                    revision: 0,
                }),
            ),
            None,
            false,
        ),
        Some(Err(error)) => (None, Some(error.summary.clone()), false),
        None => (None, None, true),
    };
    let shown = page.as_ref().map_or(0, |page| page.items.len() as u32);
    let first = offset() + 1;
    let last = offset() + shown;
    let mark_all = use_callback(move |revision: i64| {
        pending.set(true);
        spawn(async move {
            let result = change_read(ReadChange::AllRead { revision }).await;
            action_error.set(result.err().map(|error| error.summary));
            pending.set(false);
        });
    });
    let filters = [
        (
            t!(
                note = "The notification feed's filter: every notification",
                "All"
            ),
            false,
        ),
        (t!(note = "The notification feed's filter", "Unread"), true),
    ];
    rsx! {
        section { "data-cs": "notifications", "aria-label": t!("Notifications"),
            div { "data-part": "toolbar", class: "mb-4 flex flex-wrap items-center justify-between gap-3",
                div { class: "flex gap-2",
                    for (label, unread) in filters {
                        RouteLink {
                            key: "{unread}",
                            to: Href::new("/activity")
                                .text("tab", Some("notifications"))
                                .with("unread", unread.then_some(crate::router::href::Value::Bool(true)))
                                .to_string(),
                            current: unread == unread_only,
                            "data-part": "filter",
                            "data-spatial": "focusable",
                            "data-selected": (unread == unread_only).then_some("true"),
                            class: "rounded-control border border-border px-3 py-1 text-sm text-fg-muted hover:text-fg data-focus-visible:focus-ring data-selected:border-accent-alt/50 data-selected:text-fg",
                            "{label}"
                        }
                    }
                }
                Button {
                    "data-part": "mark-all",
                    "data-spatial": "focusable",
                    variant: ButtonVariant::Ghost,
                    disabled: page.as_ref().is_none_or(|page| page.unread == 0) || pending(),
                    on_press: {
                        let revision = page.as_ref().map(|page| page.revision);
                        move |_| {
                            if let Some(revision) = revision {
                                mark_all(revision);
                            }
                        }
                    },
                    Svg { icon: icons::CHECK_CHECK, class: "size-4" }
                    " "
                    {t!("Mark all read")}
                }
            }
            if loading {
                p { role: "status", class: "text-fg-muted", {t!("Loading notifications…")} }
            }
            if let Some(error) = error {
                div { role: "alert", class: "rounded-panel border border-border p-4",
                    p { {t!("Could not load notifications.")} }
                    p { class: "mt-1 text-sm text-fg-muted", "{error}" }
                    Button {
                        class: "mt-3",
                        on_press: move |_| {
                            let mut feed = feed;
                            feed.restart();
                        },
                        {t!("Try again")}
                    }
                }
            }
            if let Some(error) = action_error() {
                p { role: "alert", class: "mb-4 text-danger", "{error}" }
            }
            if let Some(page) = &page {
                if page.total == 0 {
                    p {
                        "data-part": "empty",
                        class: "rounded-panel border border-dashed border-border p-12 text-center text-fg-muted",
                        if unread_only {
                            {t!("No unread notifications.")}
                        } else {
                            {t!("No notifications yet. New games, failed jobs and updates show here.")}
                        }
                    }
                }
                ol { class: "space-y-3",
                    for item in page.items.iter().cloned() {
                        li { key: "{item.id}",
                            Item {
                                open: open.as_deref() == Some(item.id.as_str()),
                                item,
                                pending: pending(),
                                on_error: move |error: Option<String>| action_error.set(error),
                            }
                        }
                    }
                }
                if page.total > NOTIFICATION_PAGE {
                    nav {
                        "data-part": "pages",
                        "aria-label": t!("Notification pages"),
                        class: "mt-4 flex items-center justify-between gap-3 text-sm text-fg-muted",
                        Button {
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            disabled: offset() == 0,
                            on_press: move |_| offset.set(offset().saturating_sub(NOTIFICATION_PAGE)),
                            {t!("Newer")}
                        }
                        span {
                            {t!(
                                note = "The notifications shown, of how many: 1–20 of 45",
                                "{first}–{last} of {total}",
                                first = first,
                                last = last,
                                total = page.total
                            )}
                        }
                        Button {
                            "data-spatial": "focusable",
                            variant: ButtonVariant::Ghost,
                            disabled: offset() + NOTIFICATION_PAGE >= page.total,
                            on_press: move |_| offset.set(offset() + NOTIFICATION_PAGE),
                            {t!("Older")}
                        }
                    }
                }
            }
        }
    }
}

fn open_label(item: &Notification, open: bool) -> String {
    match &item.target {
        NotificationTarget::Members if open => t!("Hide list"),
        NotificationTarget::Members if item.category == NotificationCategory::GamesAdded => {
            t!("Show games")
        }
        NotificationTarget::Members => t!("Show updates"),
        NotificationTarget::Game { .. } => t!("Open game"),
        NotificationTarget::Jobs => t!("Open jobs"),
        NotificationTarget::Settings { .. } => t!("Open settings"),
    }
}

/// Where an item's target is, as an href.
pub fn target_href(item: &Notification, open: bool) -> String {
    match &item.target {
        // The list opens in place.
        NotificationTarget::Members => Href::new("/activity")
            .text("tab", Some("notifications"))
            .text("open", (!open).then_some(item.id.as_str()))
            .to_string(),
        NotificationTarget::Game { game_id } => Route::Game {
            id: game_id.clone(),
            from: Some(FROM.to_owned()),
        }
        .href(),
        NotificationTarget::Jobs => "/activity?tab=jobs".to_owned(),
        NotificationTarget::Settings { tab, section } => Href::new("/settings")
            .text("tab", Some(tab))
            .text("section", section.as_deref())
            .to_string(),
    }
}

#[component]
fn Item(
    item: Notification,
    open: bool,
    pending: bool,
    on_error: EventHandler<Option<String>>,
) -> Element {
    let router = use_router();
    let heading = format!("notification-{}", item.id);
    let change = move |change: ReadChange| {
        spawn(async move {
            let result = change_read(change).await;
            on_error.call(result.err().map(|error| error.summary));
        });
    };
    let mark_read = {
        let (id, version) = (item.id.clone(), item.version);
        move || {
            change(ReadChange::Read {
                id: id.clone(),
                version,
            })
        }
    };
    let toggle_read = {
        let (id, unread) = (item.id.clone(), item.unread);
        let mark_read = mark_read.clone();
        move || {
            if unread {
                mark_read();
            } else {
                change(ReadChange::Unread { id: id.clone() });
            }
        }
    };
    // Opening reads it: only the version shown, so later arrivals stay
    // unread.
    let open_item = {
        let item = item.clone();
        move || {
            if item.unread {
                mark_read();
            }
            let href = target_href(&item, open);
            if matches!(item.target, NotificationTarget::Members) {
                router.replace(&href);
            } else {
                router.navigate(&href);
            }
        }
    };
    let when = if item.category == NotificationCategory::GamesAdded {
        time_range(item.first_at_ms, item.last_at_ms)
    } else {
        time_range(item.updated_at_ms, item.updated_at_ms)
    };
    let severity = match item.severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    };
    let title = item.title.clone();
    let title_class = format!(
        "mt-1 break-words font-display text-lg {} {}",
        if item.unread { "font-semibold" } else { "" },
        if item.severity == Severity::Error && !item.resolved {
            "text-danger"
        } else {
            ""
        }
    );
    rsx! {
        article {
            "aria-labelledby": "{heading}",
            "data-cs": "notification",
            "data-status": if item.unread { "unread" } else { "read" },
            "data-severity": severity,
            "data-category": category_name(item.category),
            "data-resolved": item.resolved.then_some("true"),
            class: "rounded-panel border border-border bg-surface-raised p-4 data-[status=unread]:border-accent-alt/50",
            div { class: "flex flex-wrap items-start justify-between gap-3",
                div { class: "flex min-w-0 gap-3",
                    span {
                        "data-part": "indicator",
                        "aria-hidden": "true",
                        class: if item.unread { "mt-2 size-2 shrink-0 rounded-full bg-accent-alt" } else { "mt-2 size-2 shrink-0 rounded-full bg-transparent" },
                    }
                    div { class: "min-w-0",
                        p { "data-part": "meta", class: "font-mono text-xs text-fg-subtle uppercase",
                            {category_label(item.category)}
                            " · "
                            time { "data-part": "time", "{when}" }
                            if item.resolved {
                                {format!(" · {}", t!(note = "A notification whose problem was fixed", "Resolved"))}
                            }
                            if item.unread {
                                span { class: "sr-only",
                                    {format!(" · {}", t!(note = "A notification not read yet", "Unread"))}
                                }
                            }
                        }
                        h2 { id: "{heading}", "data-part": "title", class: "{title_class}", "{item.title}" }
                        if !item.body.is_empty() {
                            p { "data-part": "body", class: "mt-1 break-words text-sm text-fg-muted", "{item.body}" }
                        }
                    }
                }
                div { "data-part": "actions", class: "flex gap-2",
                    Button {
                        "data-spatial": "focusable",
                        variant: if item.unread { ButtonVariant::Primary } else { ButtonVariant::Secondary },
                        "aria-expanded": matches!(item.target, NotificationTarget::Members).then_some(if open { "true" } else { "false" }),
                        on_press: move |_| open_item(),
                        {open_label(&item, open)}
                    }
                    Button {
                        "data-spatial": "focusable",
                        variant: ButtonVariant::Ghost,
                        "aria-label": if item.unread { t!("Mark read: {title}", title = title) } else { t!("Mark unread: {title}", title = title) },
                        disabled: pending,
                        on_press: move |_| toggle_read(),
                        Svg { icon: if item.unread { icons::MAIL_OPEN } else { icons::BELL_DOT }, class: "size-4" }
                        if item.unread { {t!("Mark read")} } else { {t!("Mark unread")} }
                    }
                }
            }
            if open && matches!(item.target, NotificationTarget::Members) {
                Members { id: item.id.clone() }
            }
        }
    }
}

fn category_name(category: NotificationCategory) -> &'static str {
    match category {
        NotificationCategory::GamesAdded => "gamesAdded",
        NotificationCategory::JobFailed => "jobFailed",
        NotificationCategory::UpdatesAvailable => "updatesAvailable",
        NotificationCategory::PluginDisabled => "pluginDisabled",
        NotificationCategory::ScriptFailed => "scriptFailed",
        NotificationCategory::AchievementsUnlocked => "achievementsUnlocked",
        NotificationCategory::ShareReceived => "shareReceived",
        NotificationCategory::ShareArrived => "shareArrived",
    }
}

const MEMBER_PAGE: u32 = 50;

/// What an item is about: a batch's games, each linked to its page (or the
/// game it was merged into), or the updates on offer.
#[component]
fn Members(id: String) -> Element {
    let mut limit = use_signal(|| MEMBER_PAGE);
    let members = use_notification_members(id, limit.into());
    let page = members.read().clone();
    rsx! {
        div { "data-part": "members", class: "mt-4 border-t border-border pt-3",
            match page {
                None => rsx! {
                    p { role: "status", class: "text-sm text-fg-muted", {t!("Loading…")} }
                },
                Some(Err(_)) => rsx! {
                    div { role: "alert", class: "text-sm",
                        p { {t!("Could not load the list.")} }
                        Button {
                            class: "mt-2",
                            on_press: move |_| {
                                let mut members = members;
                                members.restart();
                            },
                            {t!("Try again")}
                        }
                    }
                },
                Some(Ok(page)) => {
                    let page = page.unwrap_or(caskade_achievements::notifications::MemberPage {
                        items: Vec::new(),
                        total: 0,
                    });
                    let left = page.total.saturating_sub(page.items.len() as u32);
                    rsx! {
                        if page.total == 0 {
                            p { class: "text-sm text-fg-muted", {t!("Nothing left to show: they have all gone.")} }
                        }
                        ul { class: "grid gap-1 sm:grid-cols-2",
                            for member in page.items.iter().cloned() {
                                li {
                                    key: "{member.kind:?}:{member.at_ms}:{member.label}",
                                    "data-part": "member",
                                    "data-status": member_state(member.state),
                                    class: "text-sm",
                                    MemberLink { member: member.clone() }
                                    if let Some(source) = &member.source {
                                        span { class: "ml-2 font-mono text-xs text-fg-subtle", "{source}" }
                                    }
                                }
                            }
                        }
                        if left > 0 {
                            Button {
                                class: "mt-3",
                                "data-spatial": "focusable",
                                variant: ButtonVariant::Ghost,
                                on_press: move |_| limit.set(limit() + MEMBER_PAGE),
                                {tn!("Show more (# left)", "Show more (# left)", left)}
                            }
                        }
                    }
                }
            }
        }
    }
}

fn member_state(state: MemberState) -> &'static str {
    match state {
        MemberState::Present => "present",
        MemberState::Merged => "merged",
        MemberState::Removed => "removed",
        MemberState::Other => "other",
    }
}

#[component]
fn MemberLink(member: NotificationMember) -> Element {
    const LINK: &str =
        "rounded-control text-accent-alt hover:underline data-focus-visible:focus-ring";
    if member.state == MemberState::Removed {
        return rsx! {
            span { class: "text-fg-subtle",
                "{member.label} "
                span { class: "text-xs", {t!("(no longer in the library)")} }
            }
        };
    }
    // Streaming's programs are updated in Settings → Streaming (#266).
    if member.state == MemberState::Other {
        let to = if member.kind == MemberKind::ToolUpdate {
            "/settings?tab=streaming"
        } else {
            "/settings?tab=emulators&section=emulators"
        };
        return rsx! {
            RouteLink { to, current: false, "data-spatial": "focusable", class: LINK, "{member.label}" }
        };
    }
    let Some(game_id) = member.game_id.clone() else {
        return rsx! {
            span { "{member.label}" }
        };
    };
    let to = Route::Game {
        id: game_id,
        from: Some(FROM.to_owned()),
    }
    .href();
    rsx! {
        RouteLink { to, current: false, "data-spatial": "focusable", class: LINK,
            "{member.label}"
            if let Some(into) = member.title.as_ref().filter(|_| member.state == MemberState::Merged) {
                " "
                span { class: "text-xs text-fg-subtle",
                    {t!(
                        note = "A game added to the library that was later merged into another game",
                        "(now part of {into})",
                        into = into
                    )}
                }
            }
        }
    }
}
