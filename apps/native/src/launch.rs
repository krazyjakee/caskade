//! Runs the main window's event loop (#88): Dioxus Native's application,
//! wrapped so the shell sees each window event first. `dioxus_native::
//! launch_cfg` closes the app on any close request; here a close can hide
//! the window to the tray instead ([`Shell::on_close`]), and the shell's
//! own requests (show the window, quit) arrive through the loop's proxy.

use std::sync::{Arc, Mutex, OnceLock, mpsc};

use blitz_dom::DocumentConfig;
use blitz_shell::{BlitzShellProxy, DataUriNetProvider, WindowConfig};
use blitz_traits::net::NetProvider;
use dioxus_native::prelude::*;
use dioxus_native::winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoopProxy},
    window::WindowId,
};
use dioxus_native::{DioxusNativeApplication, DioxusNativeWindowRenderer, WindowAttributes};

/// What the shell asks the event loop for, from any thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Show the main window and bring it to the front.
    Show,
    /// Quit: the main window closes for good.
    Quit,
    /// Relabel the tray menu in the core's language (Windows and macOS,
    /// where the tray lives on this thread).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Relabel,
}

/// What a close request of the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnClose {
    /// It closes, and the app quits.
    Quit,
    /// It hides; the tray brings it back.
    Hide,
}

/// The shell's say in the loop: how a close goes, and what to do once a
/// request arrives.
pub trait Shell: 'static {
    fn on_close(&mut self) -> OnClose;
}

static REQUESTS: OnceLock<(mpsc::Sender<Request>, Mutex<Option<EventLoopProxy>>)> = OnceLock::new();

fn requests() -> &'static (mpsc::Sender<Request>, Mutex<Option<EventLoopProxy>>) {
    REQUESTS.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        *RECEIVER.lock().unwrap_or_else(|e| e.into_inner()) = Some(receiver);
        (sender, Mutex::new(None))
    })
}

static RECEIVER: Mutex<Option<mpsc::Receiver<Request>>> = Mutex::new(None);

/// Asks the event loop for `request`, from any thread. Before the loop
/// runs it waits there.
pub fn ask(request: Request) {
    let (sender, proxy) = requests();
    let _ = sender.send(request);
    if let Some(proxy) = proxy.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        proxy.wake_up();
    }
}

/// Runs the main window with `app` as its root until it closes for good.
pub fn run(app: fn() -> Element, window: WindowAttributes, shell: impl Shell) {
    let receiver = {
        requests();
        RECEIVER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .expect("the window runs once")
    };
    let event_loop = blitz_shell::create_default_event_loop();
    let winit_proxy = event_loop.create_proxy();
    *requests().1.lock().unwrap_or_else(|e| e.into_inner()) = Some(winit_proxy.clone());
    let (proxy, event_queue) = BlitzShellProxy::new(winit_proxy);
    // Dioxus tasks (timers, resources) run on the app's own runtime.
    let _runtime = caskade_app::runtime::handle().enter();
    let vdom = VirtualDom::new(app);
    // Data URLs only: Caskade's pictures come from its artwork cache, never
    // the network (Blitz's `net` feature stays off, #87).
    let net: Arc<dyn NetProvider> = DataUriNetProvider::shared(Some(Arc::new(proxy.clone()) as _));
    vdom.provide_root_context(Arc::clone(&net));
    let document = dioxus_native::DioxusDocument::new(
        vdom,
        DocumentConfig {
            net_provider: Some(net),
            style_threading: blitz_dom::StyleThreading::Parallel,
            ..Default::default()
        },
    );
    let config = WindowConfig::with_attributes(
        Box::new(document) as _,
        DioxusNativeWindowRenderer::new(),
        window,
    );
    let application = DioxusNativeApplication::new(proxy, event_queue, config);
    if let Err(error) = event_loop.run_app(Loop {
        inner: application,
        shell,
        requests: receiver,
        main: None,
    }) {
        tracing::error!(%error, "the window's event loop failed");
    }
}

struct Loop<S> {
    inner: DioxusNativeApplication,
    shell: S,
    requests: mpsc::Receiver<Request>,
    /// The main window, once it's open.
    main: Option<WindowId>,
}

impl<S: Shell> Loop<S> {
    fn handle(&mut self, event_loop: &dyn ActiveEventLoop) {
        while let Ok(request) = self.requests.try_recv() {
            match request {
                Request::Show => {
                    if let Some(window) = crate::shell::main_window() {
                        window.set_visible(true);
                        window.set_minimized(false);
                        window.focus_window();
                    }
                }
                Request::Relabel => crate::tray::relabel(),
                Request::Quit => match self.main {
                    // Through the shell's own close: the window goes for good.
                    Some(id) => {
                        self.inner
                            .window_event(event_loop, id, WindowEvent::CloseRequested)
                    }
                    None => event_loop.exit(),
                },
            }
        }
    }
}

impl<S: Shell> ApplicationHandler for Loop<S> {
    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, cause: StartCause) {
        self.inner.new_events(event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.resumed(event_loop);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.can_create_surfaces(event_loop);
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.handle(event_loop);
        self.inner.proxy_wake_up(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        self.main.get_or_insert(window_id);
        if matches!(event, WindowEvent::CloseRequested)
            && self.shell.on_close() == OnClose::Hide
            && let Some(window) = crate::shell::main_window()
        {
            window.set_visible(false);
            return;
        }
        self.inner.window_event(event_loop, window_id, event);
    }

    fn device_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        device_id: Option<DeviceId>,
        event: DeviceEvent,
    ) {
        self.inner.device_event(event_loop, device_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.about_to_wait(event_loop);
    }

    fn suspended(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }

    fn destroy_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.destroy_surfaces(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.memory_warning(event_loop);
    }
}
