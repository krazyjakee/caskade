//! The game window: `caskade --web-game <url>` plays a
//! web game in a window of its own, in a process of its own, before any of
//! the app starts.
//!
//! It is plain tao + wry, the versions Tauri builds, with no IPC handler:
//! the page can't reach the core. Its webview keeps its data (saves,
//! storage) in the folder the launch names, one per game. New windows and
//! pop-ups are refused: a game's links never open anything.
//!
//! A `.swf` plays in Ruffle (`flash.rs`).
//!
//! With `--bundle <folder>` it plays an offline bundle through the proxy
//! (`proxy.rs`), and with `--capture` too it saves the game into one,
//! guided by the capture bar (`bar.rs`, Linux only so far).

#[cfg(target_os = "linux")]
mod bar;
#[cfg(target_os = "linux")]
mod find;
mod flash;
mod portals;
mod proxy;
mod size;

use caskade_core::launch::{
    BUNDLE_ARG, CAPTURE_ARG, DATA_DIR_ARG, FULLSCREEN_ARG, GEOMETRY_ARG, SIZE_ARG, WEB_GAME_ARG,
    WindowPlacement,
};
use caskade_core::web::GameSize;
use std::{ffi::OsString, path::PathBuf, sync::Arc};

/// The capture was saved, or the game closed normally.
pub const EXIT_SAVED: i32 = 0;
/// The window closed before the capture was saved.
pub const EXIT_CANCELLED: i32 = 3;

/// The capture bar's height, for a window sized to its game.
#[cfg(target_os = "linux")]
const BAR_HEIGHT: f64 = bar::HEIGHT as f64;
#[cfg(not(target_os = "linux"))]
const BAR_HEIGHT: f64 = 0.0;

/// How long the offline check plays the bundle before saying how it went.
#[cfg(target_os = "linux")]
const CHECK_SECONDS: u64 = 12;

/// What the launch asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub url: String,
    pub data_dir: Option<PathBuf>,
    pub fullscreen: bool,
    /// The offline bundle to play, or to save into with `capture`.
    pub bundle: Option<PathBuf>,
    pub capture: bool,
    /// Where to open: over Caskade's window, in desktop mode.
    pub geometry: Option<WindowPlacement>,
    /// How big the game is, when its page said; a capture keeps it.
    pub size: Option<GameSize>,
}

impl Options {
    /// The game window's options when the process was started as one
    /// (`WEB_GAME_ARG` first); `None` for the app.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let mut args = args.into_iter().skip(1);
        if args.next()? != WEB_GAME_ARG {
            return None;
        }
        let url = caskade_core::web::page_url(&args.next()?.into_string().ok()?)
            .ok()?
            .into();
        let mut options = Self {
            url,
            data_dir: None,
            fullscreen: false,
            bundle: None,
            capture: false,
            geometry: None,
            size: None,
        };
        while let Some(arg) = args.next() {
            if arg == DATA_DIR_ARG {
                options.data_dir = args.next().map(PathBuf::from);
            } else if arg == FULLSCREEN_ARG {
                options.fullscreen = true;
            } else if arg == BUNDLE_ARG {
                options.bundle = args.next().map(PathBuf::from);
            } else if arg == CAPTURE_ARG {
                options.capture = true;
            } else if arg == GEOMETRY_ARG {
                options.geometry = args
                    .next()
                    .and_then(|text| WindowPlacement::parse(text.to_str()?));
            } else if arg == SIZE_ARG {
                options.size = args.next().and_then(|text| GameSize::parse(text.to_str()?));
            }
        }
        // A capture needs somewhere to save to.
        if options.capture && options.bundle.is_none() {
            return None;
        }
        Some(options)
    }
}

