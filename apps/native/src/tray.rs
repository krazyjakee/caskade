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
use caskade_core::settings::{GeneralSettings, SettingsService};
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
    MenuEvent::set_event_handler(Some(|event: MenuEvent| match event.id.as_ref() {
        SHOW => launch::ask(Request::Show),
        QUIT => {
            state().quit();
            launch::ask(Request::Quit);
        }
        _ => {}
    }));
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
    caskade_app::runtime::spawn(async move {
        let general = |settings: SettingsService| async move {
            settings
                .read_or_default::<GeneralSettings>("core.general")
                .await
        };
        state().follow(&general(settings.clone()).await);
        let mut changes = settings.subscribe();
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match changes.recv().await {
                Ok(changed) if changed.namespace == "core.general" => {}
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
            state().follow(&general(settings.clone()).await);
        }
    });
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
    use std::time::Duration;

    /// What the GTK thread does for the rest of the app.
    pub enum Command {
        Relabel,
        /// Hide the icon; the sender hears once it's hidden.
        Hide(mpsc::Sender<()>),
    }

    pub static COMMANDS: Mutex<Option<mpsc::Sender<Command>>> = Mutex::new(None);

    /// libappindicator-sys panics on first use when neither library loads,
    /// so load it here first, where the panic can be caught.
    fn indicator_available() -> bool {
        std::panic::catch_unwind(|| {
            let _ = &*libappindicator_sys::LIB;
        })
        .is_ok()
    }

    /// Starts the GTK thread and puts the icon up there; whether it's up.
    pub fn start() -> bool {
        let (ready, up) = mpsc::channel();
        let (commands, received) = mpsc::channel::<Command>();
        let started = std::thread::Builder::new()
            .name("caskade-tray".into())
            .spawn(move || {
                if let Err(error) = gtk::init() {
                    tracing::warn!(%error, "no tray icon: GTK didn't start");
                    let _ = ready.send(false);
                    return;
                }
                if !indicator_available() {
                    tracing::warn!(
                        "no tray icon: install libayatana-appindicator3 (or libappindicator3); \
                         closing the window quits"
                    );
                    let _ = ready.send(false);
                    return;
                }
                if let Err(error) = build() {
                    tracing::warn!(%error, "no tray icon; closing the window quits");
                    let _ = ready.send(false);
                    return;
                }
                let _ = ready.send(true);
                gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
                    while let Ok(command) = received.try_recv() {
                        match command {
                            Command::Relabel => relabel(),
                            Command::Hide(done) => {
                                ICON_HERE.with(|here| {
                                    if let Some(icon) = here.borrow().as_ref()
                                        && let Err(error) = icon.icon.set_visible(false)
                                    {
                                        tracing::warn!(%error, "couldn't hide the tray icon");
                                    }
                                });
                                let _ = done.send(());
                            }
                        }
                    }
                    gtk::glib::ControlFlow::Continue
                });
                gtk::main();
            });
        if let Err(error) = started {
            tracing::warn!(%error, "no tray icon: its thread didn't start");
            return false;
        }
        *COMMANDS.lock().unwrap_or_else(|e| e.into_inner()) = Some(commands);
        up.recv_timeout(Duration::from_secs(5)).unwrap_or(false)
    }

    pub fn send(command: Command) -> bool {
        COMMANDS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|commands| commands.send(command).is_ok())
    }
}

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
    linux::send(linux::Command::Relabel);
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
        let (done, hidden) = std::sync::mpsc::channel();
        if linux::send(linux::Command::Hide(done)) {
            let _ = hidden.recv_timeout(std::time::Duration::from_secs(1));
            std::thread::sleep(HIDDEN_BEFORE_EXIT);
        }
    }
}
