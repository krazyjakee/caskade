//! The video window: `caskade --video <YouTube id>` plays a game's trailer
//! in a window of its own, in a process of its own, before any of the app
//! starts (`commands/video.rs` opens it).
//!
//! YouTube's player refuses to play without a web page's address as the
//! referrer (error 153), and the app's pages, at `tauri://localhost`, have
//! none: so the player is the window's page, loaded with the app's own
//! address (its identifier) as the `Referer`. Like the game window, it is
//! plain tao + wry and the page can't reach the core: its one message,
//! Escape's `close`, only closes the window. It leaves the YouTube player
//! for nothing: links and new windows are refused.
//!
//! A pad's B (or Guide) closes it and A pauses and plays while it has
//! focus. It closes with the app too: the app holds its stdin, which ends
//! when the app does.

use caskade_core::launch::{FULLSCREEN_ARG, GEOMETRY_ARG, WindowPlacement};
use std::ffi::OsString;

/// The video-window argument `run` dispatches on, before the app starts.
pub const VIDEO_ARG: &str = "--video";
/// The window's title: the game's, and the video's name.
pub const TITLE_ARG: &str = "--title";

/// The player's own page; the id goes on the end.
const EMBED: &str = "https://www.youtube-nocookie.com/embed/";
/// Who is asking for the player: YouTube wants an `https` address that
/// names the app. The app's identifier (`tauri.conf.json`).
const REFERRER: &str = "https://dev.caskade.app/";

/// Escape closes the window, except while YouTube's player is fullscreen
/// (Escape leaves that); `ipc` is wry's.
const ESCAPE_SCRIPT: &str = r#"addEventListener("keydown", (event) => {
  if (event.key === "Escape" && !document.fullscreenElement) window.ipc.postMessage("close");
}, true);"#;

/// Pauses or plays the video.
const TOGGLE_SCRIPT: &str =
    r#"(v => v && (v.paused ? v.play() : v.pause()))(document.querySelector("video"))"#;

/// What the app asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub video_id: String,
    pub title: Option<String>,
    pub fullscreen: bool,
    /// Caskade's window, to open over in desktop mode.
    pub geometry: Option<WindowPlacement>,
}

impl Options {
    /// The video window's options when the process was started as one
    /// (`VIDEO_ARG` first); `None` for the app, or a video id that isn't
    /// YouTube's.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Option<Self> {
        let mut args = args.into_iter().skip(1);
        if args.next()? != VIDEO_ARG {
            return None;
        }
        let video_id = args.next()?.into_string().ok()?;
        if !is_video_id(&video_id) {
            return None;
        }
        let mut options = Self {
            video_id,
            title: None,
            fullscreen: false,
            geometry: None,
        };
        while let Some(arg) = args.next() {
            if arg == TITLE_ARG {
                options.title = args.next().and_then(|text| text.into_string().ok());
            } else if arg == FULLSCREEN_ARG {
                options.fullscreen = true;
            } else if arg == GEOMETRY_ARG {
                options.geometry = args
                    .next()
                    .and_then(|text| WindowPlacement::parse(text.to_str()?));
            }
        }
        Some(options)
    }
}

/// A YouTube video id: 11 letters, digits, `-` or `_`.
pub fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The player's address for `video_id`, playing at once.
fn embed_url(video_id: &str) -> String {
    format!("{EMBED}{video_id}?autoplay=1&rel=0&playsinline=1")
}

/// A 16:9 window over the middle of Caskade's, most of its size.
fn centred(over: WindowPlacement) -> WindowPlacement {
    let (width, height) = (f64::from(over.width) * 0.9, f64::from(over.height) * 0.9);
    let (width, height) = if width * 9.0 > height * 16.0 {
        (height * 16.0 / 9.0, height)
    } else {
        (width, width * 9.0 / 16.0)
    };
    let (width, height) = (width.round().max(1.0), height.round().max(1.0));
    WindowPlacement {
        x: over.x + ((f64::from(over.width) - width) / 2.0).round() as i32,
        y: over.y + ((f64::from(over.height) - height) / 2.0).round() as i32,
        width: width as u32,
        height: height as u32,
    }
}

enum UserEvent {
    /// Escape in the page, a pad's B or Guide, or the app went away.
    Close,
    /// A pad's A.
    Toggle,
}