enum UserEvent {
    /// The page's title, for the window's.
    Title(String),
    /// A capture bar button.
    #[cfg(target_os = "linux")]
    Bar(bar::Action),
    /// The offline check has played long enough.
    #[cfg(target_os = "linux")]
    CheckOver,
    /// Time to look for the game in the page being captured.
    #[cfg(target_os = "linux")]
    Look,
    /// What the page shows, `find::FIND`'s answer.
    #[cfg(target_os = "linux")]
    Seen(String),
    /// Stop (SIGTERM), or a webview that can't come back: close as the
    /// window's close button does.
    #[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
    Stop,
    /// The page's renderer failed: load it again (#3).
    Reload,
}

/// Stop's signal, the same number on every Linux architecture.
#[cfg(target_os = "linux")]
const SIGTERM: i32 = 15;

/// Where a capture is. Captures need the capture bar, which is Linux only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum Step {
    Playing,
    Capturing,
    Checking,
    Checked,
}

/// Runs the game window until it closes; returns the process's exit code.
pub fn run(options: Options) -> Result<i32, String> {
    use tao::{
        dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        window::{Fullscreen, WindowBuilder},
    };
    use wry::{NewWindowResponse, WebContext, WebViewBuilder};

    let session = match &options.bundle {
        Some(dir) => {
            if !cfg!(target_os = "linux") {
                return Err("Offline web games aren't available on this system yet.".into());
            }
            let bundle = if options.capture {
                let start = url::Url::parse(&options.url).map_err(|error| error.to_string())?;
                caskade_core::web::bundle::Bundle::create(dir, &start)
            } else {
                caskade_core::web::bundle::Bundle::open(dir)
            }
            .map_err(|error| format!("the game's offline copy can't be opened: {error}"))?;
            if options.capture && options.size.is_some() {
                bundle.set_size(options.size);
            }
            let mode = if options.capture {
                proxy::Mode::Capture
            } else {
                proxy::Mode::Play
            };
            Some(Arc::new(proxy::Session::new(bundle, mode)))
        }
        None => None,
    };
    let start = session
        .as_ref()
        .map_or_else(|| options.url.clone(), |session| session.bundle.start());

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let mut window = WindowBuilder::new()
        .with_title("Caskade")
        .with_min_inner_size(LogicalSize::new(320.0, 240.0))
        .with_fullscreen(options.fullscreen.then_some(Fullscreen::Borderless(None)));
    // At the size the user last gave it, or the game's where it's known,
    // over Caskade's window in desktop mode; else covering Caskade's window,
    // or a size of its own. A capture sizes itself to the game.
    let geometry = options.geometry.filter(|_| !options.fullscreen);
    let remembers = options
        .data_dir
        .clone()
        .filter(|_| !options.capture && !options.fullscreen);
    let game = size::remembered(remembers.as_deref())
        .or_else(|| {
            size::game_size(
                options.size,
                session.as_ref().map(|session| &session.bundle),
                &start,
            )
        })
        .filter(|_| !options.fullscreen);
    let screen = geometry
        .and_then(|at| {
            let centre = PhysicalPosition::new(
                f64::from(at.x) + f64::from(at.width) / 2.0,
                f64::from(at.y) + f64::from(at.height) / 2.0,
            );
            event_loop.monitor_from_point(centre.x, centre.y)
        })
        .or_else(|| event_loop.primary_monitor());
    let placed = game.zip(screen).map(|(game, screen)| {
        let (position, area) = (screen.position(), screen.size());
        let area = WindowPlacement {
            x: position.x,
            y: position.y,
            width: area.width,
            height: area.height,
        };
        let bar = if options.capture && cfg!(target_os = "linux") {
            BAR_HEIGHT
        } else {
            0.0
        };
        size::place(game, bar, screen.scale_factor(), area, geometry)
    });
    window = match (placed, geometry) {
        (Some(at), _) | (None, Some(at)) => window
            .with_position(PhysicalPosition::new(at.x, at.y))
            .with_inner_size(PhysicalSize::new(at.width, at.height)),
        (None, None) => window.with_inner_size(LogicalSize::new(1280.0, 800.0)),
    };
    let window = window
        .build(&event_loop)
        .map_err(|error| error.to_string())?;
    // What the window was asked to be, to tell the user's resizing from it.
    let mut asked = match (placed, geometry) {
        (Some(at), _) | (None, Some(at)) => PhysicalSize::new(at.width, at.height),
        (None, None) => LogicalSize::new(1280.0, 800.0).to_physical(window.scale_factor()),
    };
    let mut context = WebContext::new(options.data_dir.clone());
    let titles = event_loop.create_proxy();
    let mut builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_autoplay(true)
        .with_back_forward_navigation_gestures(false)
        .with_document_title_changed_handler(move |title| {
            let _ = titles.send_event(UserEvent::Title(title));
        })
        .with_new_window_req_handler(|url, _| {
            tracing::debug!(host = url_host(&url), "refused a new window");
            NewWindowResponse::Deny
        });
    // A capture finds the game in its page (`find.rs`).
    #[cfg(target_os = "linux")]
    if options.capture {
        builder = builder.with_initialization_script_for_main_only(find::FRAMES_SCRIPT, false);
    }
    // An offline copy plays without its portal's page.
    if session.is_some() && !options.capture {
        builder = builder.with_initialization_script_for_main_only(portals::SCRIPT, false);
    }
    if let Some(session) = &session {
        let port = proxy::start(Arc::clone(session))?;
        builder = builder.with_proxy_config(wry::ProxyConfig::Http(wry::ProxyEndpoint {
            host: "127.0.0.1".into(),
            port: port.to_string(),
        }));
    }
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut step = match &session {
        Some(_) if options.capture => Step::Capturing,
        _ => Step::Playing,
    };
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    let (webview, bar) = {
        use tao::platform::unix::WindowExtUnix;
        use wry::{WebViewBuilderExtUnix, WebViewExtUnix};
        let vbox = window
            .default_vbox()
            .ok_or("the game window has no GTK container")?;
        #[cfg(target_os = "linux")]
        let bar = (step == Step::Capturing).then(|| {
            let actions = event_loop.create_proxy();
            bar::Bar::new(vbox, move |action| {
                let _ = actions.send_event(UserEvent::Bar(action));
            })
        });
        let webview = builder.build_gtk(vbox).map_err(|error| error.to_string())?;
        if session.is_some() {
            use webkit2gtk::{WebContextExt, WebViewExt, WebsiteDataManagerExt};
            let view = webview.webview();
            // Safe only because every request goes through the proxy, which
            // verifies upstream certificates itself.
            if let Some(data) = view.website_data_manager() {
                data.set_tls_errors_policy(webkit2gtk::TLSErrorsPolicy::Ignore);
            }
            // WebKit's own cache would hide what a bundle lacks.
            if let Some(context) = view.context() {
                context.set_cache_model(webkit2gtk::CacheModel::DocumentViewer);
            }
        }
        #[cfg(target_os = "linux")]
        if let (Some(bar), Some(session)) = (&bar, &session) {
            let (bar, session) = (bar.clone(), Arc::clone(session));
            gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                let (files, bytes) = session.bundle.stats();
                bar.stats(files, bytes);
                gtk::glib::ControlFlow::Continue
            });
        }
        #[cfg(not(target_os = "linux"))]
        let bar = ();
        (webview, bar)
    };
    #[cfg(not(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    )))]
    let (webview, bar) = (
        builder.build(&window).map_err(|error| error.to_string())?,
        (),
    );
    load(&webview, &start).map_err(|error| error.to_string())?;
    // A webview that lost its processes would stay white (#3).
    {
        use crate::recovery::{Policy, Step as Recover, log};
        let fails = event_loop.create_proxy();
        let policy = std::cell::RefCell::new(Policy::default());
        let watched = crate::recovery::watch_player(&webview, move |failed| {
            log("game window", &failed);
            let event = match policy
                .borrow_mut()
                .next(failed.kind, std::time::Instant::now())
            {
                Recover::Wait => return,
                Recover::Reload => UserEvent::Reload,
                Recover::Restart => UserEvent::Stop,
            };
            let _ = fails.send_event(event);
        });
        if let Err(error) = watched {
            tracing::warn!(%error, "can't watch the game window's webview");
        }
    }
    tracing::info!(
        host = url_host(&start),
        bundle = session.is_some(),
        capture = options.capture,
        "game window open"
    );
    #[cfg(target_os = "linux")]
    let checks = event_loop.create_proxy();
    #[cfg(target_os = "linux")]
    let looks = event_loop.create_proxy();
    #[cfg(target_os = "linux")]
    if step == Step::Capturing {
        let looks = looks.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            match looks.send_event(UserEvent::Look) {
                Ok(()) => gtk::glib::ControlFlow::Continue,
                Err(_) => gtk::glib::ControlFlow::Break,
            }
        });
    }
    #[cfg(target_os = "linux")]
    {
        let stops = event_loop.create_proxy();
        gtk::glib::unix_signal_add_local_once(SIGTERM, move || {
            let _ = stops.send_event(UserEvent::Stop);
        });
    }
    // The capture's page; the game found in it becomes the copy's start.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    let page = start.clone();
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut, unused_variables))]
    let mut start = start;
    #[cfg(target_os = "linux")]
    let mut found: Option<find::Found> = None;

    // The window manager's title bar and borders come once it has framed
    // the window; then they're taken off the size, once, so the whole
    // window covers Caskade's.
    let mut fit = geometry.filter(|_| placed.is_none());
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Some(at) = fit {
            let (outer, inner) = (window.outer_size(), window.inner_size());
            let extra = (
                outer.width.saturating_sub(inner.width),
                outer.height.saturating_sub(inner.height),
            );
            if extra != (0, 0) {
                fit = None;
                asked = PhysicalSize::new(
                    at.width.saturating_sub(extra.0).max(1),
                    at.height.saturating_sub(extra.1).max(1),
                );
                window.set_inner_size(asked);
            }
        }
        // Owned by the loop, so the page and the bar live as long as the window.
        let _ = (&webview, &bar);
        let save = || {
            if let Some(session) = &session
                && let Err(error) = session.bundle.save()
            {
                tracing::error!(%error, "the game's offline copy couldn't be saved");
            }
        };
        match event {
            Event::UserEvent(UserEvent::Title(title)) => {
                let title = title.trim();
                window.set_title(if title.is_empty() { "Caskade" } else { title });
            }
            Event::UserEvent(UserEvent::Reload) => {
                tracing::warn!("reloading the game window's page");
                if let Err(error) = webview.reload() {
                    tracing::error!(%error, "the game window's page couldn't reload");
                    *control_flow = ControlFlow::ExitWithCode(EXIT_SAVED);
                }
            }
            #[cfg(target_os = "linux")]
            Event::UserEvent(UserEvent::Bar(action)) => {
                let (Some(bar), Some(session)) = (&bar, &session) else {
                    return;
                };
                match (step, action) {
                    (Step::Capturing, bar::Action::Primary) => {
                        // Only the game, where the page showed one: a
                        // Flash movie, or a frame from another site. A
                        // site's own frame keeps its page: all of it is the
                        // game's origin anyway, and some play only inside
                        // it (totaljerkface.com's).
                        if let Some(game) = &found
                            && (caskade_core::web::is_swf(&game.url)
                                || !same_origin(&game.url, &page))
                            && session.bundle.get(&game.url).is_some()
                        {
                            session.bundle.set_start(&game.url);
                            if game.size.is_some() {
                                session.bundle.set_size(game.size);
                            }
                            match session.bundle.retain(session.needed_by(&game.url)) {
                                Ok(dropped) => tracing::info!(
                                    host = game.url.host_str(),
                                    dropped,
                                    "kept only the game found in its page"
                                ),
                                Err(error) => {
                                    tracing::warn!(%error, "the page's files couldn't be dropped");
                                }
                            }
                            start = session.bundle.start();
                        }
                        save();
                        step = Step::Checking;
                        session.set_mode(proxy::Mode::Check);
                        bar.checking();
                        clear_cache(&webview);
                        if let Err(error) = load(&webview, &start) {
                            tracing::warn!(%error, "the game couldn't load to be checked");
                        }
                        let checks = checks.clone();
                        gtk::glib::timeout_add_local_once(
                            std::time::Duration::from_secs(CHECK_SECONDS),
                            move || {
                                let _ = checks.send_event(UserEvent::CheckOver);
                            },
                        );
                    }
                    (Step::Checked, bar::Action::PlayMore) => {
                        step = Step::Capturing;
                        session.set_mode(proxy::Mode::Capture);
                        bar.capturing();
                        // In its page again: some games play only there.
                        if let Err(error) = load(&webview, &page) {
                            tracing::warn!(%error, "the game's page couldn't load");
                        }
                    }
                    (Step::Checked, bar::Action::Primary | bar::Action::SaveAnyway) => {
                        save();
                        *control_flow = ControlFlow::ExitWithCode(EXIT_SAVED);
                    }
                    _ => {}
                }
            }
            #[cfg(target_os = "linux")]
            Event::UserEvent(UserEvent::Look) => {
                if step == Step::Capturing {
                    let looks = looks.clone();
                    let _ = webview.evaluate_script_with_callback(find::FIND, move |answer| {
                        let _ = looks.send_event(UserEvent::Seen(answer));
                    });
                }
            }
            #[cfg(target_os = "linux")]
            Event::UserEvent(UserEvent::Seen(answer)) => {
                // The last game seen stays while its frame reloads.
                if step == Step::Capturing
                    && let Some(game) = find::choose(&answer)
                    && found.as_ref() != Some(&game)
                {
                    tracing::info!(host = game.url.host_str(), "found the game in its page");
                    if let Some(bar) = &bar {
                        bar.found();
                    }
                    found = Some(game);
                }
            }
            #[cfg(target_os = "linux")]
            Event::UserEvent(UserEvent::CheckOver) => {
                if step != Step::Checking {
                    return;
                }
                let (Some(bar), Some(session)) = (&bar, &session) else {
                    return;
                };
                step = Step::Checked;
                let missing = session.missing_count();
                tracing::info!(missing, "offline check done");
                if missing == 0 {
                    bar.ready();
                } else {
                    bar.missing();
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(UserEvent::Stop) => {
                #[cfg(target_os = "linux")]
                end_page(&webview);
                // The size the user gave the window, for next time; not a
                // maximized or fullscreen one's.
                if let Some(dir) = &remembers
                    && !window.is_maximized()
                    && window.fullscreen().is_none()
                {
                    let now = window.inner_size();
                    if let Some(size) = size::resized(
                        (asked.width, asked.height),
                        (now.width, now.height),
                        window.scale_factor(),
                    ) && let Err(error) = size::remember(dir, size)
                    {
                        tracing::warn!(%error, "the game window's size couldn't be kept");
                    }
                }
                // Closing before the check keeps nothing; after it, it saves.
                *control_flow = match step {
                    Step::Capturing | Step::Checking => ControlFlow::ExitWithCode(EXIT_CANCELLED),
                    Step::Checked | Step::Playing => {
                        save();
                        ControlFlow::ExitWithCode(EXIT_SAVED)
                    }
                };
            }
            _ => {}
        }
    })
}

