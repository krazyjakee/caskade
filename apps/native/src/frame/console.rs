//! Console mode's shell (#107; the webview's `shell/console/`): the same
//! routes and views as the desktop, with the console sidebar (big targets,
//! the way back to the desktop), the console top bar (search, Activity, the
//! account), the button-hint bar, and the curtain over a running game. The
//! window is fullscreen.

use caskade_ui_i18n::t;
use caskade_ui_input::Action;
use dioxus_native::NodeHandle;
use dioxus_native::prelude::*;

use super::sidebar::{LibraryNav, PRIMARY, SECONDARY};
use crate::console::HintBar;
use crate::input::{Bind, use_input, use_input_action};
use crate::router::RouteLink;
use crate::ui::{Svg, icons};

/// How long the pointer stays after it last moved.
const CURSOR_IDLE: std::time::Duration = std::time::Duration::from_millis(2500);

const ITEM: &str = "relative flex items-center gap-4 rounded-control px-4 py-3 text-lg text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover hover:text-fg data-focus-visible:bg-surface-hover data-focus-visible:text-fg data-[status=active]:bg-accent/20 data-[status=active]:text-fg";

/// Hints for what confirm and back always do, menus and dialogs included;
/// the work stays the defaults' (press the focused control, go back or
/// close). LT and RT page whatever list or view focus is in.
#[component]
fn ShellBindings() -> Element {
    use_input_action(
        Action::Confirm,
        Bind {
            label: Some(caskade_ui_i18n::tp!(
                note = "Hint bar: confirm the focused item",
                "pad action",
                "Select"
            )),
            global: true,
            ..Bind::default()
        },
        || false,
    );
    use_input_action(
        Action::Back,
        Bind {
            label: Some(caskade_ui_i18n::tp!("pad action", "Back")),
            global: true,
            ..Bind::default()
        },
        || false,
    );
    use_input_action(
        Action::PagePrev,
        Bind {
            label: Some(t!("Previous page")),
            ..Bind::default()
        },
        || false,
    );
    use_input_action(
        Action::PageNext,
        Bind {
            label: Some(t!("Next page")),
            ..Bind::default()
        },
        || false,
    );
    rsx! {}
}

/// Console mode's window: sidebar, top bar, view and hints.
#[component]
pub fn ConsoleShell(view_key: String) -> Element {
    // The cards' "Playing on …" marks, read once.
    crate::game::provide_playing_marks();
    // The pointer hides when it rests (2.5 s after it last moved).
    let mut idle = use_signal(|| true);
    let mut moves = use_signal(|| 0u64);
    let moved = move |_| {
        idle.set(false);
        let mine = *moves.peek() + 1;
        moves.set(mine);
        spawn(async move {
            let _ = caskade_app::runtime::spawn(tokio::time::sleep(CURSOR_IDLE)).await;
            if *moves.peek() == mine {
                idle.set(true);
            }
        });
    };
    rsx! {
        div {
            "data-cs": "console-shell",
            "data-cursor": if idle() { "hidden" } else { "visible" },
            class: "relative flex h-screen flex-col overflow-clip data-[cursor=hidden]:cursor-none",
            onmousemove: moved,
            onmousedown: moved,
            ShellBindings {}
            div { "data-cs": "backdrop", "aria-hidden": "true" }
            div { class: "flex min-h-0 flex-1",
                ConsoleSidebar {}
                div { "data-part": "main", class: "relative flex min-w-0 flex-1 flex-col",
                    ConsoleTopbar {}
                    main {
                        "data-cs": "view",
                        "data-focus-section": "main",
                        class: "min-h-0 flex-1 overflow-y-auto px-8 pb-8",
                        div { key: "{view_key}", class: "flex min-h-full animate-enter flex-col",
                            ErrorBoundary { handle_error: super::error::view_error,
                                super::views::Outlet {}
                            }
                        }
                    }
                }
            }
            HintBar {}
        }
    }
}

/// The desktop sidebar's links as big targets, the Library's filters under
/// its link, plugins' views, and the way back to the desktop.
#[component]
fn ConsoleSidebar() -> Element {
    let links = PRIMARY.iter().chain(SECONDARY.iter());
    rsx! {
        aside {
            "data-cs": "console-sidebar",
            "data-focus-section": "sidebar",
            // Scrolls: on a 1280×800 screen (a Steam Deck) the list with the
            // Library's filters is taller than the window.
            class: "relative z-10 flex w-sidebar shrink-0 flex-col overflow-y-auto border-r border-border bg-surface-raised/80 px-4 pb-6",
            div { "data-part": "brand", class: "flex h-topbar shrink-0 items-center gap-3 px-3",
                super::Brand { class: "size-10" }
                span { class: "font-display text-xl font-semibold tracking-[0.3em]", "CASKADE" }
            }
            nav {
                "data-part": "nav",
                "aria-label": t!(note = "The console-mode sidebar's navigation", "Console"),
                class: "mt-4 flex flex-col gap-1.5",
                for item in links {
                    RouteLink {
                        key: "{item.to}",
                        to: item.to,
                        exact: item.exact,
                        class: ITEM,
                        "data-part": "nav-item",
                        Svg { icon: item.icon, part: "icon", class: "size-6 shrink-0" }
                        span { "data-part": "label", class: "flex-1", {(item.label)()} }
                    }
                    if item.to == "/library" {
                        LibraryNav { console: true }
                    }
                }
            }
            div { class: "mt-4",
                crate::plugins::PluginNav { item_class: ITEM }
            }
            button {
                "data-part": "leave",
                r#type: "button",
                "data-spatial": "focusable",
                class: "mt-auto flex items-center gap-4 rounded-control px-4 py-3 text-fg-muted hover:bg-surface-hover hover:text-fg data-focus-visible:bg-surface-hover data-focus-visible:text-fg",
                onclick: move |_| crate::console::set_mode(crate::console::Mode::Desktop, true),
                Svg { icon: icons::MONITOR, class: "size-6 shrink-0" }
                {t!("Desktop mode")}
            }
        }
    }
}

