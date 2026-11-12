//! The top bar (`shell/Topbar.tsx`, `ActivityLink.tsx`): the search, the
//! Activity button, the account menu (#106) and, where Caskade draws its own title bar, the window's
//! controls. Dragging anywhere but a control moves the window, and a double
//! click maximizes it (`window.rs`).

use crate::data::status::{is_active, use_jobs, use_unread_notifications};
use crate::router::RouteLink;
use crate::ui::{Anchor, Menu, MenuEntry, MenuItem, Svg, icons};
use caskade_core::install::JobStatus;
use caskade_ui_i18n::{t, tn, tp};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

/// A right click on the bar itself, not on a control, offers the window's
/// own menu, as a native title bar does.
fn window_menu(maximized: bool) -> Vec<MenuEntry> {
    vec![
        MenuEntry::Item(MenuItem {
            icon: Some(icons::MINUS),
            ..MenuItem::new("minimize", tp!("window", "Minimize"))
        }),
        MenuEntry::Item(if maximized {
            MenuItem {
                icon: Some(icons::COPY),
                ..MenuItem::new("maximize", tp!("window", "Restore"))
            }
        } else {
            MenuItem {
                icon: Some(icons::SQUARE),
                ..MenuItem::new("maximize", tp!("window", "Maximize"))
            }
        }),
        MenuEntry::Item(MenuItem {
            icon: Some(icons::X),
            ..MenuItem::new("close", tp!("window", "Close"))
        }),
    ]
}

#[component]
pub fn Topbar() -> Element {
    let maximized = super::window::use_maximized();
    let mut bar = use_signal(|| None::<NodeHandle>);
    let mut menu = use_signal(|| None::<Anchor>);
    rsx! {
        header {
            "data-cs": "topbar",
            "data-window-drag": "true",
            class: "relative z-10 flex h-topbar shrink-0 items-center gap-4 pr-4 pl-6",
            onmounted: move |event: Event<MountedData>| bar.set(event.data().downcast::<NodeHandle>().cloned()),
            oncontextmenu: move |event: MouseEvent| {
                if !super::window::CUSTOM_FRAME {
                    return;
                }
                let on_bar = bar.peek().as_ref().is_some_and(|bar| {
                    let doc = bar.doc();
                    doc.get_hover_node_id()
                        .is_some_and(|hovered| super::window::in_drag_region(&doc, hovered))
                });
                if on_bar {
                    menu.set(Some(super::pointer_anchor(bar.peek().as_ref(), &event)));
                }
            },
            super::search::SearchBox {}
            div { "data-part": "actions", class: "ml-auto flex items-center gap-4",
                // Plugins' items (#109).
                crate::plugins::TopbarItems {}
                ActivityLink {}
                crate::accounts::AccountMenu {}
                // Console mode (#107), where Caskade draws no window
                // controls of its own (macOS); otherwise they have it.
                if !super::window::CUSTOM_FRAME {
                    button {
                        "data-part": "console",
                        r#type: "button",
                        "aria-label": t!("Console mode"),
                        class: "grid size-8 place-items-center rounded-control text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover hover:text-fg data-focus-visible:focus-ring",
                        onclick: move |_| crate::console::set_mode(crate::console::Mode::Console, true),
                        crate::ui::Svg { icon: crate::ui::icons::GAMEPAD_2, class: "size-4", stroke_width: 1.75 }
                    }
                }
            }
            super::window::WindowControls {}
            if let Some(anchor) = menu() {
                Menu {
                    anchor,
                    items: window_menu(maximized),
                    label: t!(note = "The title bar's right-click menu", "Window"),
                    on_close: move |_| menu.set(None),
                    on_action: move |id: String| {
                        menu.set(None);
                        match id.as_str() {
                            "minimize" => {
                                if let Some(window) = super::window::window() {
                                    window.set_minimized(true);
                                }
                            }
                            "maximize" => super::window::toggle_maximized(),
                            _ => crate::launch::ask(crate::launch::Request::Close),
                        }
                    },
                }
            }
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