/// Whether `url` comes from the same site as the `page` it was found in.
#[cfg(target_os = "linux")]
fn same_origin(url: &url::Url, page: &str) -> bool {
    url::Url::parse(page).is_ok_and(|page| page.origin() == url.origin())
}

/// Opens the game at `start`: its page, or Ruffle's for a Flash movie.
fn load(webview: &wry::WebView, start: &str) -> wry::Result<()> {
    let Some(movie) = url::Url::parse(start)
        .ok()
        .filter(caskade_core::web::is_swf)
    else {
        return webview.load_url(start);
    };
    let page = flash::page(movie.as_str());
    // At the movie's address where the webview allows it.
    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::WebViewExt;
        use wry::WebViewExtUnix;
        webview.webview().load_html(&page, Some(movie.as_str()));
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    webview.load_html(&page)
}

/// Ends the page's web process with the window. Left to notice the window
/// is gone, a web process busy with the game runs on, sound and all, for
/// up to 10 s. Saves are safe: the network process keeps the page's storage.
#[cfg(target_os = "linux")]
fn end_page(webview: &wry::WebView) {
    use webkit2gtk::WebViewExt;
    use wry::WebViewExtUnix;
    webview.webview().terminate_web_process();
}

/// Drops WebKit's cached responses, so the next load asks the proxy.
#[cfg(target_os = "linux")]
fn clear_cache(webview: &wry::WebView) {
    use webkit2gtk::{WebContextExt, WebViewExt};
    use wry::WebViewExtUnix;
    if let Some(context) = webview.webview().context() {
        context.clear_cache();
    }
}

