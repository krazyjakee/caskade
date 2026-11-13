//! The tray icon (#88): its menu shows the main window or quits, a left
//! click shows it (except on macOS, where a click opens the menu), and the
//! close button hides the window to it when `core.general.onClose` is
//! `tray`. Its state is `caskade_app::tray`'s, as in the Tauri app.
//!
//! - Linux: an AppIndicator, loaded at run time from libayatana-appindicator3
//!   or libappindicator3, on a GTK main loop of its own thread (winit
//!   doesn't run GTK). Without either library there's no icon and closing
//!   the window quits. Before Caskade exits the icon is hidden while that
//!   loop still answers the bus ([`hide_before_exit`]): xapp's watcher
//!   (Cinnamon) crashes when an icon's owner leaves before answering.
//! - Windows and macOS: on the main thread, with the window's event loop.

use std::cell::RefCell;
use std::sync::LazyLock;

use caskade_app::tray::{Tray, quit_label, show_label};
use caskade_core::settings::SettingsService;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::launch::{self, Request};

const SHOW: &str = "tray.show";
const QUIT: &str = "tray.quit";

/// How long Caskade keeps answering the bus after hiding its icon, before
/// it exits: the watcher asks for the icon's properties within ~10 ms.
#[cfg(target_os = "linux")]
const HIDDEN_BEFORE_EXIT: std::time::Duration = std::time::Duration::from_millis(500);

#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../../desktop/src-tauri/icons/tray-template.png");
#[cfg(not(target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../../desktop/src-tauri/icons/tray.png");

static STATE: LazyLock<Tray> = LazyLock::new(Tray::default);

/// The tray's state: whether the icon is up and what a close does.
pub fn state() -> &'static Tray {
    &STATE
}

/// The icon and its menu items, on the thread that made them.
struct Icon {
    // Held so the icon stays up; Linux also hides it before exit.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    icon: TrayIcon,
    show: MenuItem,
    quit: MenuItem,
}

thread_local! {
    static ICON_HERE: RefCell<Option<Icon>> = const { RefCell::new(None) };
}

fn image() -> Result<tray_icon::Icon, String> {
    let image = image::load_from_memory_with_format(ICON, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?
        .into_rgba8();
    let (width, height) = image.dimensions();
    tray_icon::Icon::from_rgba(image.into_raw(), width, height).map_err(|error| error.to_string())
}

/// Runs that put no icon up (#122): a data probe
/// (`CASKADE_DATA_PROBE`), a motion measurement
/// (`CASKADE_NATIVE_SMOKE_MOTION`), or `CASKADE_NATIVE_NO_TRAY=1`.
/// Screenshots (`--shots`) never get this far.
const NO_TRAY: [&str; 3] = [
    "CASKADE_NATIVE_NO_TRAY",
    "CASKADE_DATA_PROBE",
    "CASKADE_NATIVE_SMOKE_MOTION",
];

/// Whether this run puts a tray icon up.
fn wanted() -> bool {
    let quiet = NO_TRAY
        .iter()
        .find(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    if let Some(name) = quiet {
        tracing::info!(
            because = name,
            "no tray icon in this run; closing the window quits"
        );
    }
    quiet.is_none()
}

/// Puts the icon up on this thread, and takes its clicks and menu.
fn build() -> Result<(), String> {
    let macos = cfg!(target_os = "macos");
    let show = MenuItem::with_id(SHOW, show_label(), true, None);
    let quit = MenuItem::with_id(QUIT, quit_label(), true, None);
    let menu = Menu::with_items(&[&show, &PredefinedMenuItem::separator(), &quit])
        .map_err(|error| error.to_string())?;
    let icon = TrayIconBuilder::new()
        .with_id("main")
        .with_icon(image()?)
        .with_icon_as_template(macos)
        .with_tooltip("Caskade")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(macos)
        .build()
        .map_err(|error| error.to_string())?;
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
            && !macos
        {
            launch::ask(Request::Show);
        }
    }));
    ICON_HERE.with(|here| *here.borrow_mut() = Some(Icon { icon, show, quit }));
    state().set_shown(true);
    Ok(())
}

/// Takes the menus' clicks (the tray's and, on macOS, the menu bar's):
/// once, before either menu is up.
pub fn handle_menu_events() {
    MenuEvent::set_event_handler(Some(|event: MenuEvent| match event.id.as_ref() {
        SHOW => launch::ask(Request::Show),
        QUIT => {
            state().quit();
            launch::ask(Request::Quit);
        }
        _ => {}
    }));
}

/// macOS: the menu bar's Caskade, Edit and Window menus. Quit is Caskade's
/// own (⌘Q), so it goes through the same quit as the tray's.
#[cfg(target_os = "macos")]
pub fn app_menu() {
    use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
    use tray_icon::menu::{AboutMetadata, Submenu};
    let quit = MenuItem::with_id(
        QUIT,
        quit_label(),
        true,
        Some(Accelerator::new(Modifiers::META, Code::KeyQ)),
    );
    let separator = PredefinedMenuItem::separator;
    let built = (|| -> Result<(), tray_icon::menu::Error> {
        let about = PredefinedMenuItem::about(
            None,
            Some(AboutMetadata {
                name: Some("Caskade".into()),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                ..Default::default()
            }),
        );
        let caskade = Submenu::with_items(
            "Caskade",
            true,
            &[
                &about,
                &separator(),
                &PredefinedMenuItem::services(None),
                &separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &separator(),
                &quit,
            ],
        )?;
        let edit = Submenu::with_items(
            caskade_core::tr!("Edit"),
            true,
            &[
                &PredefinedMenuItem::undo(None),
                &PredefinedMenuItem::redo(None),
                &separator(),
                &PredefinedMenuItem::cut(None),
                &PredefinedMenuItem::copy(None),
                &PredefinedMenuItem::paste(None),
                &PredefinedMenuItem::select_all(None),
            ],
        )?;
        let window = Submenu::with_items(
            caskade_core::tr!("Window"),
            true,
            &[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::fullscreen(None),
            ],
        )?;
        let menu = Menu::with_items(&[&caskade, &edit, &window])?;
        menu.init_for_nsapp();
        // The menu bar keeps it for the app's lifetime.
        std::mem::forget(menu);
        Ok(())
    })();
    if let Err(error) = built {
        tracing::warn!(%error, "no menu bar menus");
    }
}

