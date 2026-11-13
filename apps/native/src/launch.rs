//! Runs the main window's event loop (#88): Dioxus Native's application,
//! wrapped so the shell sees each window event first. `dioxus_native::
//! launch_cfg` closes the app on any close request; here a close can hide
//! the window to the tray instead ([`Shell::on_close`]), and the shell's
//! own requests (show the window, quit) arrive through the loop's proxy.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use anyrender::WindowRenderer;
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
    /// Hide the main window (to the tray) or minimise it: a game making way,
    /// from any thread. Its GPU resources are released while it can't be
    /// seen.
    Hide,
    Minimize,
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
    // The GPU backdrop, kept in step with the document before each frame
    // is painted (#149).
    let mut backdrop = crate::backdrop::Sync::default();
    let config = WindowConfig::with_attributes(Box::new(document) as _, renderer.clone(), window)
        .with_before_paint(move |doc, renderer, time| backdrop.before_paint(doc, renderer, time));
    let application = DioxusNativeApplication::new(proxy, event_queue, config);
    if let Err(error) = event_loop.run_app(Loop {
        inner: application,
        shell,
        requests: receiver,
        main: None,
        doc,
        renderer,
        paint_cpu: false,
        released: false,
        look_again: Vec::new(),
    }) {
        tracing::error!(%error, "the window's event loop failed");
    }
}

/// The window's renderer (#110): vello_hybrid on a GPU, vello_cpu (which
/// repaints only damage) where there's none or only a software one.
/// `CASKADE_RENDERER=cpu` draws on the CPU anyway; `CASKADE_RENDERER=gpu`
/// draws through wgpu on a software adapter too (lavapipe), to test the GPU
/// path, the backdrop's included (#149), without a GPU.
#[cfg(feature = "auto")]
fn renderer() -> DioxusNativeWindowRenderer {
    let asked = std::env::var("CASKADE_RENDERER").unwrap_or_default();
    DioxusNativeWindowRenderer::with_options(dioxus_native::RendererOptions {
        cpu_only: asked == "cpu",
        allow_software_gpu: asked == "gpu",
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
    /// The renderer's GPU resources are released while the window can't be
    /// seen (in the tray, minimised): see [`Self::follow_visibility`].
    released: bool,
    /// When to look at whether the window can be seen again.
    look_again: Vec<std::time::Instant>,
}

impl<S: Shell> Loop<S> {
    /// Looks whether the main window can be seen when it may have changed.
    /// Hidden (in the tray) or minimised, it costs nothing: the renderer is
    /// suspended, which drops its surface, the kept document frame, its
    /// atlases and the backdrop's GPU state (#149), and no frame is drawn.
    /// The document keeps running. Shown again, the renderer resumes and
    /// the window is painted whole.
    fn follow_visibility(&mut self) {
        use std::time::Instant;
        let now = Instant::now();
        let due = self.look_again.len();
        self.look_again.retain(|at| *at > now);
        if self.look_again.len() == due {
            return;
        }
        let Some(window) = crate::shell::main_window() else {
            return;
        };
        // Before the first frame there's nothing to release.
        if !self.released && !self.renderer.is_active() {
            return;
        }
        let hidden = window.is_visible() == Some(false) || window.is_minimized() == Some(true);
        if hidden && !self.released {
            self.renderer.suspend();
            self.released = true;
            // What that freed goes back to the system now, not at malloc's
            // leisure.
            caskade_malloc::trim();
            tracing::info!("window hidden: its GPU resources are released");
        } else if !hidden && self.released {
            let size = window.surface_size();
            self.renderer
                .resume(Arc::new(window.clone()), size.width, size.height, || {});
            if !self.renderer.complete_resume() {
                tracing::warn!("the window's renderer didn't resume");
            }
            self.released = false;
            tracing::info!("window shown: its renderer resumed");
            window.request_redraw();
        }
    }

    /// Looks at whether the window can be seen `delay` from now.
    fn look_at_visibility(&mut self, delay: std::time::Duration) {
        let at = std::time::Instant::now() + delay;
        // A burst of events asks for the same looks.
        if !self
            .look_again
            .iter()
            .any(|due| due.max(&at).duration_since(*due.min(&at)).as_millis() < 50)
        {
            self.look_again.push(at);
        }
    }

    fn handle(&mut self, event_loop: &dyn ActiveEventLoop) {
        while let Ok(request) = self.requests.try_recv() {
            match request {
                Request::Show => {
                    if let Some(window) = crate::shell::main_window() {
                        window.set_visible(true);
                        window.set_minimized(false);
                        window.focus_window();
                    }
                    self.look_at_visibility(std::time::Duration::ZERO);
                }
                Request::Fullscreen(on) => {
                    if let Some(window) = crate::shell::main_window() {
                        window.set_fullscreen(on.then_some(
                            dioxus_native::winit::monitor::Fullscreen::Borderless(None),
                        ));
                    }
                }
                Request::Paste => crate::paste::fetch(event_loop),
                Request::Hide | Request::Minimize => {
                    if let Some(window) = crate::shell::main_window() {
                        if request == Request::Hide {
                            window.set_visible(false);
                        } else {
                            window.set_minimized(true);
                        }
                    }
                    self.look_at_visibility(std::time::Duration::ZERO);
                    self.look_at_visibility(std::time::Duration::from_millis(500));
                }
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
        // A window that starts hidden (Start minimized) releases its
        // resources once its first frame is drawn.
        self.look_at_visibility(std::time::Duration::from_secs(1));
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
        // What may come with the window being hidden, minimised or shown
        // again; a window manager may set the state a moment later.
        match &event {
            WindowEvent::Focused(_) | WindowEvent::Occluded(_) | WindowEvent::SurfaceResized(_) => {
                self.look_at_visibility(std::time::Duration::ZERO);
                self.look_at_visibility(std::time::Duration::from_millis(500));
            }
            WindowEvent::RedrawRequested if self.released => {
                self.look_at_visibility(std::time::Duration::ZERO);
            }
            _ => {}
        }
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
                        self.look_at_visibility(std::time::Duration::ZERO);
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
        self.follow_visibility();
        self.inner.about_to_wait(event_loop);
        if let Some(&at) = self.look_again.iter().min() {
            use dioxus_native::winit::event_loop::ControlFlow;
            let wake = match event_loop.control_flow() {
                ControlFlow::WaitUntil(other) => at.min(other),
                _ => at,
            };
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        }
        crate::smoke::frames(
            crate::backdrop::document_frames(),
            self.renderer.backdrop_frames(),
            crate::backdrop::on_gpu(),
            self.released,
        );
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
