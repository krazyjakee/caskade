//! The top bar (`shell/Topbar.tsx`, `ActivityLink.tsx`): the search, the
//! Activity button and, where Caskade draws its own title bar, the window's
//! controls. Dragging anywhere but a control moves the window, and a double
//! click maximizes it (`window.rs`).

use crate::data::status::{is_active, use_jobs, use_unread_notifications};
use crate::router::RouteLink;
use crate::ui::{Svg, icons};
use caskade_core::install::JobStatus;
use caskade_ui_i18n::{tn, tp};
use dioxus_native::prelude::*;

#[component]
pub fn Topbar() -> Element {
    rsx! {
        header {
            "data-cs": "topbar",
            "data-window-drag": "true",
            class: "relative z-10 flex h-topbar shrink-0 items-center gap-4 pr-4 pl-6",
            super::search::SearchBox {}
            div { "data-part": "actions", class: "ml-auto flex items-center gap-4",
                ActivityLink {}
            }
            super::window::WindowControls {}
        }
    }
}

/// The bell: Activity, with the number of jobs under way and, apart from
/// them, the unread notifications. It opens the jobs while one waits for an
/// answer, else the notifications while some are unread.
#[component]
pub fn ActivityLink() -> Element {
    let jobs = use_jobs();
    let unread = use_unread_notifications();
    let jobs = jobs
        .read()
        .as_ref()
        .and_then(|jobs| jobs.as_ref().ok().cloned().flatten())
        .unwrap_or_default();
    let unread = unread
        .read()
        .as_ref()
        .and_then(|unread| unread.as_ref().ok().copied().flatten())
        .unwrap_or(0);
    let active = jobs.iter().filter(|job| is_active(job)).count();
    let waiting = jobs
        .iter()
        .filter(|job| job.status == JobStatus::NeedsUser)
        .count();
    let mut label = vec![tp!("navigation", "Activity")];
    if active > 0 {
        label.push(tn!("# active job", "# active jobs", active));
    }
    if waiting > 0 {
        label.push(tn!(
            note = "Jobs waiting for an answer, after the Activity button's name",
            "# needs your attention",
            "# need your attention",
            waiting
        ));
    }
    if unread > 0 {
        label.push(tn!(
            "# unread notification",
            "# unread notifications",
            unread
        ));
    }
    let tab = if waiting == 0 && unread > 0 {
        "notifications"
    } else {
        "jobs"
    };
    rsx! {
        RouteLink {
            to: "/activity?tab={tab}",
            "aria-label": label.join(", "),
            "data-spatial": "focusable",
            "data-part": "activity",
            class: "relative rounded-control p-2 text-fg-muted data-focus-visible:focus-ring",
            Svg { icon: icons::BELL, class: "size-5" }
            if active > 0 {
                span {
                    "data-part": "job-count",
                    "data-status": if waiting > 0 { "needs-user" } else { "active" },
                    class: "absolute -top-1 -right-1 min-w-4 rounded-full bg-accent-alt px-1 text-center font-mono text-[10px] text-accent-fg",
                    "{active}"
                }
            }
            if unread > 0 {
                span {
                    "data-part": "notification-count",
                    class: "absolute -right-1 -bottom-1 min-w-4 rounded-full border border-surface bg-accent px-1 text-center font-mono text-[10px] text-accent-fg",
                    if unread > 99 { "99+" } else { "{unread}" }
                }
            }
        }
    }
}