/// The host alone, for logs: URLs can carry tokens.
fn url_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{GameSize, Options};

    fn args(list: &[&str]) -> Vec<std::ffi::OsString> {
        list.iter().map(Into::into).collect()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_frame_from_the_pages_own_site_is_the_same_origin() {
        let url = |text: &str| url::Url::parse(text).unwrap();
        let page = "https://totaljerkface.com/happy_wheels.tjf?level_id=1";
        assert!(super::same_origin(
            &url("https://totaljerkface.com/happy-wheels-js/index.tjf"),
            page
        ));
        assert!(!super::same_origin(
            &url("https://html-classic.itch.zone/html/1/index.html"),
            "https://maddy.itch.io/celeste"
        ));
        assert!(!super::same_origin(
            &url("https://cache.armorgames.com/files/games/x/index.html"),
            "https://armorgames.com/play/1/x"
        ));
    }

    #[test]
    fn only_a_web_game_launch_starts_the_game_window() {
        assert_eq!(
            Options::from_args(args(&[
                "caskade",
                "--web-game",
                "https://example.com/game#x",
                "--data-dir",
                "/data/web/e1",
                "--fullscreen",
            ])),
            Some(Options {
                url: "https://example.com/game".into(),
                data_dir: Some("/data/web/e1".into()),
                fullscreen: true,
                bundle: None,
                capture: false,
                geometry: None,
                size: None,
            })
        );
        assert_eq!(
            Options::from_args(args(&["caskade", "--web-game", "https://example.com/"]))
                .map(|options| (options.data_dir, options.fullscreen)),
            Some((None, false))
        );
        assert_eq!(Options::from_args(args(&["caskade"])), None);
        assert_eq!(Options::from_args(args(&["caskade", "--console"])), None);
        assert_eq!(
            Options::from_args(args(&["caskade", "--web-game", "file:///etc/passwd"])),
            None
        );
    }

    #[test]
    fn a_capture_saves_into_a_bundle() {
        let capture = Options::from_args(args(&[
            "caskade",
            "--web-game",
            "https://html-classic.itch.zone/html/1/index.html",
            "--bundle",
            "/games/needle",
            "--capture",
            "--size",
            "640x360",
        ]))
        .unwrap();
        assert_eq!(capture.bundle, Some("/games/needle".into()));
        assert_eq!(capture.size, GameSize::new(640, 360));
        let placed = Options::from_args(args(&[
            "caskade",
            "--web-game",
            "https://a.example/",
            "--geometry",
            "40,30,1600,900",
        ]))
        .unwrap();
        assert_eq!(
            placed.geometry.map(|at| (at.x, at.y, at.width, at.height)),
            Some((40, 30, 1600, 900))
        );
        assert!(capture.capture);
        assert_eq!(
            Options::from_args(args(&[
                "caskade",
                "--web-game",
                "https://a.example/",
                "--capture"
            ])),
            None,
            "nowhere to save to"
        );
    }
}
