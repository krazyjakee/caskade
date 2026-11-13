//! Runs the main window's event loop (#88): Dioxus Native's application,
//! wrapped so the shell sees each window event first. `dioxus_native::
//! launch_cfg` closes the app on any close request; here a close can hide
//! the window to the tray instead ([`Shell::on_close`]), and the shell's
//! own requests (show the window, quit) arrive through the loop's proxy.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_shell::{BlitzShellProxy, WindowConfig};
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
    /// The main window's close button, in the title bar Caskade draws
    /// (#96): as the OS's own close, it hides to the tray or quits.
    Close,
    /// Relabel the tray menu in the core's language (Windows and macOS,
    /// where the tray lives on this thread).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Relabel,
    /// Apply the look waiting in `theme` (#90).
    Restyle,
    /// Console mode's window (#107): fullscreen, or back to its desktop
    /// size and place.
    Fullscreen(bool),
    /// Read the clipboard for a paste outside a text field (#132).
    Paste,
}

/// What a close request of the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnClose {
    /// It closes, and the app quits.
    Quit,
    /// It hides; the tray brings it back.
    Hide,
    /// It stays open: something asks first (Settings' unsaved edits, #103),
    /// and closes it again once answered.
    Hold,
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
        *caskade_core::lock(&RECEIVER) = Some(receiver);
        (sender, Mutex::new(None))
    })
}

static RECEIVER: Mutex<Option<mpsc::Receiver<Request>>> = Mutex::new(None);

/// Asks the event loop for `request`, from any thread. Before the loop
/// runs it waits there.
pub fn ask(request: Request) {
    let (sender, proxy) = requests();
    let _ = sender.send(request);
    if let Some(proxy) = caskade_core::lock(proxy).as_ref() {
        proxy.wake_up();
    }
}

/// Runs the main window with `app` as its root until it closes for good,
/// styled with `look` from its first frame.
pub fn run(
    app: fn() -> Element,
    window: WindowAttributes,
    look: &crate::theme::Look,
    shell: impl Shell,
) {
    let receiver = {
        requests();
        caskade_core::lock(&RECEIVER)
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
    // Data URLs and theme assets only: Caskade's pictures come from its
    // artwork cache, never the network (Blitz's `net` feature stays off, #87).
    let net: Arc<dyn NetProvider> = crate::theme::Net::shared(Some(Arc::new(proxy.clone()) as _));
    vdom.provide_root_context(Arc::clone(&net));
    let document = dioxus_native::DioxusDocument::new(
        vdom,
        DocumentConfig {
            net_provider: Some(net),
            base_url: Some(crate::theme::BASE_URL.to_owned()),
            font_ctx: Some(crate::theme::fonts()),
            style_threading: blitz_dom::StyleThreading::Parallel,
            ..Default::default()
        },
    );
    // The look before the first frame (#90).
    let doc = Rc::clone(&document.inner);
    crate::theme::apply(&mut doc.borrow_mut(), look);
    let renderer = renderer();
    let config = WindowConfig::with_attributes(Box::new(document) as _, renderer.clone(), window);
    let application = DioxusNativeApplication::new(proxy, event_queue, config);
    if let Err(error) = event_loop.run_app(Loop {
        inner: application,
        shell,
        requests: receiver,
        main: None,
        doc,
        renderer,
        paint_cpu: false,
    }) {
        tracing::error!(%error, "the window's event loop failed");
    }
}

/// The window's renderer (#110): vello_hybrid on a GPU, vello_cpu (which
/// repaints only damage) where there's none or only a software one.
/// `CASKADE_RENDERER=cpu` draws on the CPU anyway.
#[cfg(feature = "auto")]
fn renderer() -> DioxusNativeWindowRenderer {
    DioxusNativeWindowRenderer::with_options(dioxus_native::RendererOptions {
        cpu_only: std::env::var("CASKADE_RENDERER").is_ok_and(|renderer| renderer == "cpu"),
        ..Default::default()
    })
}

#[cfg(not(feature = "auto"))]
fn renderer() -> DioxusNativeWindowRenderer {
    DioxusNativeWindowRenderer::new()
}

struct Loop<S> {
    inner: DioxusNativeApplication,
    shell: S,
    requests: mpsc::Receiver<Request>,
    /// The main window, once it's open.
    main: Option<WindowId>,
    /// Its document, for the look.
    doc: Rc<RefCell<BaseDocument>>,
    /// The window's renderer, to tell when it draws on the CPU.
    renderer: DioxusNativeWindowRenderer,
    /// Whether the document says so (`data-paint="cpu"`).
    paint_cpu: bool,
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
                Request::Fullscreen(on) => {
                    if let Some(window) = crate::shell::main_window() {
                        window.set_fullscreen(on.then_some(
                            dioxus_native::winit::monitor::Fullscreen::Borderless(None),
                        ));
                    }
                }
                Request::Paste => crate::paste::fetch(event_loop),
                Request::Relabel => crate::tray::relabel(),
                Request::Restyle => {
                    if crate::theme::apply_pending(&mut self.doc.borrow_mut())
                        && let Some(window) = crate::shell::main_window()
                    {
                        window.request_redraw();
                    }
                }
                Request::Close => {
                    if let Some(id) = self.main {
                        self.window_event(event_loop, id, WindowEvent::CloseRequested);
                    }
                }
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
        // Files dragged onto the window (#99); a file list it asked for is
        // its own, not the shell's.
        // Console mode's curtain lifts when Caskade has focus again (#107).
        if let WindowEvent::Focused(focused) = event {
            let _ = crate::console::focus_changes().send(focused);
        }
        // The clipboard's text for a paste outside a text field (#132).
        if crate::paste::follow(&event) {
            return;
        }
        if crate::drops::follow(event_loop, &event)
            && matches!(event, WindowEvent::DataTransferReceived { .. })
        {
            return;
        }
        if matches!(event, WindowEvent::CloseRequested) {
            match self.shell.on_close() {
                OnClose::Hold => return,
                OnClose::Hide => {
                    if let Some(window) = crate::shell::main_window() {
                        window.set_visible(false);
                        return;
                    }
                }
                OnClose::Quit => {}
            }
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
        // Without a usable GPU the window falls back to the CPU as it
        // starts (#110): ambient loops pause, as in a webview painting on
        // the CPU.
        if !self.paint_cpu && self.renderer.is_cpu() {
            self.paint_cpu = true;
            tracing::info!(
                reason = self
                    .renderer
                    .fallback_reason()
                    .as_deref()
                    .unwrap_or("built for it"),
                "the window draws on the CPU"
            );
            crate::theme::mark_cpu_paint(&mut self.doc.borrow_mut());
            if let Some(window) = crate::shell::main_window() {
                window.request_redraw();
            }
        }
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
