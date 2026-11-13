//! `caskade --launch <game id> [--entry <id>] [--action <id>]` (#256):
//! `caskade_app::launch_cli`, shared by both app shells: holding
//! the caller's process (on Windows with a hidden window, so that WM_CLOSE
//! ends it).

pub use caskade_app::launch_cli::*;

/// Runs the caller for `request` and exits with its code. Never returns.
pub fn main(request: Request) -> ! {
    hold(move || caller(request))
}

/// Waits for `work` on POSIX, where a signal ends the process and so the
/// channel.
#[cfg(not(windows))]
fn hold(work: impl FnOnce() -> i32) -> ! {
    std::process::exit(work())
}

/// Waits for `work` with a hidden window, so that WM_CLOSE (what Sunshine
/// sends a program when a stream ends) ends the caller, and so the game.
#[cfg(windows)]
fn hold(work: impl FnOnce() -> i32 + Send + 'static) -> ! {
    use tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopBuilder},
        window::WindowBuilder,
    };
    let event_loop = EventLoopBuilder::<i32>::with_user_event().build();
    let window = match WindowBuilder::new()
        .with_visible(false)
        .with_title("Caskade")
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(error) => {
            tracing::warn!(%error, "no window for WM_CLOSE; waiting without one");
            std::process::exit(work());
        }
    };
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        let code = work();
        // Without the loop, the code is all that is left to give.
        if proxy.send_event(code).is_err() {
            std::process::exit(code);
        }
    });
    event_loop.run(move |event, _, flow| {
        let _ = &window;
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(code) => std::process::exit(code),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => std::process::exit(code::STOPPED),
            _ => {}
        }
    })
}