/// Search, Activity and the account. No window controls: the window is
/// fullscreen.
#[component]
fn ConsoleTopbar() -> Element {
    rsx! {
        header {
            "data-cs": "console-topbar",
            class: "relative z-10 flex h-topbar shrink-0 items-center gap-6 px-8",
            super::search::SearchBox {}
            div { "data-part": "actions", class: "ml-auto flex items-center gap-5",
                super::topbar::ActivityLink {}
                crate::accounts::AccountMenu {}
            }
        }
    }
}

/// The game a running session belongs to, the latest last.
type Running = Vec<(String, String)>;

/// Console mode's curtain over a running game (the webview's
/// `GameCurtain`): when a game starts Caskade fades to black instead of
/// minimizing, and the game's window takes focus over it. It lifts when
/// Caskade has focus again after the game had it, when the last game exits,
/// or on back. While it is down it takes the pad, so nothing behind it is
/// pressed; guide still opens the quick menu over it.
#[component]
pub fn GameCurtain() -> Element {
    let source = use_context::<crate::data::DataSource>();
    let mut running = use_signal(Running::new);
    let mut covered = use_signal(|| None::<String>);
    // The last game stays on the curtain while it fades out.
    let mut shown = use_signal(|| None::<String>);
    let mut left = use_signal(|| false);
    use_hook(move || {
        let mut events = source.events.subscribe();
        spawn(async move {
            loop {
                match events.recv().await {
                    Ok(caskade_app::events::Event::GameStarted(started)) => {
                        running
                            .write()
                            .push((started.0.session_id.clone(), started.0.game_id.clone()));
                        left.set(false);
                        shown.set(Some(started.0.game_id.clone()));
                        covered.set(Some(started.0.game_id.clone()));
                    }
                    Ok(caskade_app::events::Event::GameExited(exited)) => {
                        running
                            .write()
                            .retain(|(session, _)| *session != exited.0.session_id);
                        if running.peek().is_empty() {
                            covered.set(None);
                        }
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        let mut focus = crate::console::focus_changes().subscribe();
        spawn(async move {
            while let Ok(focused) = focus.recv().await {
                if covered.peek().is_none() {
                    continue;
                }
                if !focused {
                    left.set(true);
                } else if *left.peek() {
                    covered.set(None);
                }
            }
        });
    });
    let down = covered().is_some();
    rsx! {
        div {
            "data-cs": "game-curtain",
            "data-state": if down { "covered" } else { "open" },
            "aria-hidden": "true",
            class: "pointer-events-none fixed inset-0 z-50 grid place-items-center bg-black opacity-0 transition-opacity duration-[calc(var(--cs-motion-duration-slow)*2)] data-[state=covered]:opacity-100",
            if let Some(game) = shown() {
                CurtainText { game_id: game }
            }
        }
        if down {
            CurtainGuard { on_lift: move |_| covered.set(None) }
        }
    }
}

/// Takes the pad and the pointer while the curtain is down; back lifts it.
#[component]
fn CurtainGuard(on_lift: EventHandler<()>) -> Element {
    let mut me = use_signal(|| None::<blitz_dom::NodeId>);
    let input = use_input();
    let focus = crate::ui::popover::use_overlay_focus();
    use_input_action(
        Action::Back,
        Bind {
            owner: me(),
            ..Bind::default()
        },
        move || {
            on_lift.call(());
            true
        },
    );
    rsx! {
        div {
            role: "dialog",
            "aria-modal": "true",
            "aria-label": t!(note = "The black curtain over a running game", "Playing"),
            tabindex: "-1",
            "data-spatial": "focusable",
            class: "fixed inset-0 z-50 cursor-none outline-none",
            onmounted: move |event| {
                focus.mounted(&event);
                if let Some(handle) = event.data().downcast::<NodeHandle>() {
                    me.set(Some(handle.node_id()));
                    input.focus(handle.node_id());
                }
            },
        }
    }
}

#[component]
fn CurtainText(game_id: String) -> Element {
    let game = crate::data::use_game(game_id);
    let title = game
        .read()
        .as_ref()
        .and_then(|game| game.as_ref().ok().cloned().flatten())
        .map(|game| game.game.title);
    let family = crate::console::use_family();
    let guide = crate::console::glyph(Action::Guide, family)
        .unwrap_or_else(|| caskade_ui_i18n::tp!("pad button", "Guide"));
    rsx! {
        div { class: "flex flex-col items-center gap-4 text-center",
            p { "data-part": "title", class: "font-display text-4xl font-semibold text-fg",
                {title.unwrap_or_else(|| t!(note = "A game starting, before its title is known", "Starting…"))}
            }
            // The guide button's name comes first.
            p { "data-part": "hint", class: "text-sm text-fg-muted",
                {crate::trans!("<0>{guide}</0> opens the quick menu", guide = guide;
                    0 => |inner| rsx! { kbd { class: "font-mono font-semibold text-fg", {inner} } })}
            }
        }
    }
}