/// Relabels the menu in the core's language, on the icon's thread.
pub fn relabel() {
    ICON_HERE.with(|here| {
        if let Some(icon) = here.borrow().as_ref() {
            icon.show.set_text(show_label());
            icon.quit.set_text(quit_label());
        }
    });
}

/// Follows `core.general`'s close setting, and the core's language for the
/// menu.
pub fn follow(settings: SettingsService) {
    caskade_app::runtime::spawn(caskade_app::tray::follow_settings(settings, |general| {
        state().follow(general)
    }));
    caskade_app::runtime::spawn(async move {
        let mut changes = caskade_core::i18n::subscribe();
        while changes.changed().await.is_ok() {
            relabel_everywhere();
        }
    });
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::sync::{Mutex, mpsc};

    /// Whether GTK runs on its thread.
    static GTK: Mutex<Option<bool>> = Mutex::new(None);

    /// libappindicator-sys panics on first use when neither library loads,
    /// so load it here first, where the panic can be caught.
    fn indicator_available() -> bool {
        std::panic::catch_unwind(|| {
            let _ = &*libappindicator_sys::LIB;
        })
        .is_ok()
    }

    /// Starts GTK on a thread of its own (once); whether it runs. Never in
    /// unit tests: GTK would open its display and session bus, which are the
    /// developer's desktop when the tests run there (#122). Without it, the
    /// tray, dialogs and GTK settings act as with no GTK.
    pub fn start_gtk() -> bool {
        if cfg!(test) {
            return false;
        }
        let mut gtk = caskade_core::lock(&GTK);
        if let Some(running) = *gtk {
            return running;
        }
        let (ready, started) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("caskade-gtk".into())
            .spawn(move || {
                if let Err(error) = gtk::init() {
                    tracing::warn!(%error, "GTK didn't start: no tray icon or dialogs");
                    let _ = ready.send(false);
                    return;
                }
                let _ = ready.send(true);
                gtk::main();
            });
        let running = spawned.is_ok()
            && started
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap_or(false);
        *gtk = Some(running);
        running
    }

    /// Runs `work` on the GTK thread and waits for its answer; `None` when
    /// GTK doesn't run.
    pub fn on_gtk<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        if !start_gtk() {
            return None;
        }
        let (done, answer) = mpsc::channel();
        gtk::glib::MainContext::default().invoke(move || {
            let _ = done.send(work());
        });
        answer.recv().ok()
    }

    /// Puts the icon up on the GTK thread; whether it's up.
    pub fn start() -> bool {
        if !wanted() {
            return false;
        }
        on_gtk(|| {
            if !indicator_available() {
                tracing::warn!(
                    "no tray icon: install libayatana-appindicator3 (or libappindicator3); \
                     closing the window quits"
                );
                return false;
            }
            match build() {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, "no tray icon; closing the window quits");
                    false
                }
            }
        })
        .unwrap_or(false)
    }

    /// Hides the icon on the GTK thread, while its loop answers the bus.
    pub fn hide() {
        on_gtk(|| {
            ICON_HERE.with(|here| {
                if let Some(icon) = here.borrow().as_ref()
                    && let Err(error) = icon.icon.set_visible(false)
                {
                    tracing::warn!(%error, "couldn't hide the tray icon");
                }
            });
        });
    }
}

/// Runs `work` on the GTK thread (Linux), for the dialogs.
#[cfg(target_os = "linux")]
pub use linux::on_gtk;

/// Puts the icon up where this OS wants it: on Linux, on the tray's own
/// GTK thread (call this before the window opens); elsewhere call
/// [`start_here`] on the main thread once the event loop runs.
#[cfg(target_os = "linux")]
pub fn start() -> bool {
    linux::start()
}

/// Puts the icon up on this thread, the main one, with the event loop
/// running (Windows and macOS).
#[cfg(not(target_os = "linux"))]
pub fn start_here() -> bool {
    if !wanted() {
        return false;
    }
    match build() {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "no tray icon; closing the window quits");
            false
        }
    }
}

fn relabel_everywhere() {
    #[cfg(target_os = "linux")]
    caskade_app::runtime::spawn_blocking(|| linux::on_gtk(relabel));
    #[cfg(not(target_os = "linux"))]
    launch::ask(Request::Relabel);
}

/// Linux: hides the icon while its loop still answers the bus, then waits
/// [`HIDDEN_BEFORE_EXIT`] for the watcher's request for its properties to be
/// answered. Call it just before the process exits; elsewhere it returns at
/// once.
pub fn hide_before_exit() {
    #[cfg(target_os = "linux")]
    {
        if !state().take_shown() {
            return;
        }
        linux::hide();
        std::thread::sleep(HIDDEN_BEFORE_EXIT);
    }
}
