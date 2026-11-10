use crate::{BlitzShellEvent, event::BlitzShellProxy};
use accesskit::{Action, ActionRequest, NodeId, Rect};
use accesskit_xplat::{Adapter, EventHandler, WindowEvent as AccessKitEvent};
use blitz_dom::BaseDocument;
use blitz_traits::events::{BlitzAccessibilityAction, BlitzAccessibilityEvent, UiEvent};
use std::sync::Arc;
use winit::{
    event::WindowEvent,
    raw_window_handle::HasWindowHandle,
    window::{Window, WindowId},
};

/// State of the accessibility node tree and platform adapter.
pub struct AccessibilityState {
    // /// Adapter to connect to the [`EventLoop`](`winit::event_loop::EventLoop`).
    adapter: Adapter,
}

struct Handler {
    window_id: WindowId,
    proxy: BlitzShellProxy,
}
impl EventHandler for Handler {
    fn handle_accesskit_event(&self, event: AccessKitEvent) {
        self.proxy.send_event(BlitzShellEvent::Accessibility {
            window_id: self.window_id,
            data: Arc::new(event),
        });
    }
}

impl AccessibilityState {
    pub fn new(window: &dyn Window, proxy: BlitzShellProxy) -> Self {
        let window_id = window.id();
        Self {
            adapter: Adapter::with_combined_handler(
                #[cfg(target_os = "android")]
                &crate::current_android_app(),
                #[cfg(not(target_os = "android"))]
                window.window_handle().unwrap().as_raw(),
                Arc::new(Handler { window_id, proxy }),
            ),
        }
    }
    pub fn update_tree(&mut self, doc: &BaseDocument) {
        let _ = doc;
        self.adapter
            .update_if_active(|| doc.build_accessibility_tree());
    }

    /// Allows reacting to window events.
    ///
    /// This must be called whenever a new window event is received
    /// and before it is handled by the application.
    pub fn process_window_event(&mut self, window: &dyn Window, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(is_focused) => {
                self.adapter.set_focus(*is_focused);
            }
            WindowEvent::Moved(_) | WindowEvent::SurfaceResized(_) => {
                let outer_position: (_, _) = window
                    .outer_position()
                    .unwrap_or_default()
                    .cast::<f64>()
                    .into();
                let outer_size: (_, _) = window.outer_size().cast::<f64>().into();
                let inner_position: (_, _) = window.surface_position().cast::<f64>().into();
                let inner_size: (_, _) = window.surface_size().cast::<f64>().into();

                self.adapter.set_window_bounds(
                    Rect::from_origin_size(outer_position, outer_size),
                    Rect::from_origin_size(inner_position, inner_size),
                )
            }
            _ => (),
        }
    }
}

/// The DOM event for an assistive technology's request, if Blitz does it.
pub(crate) fn ui_event_for_request(request: &ActionRequest) -> Option<UiEvent> {
    // The window node is AccessKit's, not the document's.
    if request.target_node == NodeId(u64::MAX) {
        return None;
    }
    let action = match request.action {
        Action::Focus => BlitzAccessibilityAction::Focus,
        Action::Click => BlitzAccessibilityAction::Click,
        Action::ScrollIntoView => BlitzAccessibilityAction::ScrollIntoView,
        Action::Increment => BlitzAccessibilityAction::Increment,
        Action::Decrement => BlitzAccessibilityAction::Decrement,
        Action::Expand => BlitzAccessibilityAction::Expand,
        Action::Collapse => BlitzAccessibilityAction::Collapse,
        _ => return None,
    };
    Some(UiEvent::Accessibility(BlitzAccessibilityEvent {
        target: blitz_traits::node_id::NodeId::from_u64(request.target_node.0),
        action,
    }))
}
