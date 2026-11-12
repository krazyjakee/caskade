//! The app's frame around every view (#96, `shell/AppShell.tsx`): the
//! sidebar, the top bar with the search, the page the router shows, the
//! status bar, toasts, the type-to-search overlay and the window's own
//! title bar; a view that fails shows its error in place. Console mode's
//! shell is #107, the account gate and menu `crate::accounts` (#106), plugins' links and slots #109.

mod anatomy;
mod error;
mod notifications;
mod search;
#[cfg(feature = "shots")]
pub mod shot;
mod sidebar;
mod status_bar;
mod topbar;
mod type_search;
mod views;
pub mod window;

#[cfg(test)]
pub(crate) mod tests;

use crate::router::{RouterRoot, use_router};
use crate::ui::ToastRegion;
pub use anatomy::ANATOMY;
use dioxus_native::prelude::dioxus_html::input_data::MouseButton;
use dioxus_native::prelude::*;
pub(crate) use sidebar::Brand;
pub(crate) use type_search::PageSearch;

/// The frame, holding navigation.
#[component]
pub fn Frame(
    /// The page to open on, in place of the "Open to" setting (tests).
    #[props(default)]
    start: Option<String>,
) -> Element {
    // Until the library opens (`DataRoot` mounts it again then), as the
    // webview renders once its data's there.
    if try_consume_context::<crate::data::DataSource>().is_none() {
        return rsx! {};
    }
    rsx! {
        ErrorBoundary { handle_error: error::app_crash,
            RouterRoot { start,
                ToastRegion {
                    // Nobody signed in: the account picker instead (#106).
                    crate::accounts::AccountGate {
                    type_search::TypeSearchRoot {
                        // Pairing, remote play and stream requests (#108).
                        crate::flows::Hosts {}
                        // A followed job's questions (#101).
                        crate::activity::JobQuestionHost {}
                        // Install requests waiting for the user (#101).
                        crate::activity::InstallIngress {}
                        // What plugins draw (#109), for every slot.
                        crate::plugins::PluginRoot {
                            crate::game_menu::GameMenuHost {
                                Shell {}
                            }
                        }
                    }
                    }
                }
            }
        }
    }
}

#[component]
fn Shell() -> Element {
    let router = use_router();
    #[cfg(test)]
    tests::ROUTER.with(|seen| seen.set(Some(router)));
    // Keyed by visit, so each view enters again on navigation.
    let key = router.key();
    let drag = window::use_drag();
    rsx! {
        div {
            "data-cs": "app-shell",
            class: "relative flex h-screen flex-col overflow-clip",
            onmounted: move |event| drag.mounted(&event),
            onmousedown: move |event: MouseEvent| {
                // A mouse's back and forward buttons go through history.
                match event.trigger_button() {
                    Some(MouseButton::Fourth) => {
                        router.back();
                    }
                    Some(MouseButton::Fifth) => {
                        router.forward();
                    }
                    _ => drag.down(&event),
                }
            },
            onmousemove: move |event| drag.moved(&event),
            onmouseup: move |_| drag.up(),
            div { "data-cs": "backdrop", "aria-hidden": "true" }
            div { class: "flex min-h-0 flex-1",
                sidebar::Sidebar {}
                div { "data-part": "main", class: "relative flex min-w-0 flex-1 flex-col",
                    topbar::Topbar {}
                    main {
                        "data-cs": "view",
                        "data-focus-section": "main",
                        class: "min-h-0 flex-1 overflow-y-auto px-6 pb-8",
                        div { key: "{key}", class: "flex min-h-full animate-enter flex-col",
                            ErrorBoundary { handle_error: error::view_error,
                                views::Outlet {}
                            }
                        }
                    }
                }
            }
            status_bar::StatusBar {}
            window::ResizeEdges {}
        }
    }
}

/// A menu's anchor at the pointer, for a right click inside `within`'s
/// document.
pub(crate) fn pointer_anchor(
    within: Option<&dioxus_native::NodeHandle>,
    event: &MouseEvent,
) -> crate::ui::Anchor {
    let point = event.client_coordinates();
    let viewport_height = within.map_or(0.0, |handle| {
        let doc = handle.doc();
        let viewport = doc.viewport();
        f64::from(viewport.window_size.1) / viewport.scale_f64()
    });
    crate::ui::Anchor {
        x: point.x,
        y: point.y,
        width: 0.0,
        height: 0.0,
        viewport_height,
    }
}