/// Runs the video window until it closes; returns the process's exit code.
pub fn run(options: Options) -> Result<i32, String> {
    use caskade_input::{Action, InputEvent, InputService};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tao::{
        dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        window::{Fullscreen, WindowBuilder},
    };
    use wry::{
        NewWindowResponse, WebViewBuilder,
        http::{HeaderMap, HeaderValue, header::REFERER},
    };

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let mut window = WindowBuilder::new()
        .with_title(options.title.as_deref().unwrap_or("Caskade"))
        .with_min_inner_size(LogicalSize::new(320.0, 180.0))
        .with_fullscreen(options.fullscreen.then_some(Fullscreen::Borderless(None)));
    window = match options.geometry.filter(|_| !options.fullscreen) {
        Some(over) => {
            let at = centred(over);
            window
                .with_position(PhysicalPosition::new(at.x, at.y))
                .with_inner_size(PhysicalSize::new(at.width, at.height))
        }
        None => window.with_inner_size(LogicalSize::new(1280.0, 720.0)),
    };
    let window = window
        .build(&event_loop)
        .map_err(|error| error.to_string())?;

    let mut headers = HeaderMap::new();
    headers.insert(REFERER, HeaderValue::from_static(REFERRER));
    let closes = event_loop.create_proxy();
    let builder = WebViewBuilder::new()
        .with_url_and_headers(embed_url(&options.video_id), headers)
        .with_autoplay(true)
        .with_background_color((0, 0, 0, 255))
        .with_back_forward_navigation_gestures(false)
        .with_initialization_script_for_main_only(ESCAPE_SCRIPT, true)
        .with_ipc_handler(move |request| {
            if request.body() == "close" {
                let _ = closes.send_event(UserEvent::Close);
            }
        })
        // The player's own page only: its links lead to YouTube.
        .with_navigation_handler(|url| url.starts_with(EMBED) || url == "about:blank")
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny);
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window
            .default_vbox()
            .ok_or("the video window has no GTK container")?;
        builder.build_gtk(vbox).map_err(|error| error.to_string())?
    };
    #[cfg(not(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    )))]
    let webview = builder.build(&window).map_err(|error| error.to_string())?;
    // A webview that lost its processes would stay black: close (#3).
    {
        use crate::webview_recovery::{Policy, Step, failure_of, log};
        let fails = event_loop.create_proxy();
        let policy = std::cell::RefCell::new(Policy::default());
        let watched = crate::webview_recovery::watch_player(&webview, move |failed| {
            log("video window", &failed);
            let step = policy
                .borrow_mut()
                .next(failure_of(failed.kind), std::time::Instant::now());
            if step != Step::Wait {
                let _ = fails.send_event(UserEvent::Close);
            }
        });
        if let Err(error) = watched {
            tracing::warn!(%error, "can't watch the video window's webview");
        }
    }
    tracing::info!(video = %options.video_id, "video window open");

    // The app holds the other end of stdin: when it ends, so does the app.
    let orphaned = event_loop.create_proxy();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        let _ = orphaned.send_event(UserEvent::Close);
    });

    // A pad, while the window has focus (Guide always: it brings Caskade back).
    let focused = Arc::new(AtomicBool::new(true));
    let pads = event_loop.create_proxy();
    let _input = InputService::start({
        let focused = Arc::clone(&focused);
        move |event| {
            let InputEvent::Action(input) = event else {
                return;
            };
            let event = match input.action {
                Action::Guide => UserEvent::Close,
                Action::Back if focused.load(Ordering::Relaxed) => UserEvent::Close,
                Action::Confirm if focused.load(Ordering::Relaxed) && !input.repeat => {
                    UserEvent::Toggle
                }
                _ => return,
            };
            let _ = pads.send_event(event);
        }
    });

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // Owned by the loop, so the page lives as long as the window.
        let _ = &webview;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(UserEvent::Close) => {
                window.set_visible(false);
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(now),
                ..
            } => focused.store(now, Ordering::Relaxed),
            Event::UserEvent(UserEvent::Toggle) => {
                if let Err(error) = webview.evaluate_script(TOGGLE_SCRIPT) {
                    tracing::warn!(%error, "couldn't pause or play the video");
                }
            }
            _ => {}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{Options, centred, is_video_id};
    use caskade_core::launch::WindowPlacement;

    fn args(list: &[&str]) -> Vec<std::ffi::OsString> {
        list.iter().map(Into::into).collect()
    }

    #[test]
    fn reads_its_arguments() {
        let options = Options::from_args(args(&[
            "caskade",
            "--video",
            "c0i88t0Kacs",
            "--title",
            "The Witcher 3 · Trailer",
            "--geometry",
            "10,20,1600,900",
        ]))
        .unwrap();
        assert_eq!(options.video_id, "c0i88t0Kacs");
        assert_eq!(options.title.as_deref(), Some("The Witcher 3 · Trailer"));
        assert!(!options.fullscreen);
        assert_eq!(
            options.geometry,
            Some(WindowPlacement {
                x: 10,
                y: 20,
                width: 1600,
                height: 900
            })
        );
        let fullscreen =
            Options::from_args(args(&["caskade", "--video", "c0i88t0Kacs", "--fullscreen"]));
        assert!(fullscreen.unwrap().fullscreen);
    }

    #[test]
    fn only_a_youtube_id_opens_one() {
        assert!(Options::from_args(args(&["caskade"])).is_none());
        assert!(Options::from_args(args(&["caskade", "--web-game", "c0i88t0Kacs"])).is_none());
        assert!(Options::from_args(args(&["caskade", "--video"])).is_none());
        for id in ["c0i88t0Kac", "c0i88t0Kacs1", "c0i88t0K/cs", "https://x", ""] {
            assert!(!is_video_id(id), "{id}");
            assert!(Options::from_args(args(&["caskade", "--video", id])).is_none());
        }
        assert!(is_video_id("yowv6_rspoM") && is_video_id("-b-_A9zZ0aa"));
    }

    #[test]
    fn opens_16_by_9_over_the_middle_of_caskade() {
        let wide = centred(WindowPlacement {
            x: 100,
            y: 50,
            width: 2000,
            height: 1000,
        });
        assert_eq!((wide.width, wide.height), (1600, 900));
        assert_eq!((wide.x, wide.y), (300, 100));
        let tall = centred(WindowPlacement {
            x: 0,
            y: 0,
            width: 1000,
            height: 1000,
        });
        assert_eq!((tall.width, tall.height), (900, 506));
        assert_eq!((tall.x, tall.y), (50, 247));
    }
}
