use crate::events::focus::generate_focus_events;
use crate::{Document, ScrollBehavior, ScrollLogicalPosition};
use blitz_traits::events::{
    BlitzAccessibilityAction, BlitzAccessibilityEvent, BlitzKeyEvent, BlitzPointerEvent,
    BlitzPointerId, DomEvent, DomEventData, EventState, KeyState, Point, PointerCoords, UiEvent,
};
use blitz_traits::node_id::NodeId;
use keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use markup5ever::local_name;
use std::collections::VecDeque;

pub trait EventHandler {
    fn handle_event(
        &mut self,
        chain: &[NodeId],
        event: &mut DomEvent,
        doc: &mut dyn Document,
        event_state: &mut EventState,
    );
}

pub struct NoopEventHandler;
impl EventHandler for NoopEventHandler {
    fn handle_event(
        &mut self,
        _chain: &[NodeId],
        _event: &mut DomEvent,
        _doc: &mut dyn Document,
        _event_state: &mut EventState,
    ) {
        // Do nothing
    }
}

pub struct EventDriver<'doc, Handler: EventHandler> {
    doc: &'doc mut dyn Document,
    handler: Handler,
    queue: VecDeque<DomEvent>,
}

impl<'doc, Handler: EventHandler> EventDriver<'doc, Handler> {
    pub fn new(doc: &'doc mut dyn Document, handler: Handler) -> Self {
        EventDriver {
            doc,
            handler,
            queue: VecDeque::with_capacity(4),
        }
    }

    pub fn handle_pointer_move(&mut self, event: &BlitzPointerEvent) -> Option<NodeId> {
        let mut doc = self.doc.inner_mut();

        let prev_hover_node_id = doc.hover_node_id;
        let changed = doc.set_hover_to(event.page_x(), event.page_y());
        let hover_node_id = doc.hover_node_id;

        drop(doc);

        if !changed {
            return prev_hover_node_id;
        }

        self.dispatch_hover_change(prev_hover_node_id, hover_node_id, event);

        hover_node_id
    }

    /// End hover when the pointer leaves the document, dispatching out/leave
    /// events to the previously hovered chain
    pub fn handle_pointer_leave(&mut self, event: &BlitzPointerEvent) {
        let mut doc = self.doc.inner_mut();
        let prev_hover_node_id = doc.hover_node_id;
        doc.clear_hover();
        drop(doc);

        self.dispatch_hover_change(prev_hover_node_id, None, event);
    }

    fn dispatch_hover_change(
        &mut self,
        prev_hover_node_id: Option<NodeId>,
        hover_node_id: Option<NodeId>,
        event: &BlitzPointerEvent,
    ) {
        let doc = self.doc.inner();
        let mut old_chain = prev_hover_node_id
            .map(|id| doc.node_chain(id))
            .unwrap_or_default();
        let mut new_chain = hover_node_id
            .map(|id| doc.node_chain(id))
            .unwrap_or_default();
        old_chain.reverse();
        new_chain.reverse();

        // Find the difference in the node chain of the last hovered objected and the newest
        let old_len = old_chain.len();
        let new_len = new_chain.len();

        let first_difference_index = old_chain
            .iter()
            .zip(&new_chain)
            .position(|(old, new)| old != new)
            .unwrap_or_else(|| old_len.min(new_len));

        drop(doc);

        let is_mouse = event.is_mouse();

        if let Some(target) = prev_hover_node_id {
            self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::PointerOut(event.clone()),
            ));
            if is_mouse {
                self.handle_dom_event(DomEvent::new(target, DomEventData::MouseOut(event.clone())));
            }

            // Send an mouseleave event to all old elements on the chain
            for node_id in old_chain
                .get(first_difference_index..)
                .unwrap_or(&[])
                .iter()
            {
                self.handle_dom_event(DomEvent::new(
                    *node_id,
                    DomEventData::PointerLeave(event.clone()),
                ));
                if is_mouse {
                    self.handle_dom_event(DomEvent::new(
                        *node_id,
                        DomEventData::MouseLeave(event.clone()),
                    ));
                }
            }
        }

        if let Some(target) = hover_node_id {
            self.handle_dom_event(DomEvent::new(
                target,
                DomEventData::PointerOver(event.clone()),
            ));

            if is_mouse {
                self.handle_dom_event(DomEvent::new(
                    target,
                    DomEventData::MouseOver(event.clone()),
                ));
            }

            // Send an mouseenter event to all new elements on the chain
            for node_id in new_chain
                .get(first_difference_index..)
                .unwrap_or(&[])
                .iter()
            {
                self.handle_dom_event(DomEvent::new(
                    *node_id,
                    DomEventData::PointerEnter(event.clone()),
                ));

                if is_mouse {
                    self.handle_dom_event(DomEvent::new(
                        *node_id,
                        DomEventData::MouseEnter(event.clone()),
                    ));
                }
            }
        }
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        let event = match event {
            UiEvent::Accessibility(event) => return self.handle_accessibility_event(event),
            event => event,
        };
        let doc = self.doc.inner();

        // A non-hovering pointer (finger) ends hover once it is lifted or cancelled
        let mut hover_end_event = None;
        let mut hover_node_id = doc.hover_node_id;
        let focussed_node_id = doc.focus_node_id;
        let prev_hover = doc.hover_state();
        drop(doc);

        // A document the pointer has left has no say over the cursor
        let is_leave = matches!(event, UiEvent::PointerLeave(_));

        // Update document input state (hover, focus, active, etc)
        match &event {
            UiEvent::PointerMove(event) => {
                hover_node_id = self.handle_pointer_move(event);
            }
            UiEvent::PointerDown(event) => {
                hover_node_id = self.handle_pointer_move(event);
                let mut doc = self.doc.inner_mut();
                doc.active_node();
                doc.set_mousedown_node_id(hover_node_id);
            }
            UiEvent::PointerUp(event) => {
                hover_node_id = self.handle_pointer_move(event);
                let mut doc = self.doc.inner_mut();
                doc.unactive_node();

                if event.is_primary && matches!(event.id, BlitzPointerId::Finger(_)) {
                    hover_end_event = Some(event.clone());
                }
            }
            UiEvent::PointerCancel(event) => {
                hover_node_id = self.handle_pointer_move(event);
                let mut doc = self.doc.inner_mut();
                doc.unactive_node();

                if event.is_primary && matches!(event.id, BlitzPointerId::Finger(_)) {
                    hover_end_event = Some(event.clone());
                }
            }
            UiEvent::PointerLeave(event) => {
                self.handle_pointer_leave(event);
                hover_node_id = None;
            }
            _ => {}
        };

        let target = match event {
            UiEvent::PointerMove(_) => hover_node_id,
            UiEvent::PointerUp(_) => hover_node_id,
            UiEvent::PointerDown(_) => hover_node_id,
            UiEvent::PointerCancel(_) => hover_node_id,
            UiEvent::PointerLeave(_) => hover_node_id,
            UiEvent::Wheel(_) => hover_node_id,
            UiEvent::KeyUp(_) => focussed_node_id,
            UiEvent::KeyDown(_) => focussed_node_id,
            UiEvent::Ime(_) => focussed_node_id,
            UiEvent::Paste(_) => focussed_node_id,
            UiEvent::AppleStandardKeybinding(_) => focussed_node_id,
            UiEvent::Accessibility(_) => None,
        };
        // Fall back to the root element. A document without a root element (e.g. an
        // empty iframe sub-document) has no event target, so there is nothing to do.
        let Some(target) = target.or_else(|| self.doc.inner().try_root_element().map(|el| el.id))
        else {
            return;
        };

        match event {
            UiEvent::PointerMove(data) => {
                self.handle_pointer_event(
                    target,
                    data,
                    DomEventData::PointerMove,
                    Some(DomEventData::MouseMove),
                    DomEventData::TouchMove,
                );
            }
            UiEvent::PointerUp(data) => {
                self.handle_pointer_event(
                    target,
                    data,
                    DomEventData::PointerUp,
                    Some(DomEventData::MouseUp),
                    DomEventData::TouchEnd,
                );
            }
            UiEvent::PointerDown(data) => {
                self.handle_pointer_event(
                    target,
                    data,
                    DomEventData::PointerDown,
                    Some(DomEventData::MouseDown),
                    DomEventData::TouchStart,
                );
            }
            UiEvent::PointerCancel(data) => {
                // `pointercancel` has no mouse-compatibility event, but does
                // generate a `touchcancel` for touch-like inputs.
                self.handle_pointer_event(
                    target,
                    data,
                    DomEventData::PointerCancel,
                    None::<fn(BlitzPointerEvent) -> DomEventData>,
                    DomEventData::TouchCancel,
                );
            }
            // Out/leave events were dispatched by `handle_pointer_leave`
            UiEvent::PointerLeave(_) => {}
            UiEvent::Wheel(data) => {
                self.handle_dom_event(DomEvent::new(target, DomEventData::Wheel(data)))
            }
            UiEvent::KeyUp(mut data) => {
                data.is_composing |= self.is_composing(target);
                self.handle_dom_event(DomEvent::new(target, DomEventData::KeyUp(data)))
            }
            UiEvent::KeyDown(mut data) => {
                data.is_composing |= self.is_composing(target);
                self.handle_dom_event(DomEvent::new(target, DomEventData::KeyDown(data)))
            }
            // The IME's own events are the field's to apply; script hears the
            // composition events and the input they generate.
            UiEvent::Ime(data) => {
                let mut event = DomEvent::new(target, DomEventData::Ime(data));
                self.run_default_action(&mut event);
                self.process_queue();
            }
            UiEvent::Paste(text) => {
                super::ime::handle_ime_event(
                    &mut self.doc.inner_mut(),
                    blitz_traits::events::BlitzImeEvent::Commit(text),
                    |event| self.queue.push_back(event),
                );
                self.process_queue();
            }
            UiEvent::AppleStandardKeybinding(data) => {
                let mut dom_event =
                    DomEvent::new(target, DomEventData::AppleStandardKeybinding(data));
                self.run_default_action(&mut dom_event);
                self.process_queue();
            }
            UiEvent::Accessibility(_) => {}
        };

        // Update document input state (hover, focus, active, etc)
        if let Some(event) = hover_end_event {
            self.handle_pointer_leave(&event);
        }

        // Update the cursor once the event (including any forwarding to a
        // sub-document) has been fully handled
        let doc = self.doc.inner();
        if !is_leave && doc.hover_state() != prev_hover {
            doc.update_cursor();
        }
    }

    /// An assistive technology's action, done as the user's own input
    /// would do it: a click is a click event (with its default action),
    /// focusing sends focus and blur events, and stepping a range control
    /// presses its arrow keys.
    fn handle_accessibility_event(&mut self, event: BlitzAccessibilityEvent) {
        let BlitzAccessibilityEvent { target, action } = event;
        let doc = self.doc.inner();
        let Some(node) = doc.get_node(target) else {
            return;
        };
        if !node.is_element() {
            return;
        }
        let focusable = std::iter::successors(Some(node), |node| {
            node.parent.and_then(|id| doc.get_node(id))
        })
        .find(|node| node.is_focussable())
        .map(|node| node.id);
        let click = node.synthetic_click_event(Modifiers::empty());
        let now = node
            .attr(local_name!("aria-valuenow"))
            .and_then(|value| value.trim_ascii().parse::<f64>().ok());
        drop(doc);
        // A value to set is a step up or down from the current one.
        let action = match action {
            BlitzAccessibilityAction::SetNumericValue(value) => match now {
                Some(now) if value > now => BlitzAccessibilityAction::Increment,
                Some(now) if value < now => BlitzAccessibilityAction::Decrement,
                _ => return,
            },
            action => action,
        };

        match action {
            BlitzAccessibilityAction::Focus => self.focus_for_accessibility(focusable),
            // As a browser does, focus what is activated, then click it.
            BlitzAccessibilityAction::Click
            | BlitzAccessibilityAction::Expand
            | BlitzAccessibilityAction::Collapse => {
                self.focus_for_accessibility(focusable);
                self.handle_dom_event(DomEvent::new(target, click));
            }
            BlitzAccessibilityAction::ScrollIntoView => {
                self.doc.inner_mut().scroll_into_view(
                    target,
                    ScrollBehavior::Instant,
                    ScrollLogicalPosition::Nearest,
                    ScrollLogicalPosition::Nearest,
                );
            }
            BlitzAccessibilityAction::SetNumericValue(_) => {}
            BlitzAccessibilityAction::Increment | BlitzAccessibilityAction::Decrement => {
                self.focus_for_accessibility(focusable);
                if self.doc.inner().focus_node_id != Some(target) {
                    return;
                }
                let key = match action {
                    BlitzAccessibilityAction::Increment => NamedKey::ArrowUp,
                    _ => NamedKey::ArrowDown,
                };
                for state in [KeyState::Pressed, KeyState::Released] {
                    let event = BlitzKeyEvent {
                        key: Key::Named(key),
                        code: match key {
                            NamedKey::ArrowUp => Code::ArrowUp,
                            _ => Code::ArrowDown,
                        },
                        modifiers: Modifiers::empty(),
                        location: Location::Standard,
                        is_auto_repeating: false,
                        is_composing: false,
                        state,
                        text: None,
                    };
                    self.handle_ui_event(match state {
                        KeyState::Pressed => UiEvent::KeyDown(event),
                        KeyState::Released => UiEvent::KeyUp(event),
                    });
                }
            }
        }
    }

    /// Whether an IME is composing in the text field `target`: a key event
    /// then says so (`KeyboardEvent.isComposing`).
    fn is_composing(&self, target: NodeId) -> bool {
        self.doc
            .inner()
            .get_node(target)
            .and_then(|node| node.element_data())
            .and_then(|element| element.text_input_data())
            .is_some_and(|input| input.editor.is_composing())
    }

    fn focus_for_accessibility(&mut self, focusable: Option<NodeId>) {
        let Some(focusable) = focusable else {
            return;
        };
        let queue = &mut self.queue;
        generate_focus_events(
            &mut self.doc.inner_mut(),
            &mut |doc| {
                doc.set_focus_to(focusable);
            },
            &mut |event| queue.push_back(event),
        );
        self.process_queue();
    }

    pub fn handle_dom_event(&mut self, event: DomEvent) {
        self.queue.push_back(event);
        self.process_queue();
    }

    fn handle_pointer_event(
        &mut self,
        target: NodeId,
        data: BlitzPointerEvent,
        make_ptr_data: impl FnOnce(BlitzPointerEvent) -> DomEventData,
        make_mouse_data: Option<impl FnOnce(BlitzPointerEvent) -> DomEventData>,
        make_touch_data: impl FnOnce(BlitzPointerEvent) -> DomEventData,
    ) {
        let mut ptr_event = DomEvent::new(target, make_ptr_data(data.clone()));
        let mut event_state = EventState::default();
        event_state = self.run_handler_event(&mut ptr_event, event_state);

        // Generate the corresponding compatibility event (mouse events for the
        // mouse, touch events for fingers and pen/stylus input) and expose it to
        // script. The default action is always run on the pointer event so that
        // the shell layer and default actions remain pointer-based.
        //
        // `pointercancel` has no mouse equivalent, so `make_mouse_data` is `None`
        // in that case and no mouse event is generated.
        if !event_state.is_cancelled() {
            if data.is_mouse() {
                if let Some(make_mouse_data) = make_mouse_data {
                    let mut mouse_event = DomEvent::new(target, make_mouse_data(data));
                    event_state = self.run_handler_event(&mut mouse_event, event_state);
                }
            } else if data.is_finger() || data.is_pen() {
                let mut touch_event = DomEvent::new(target, make_touch_data(data));
                event_state = self.run_handler_event(&mut touch_event, event_state);
            }
        }

        if !event_state.is_cancelled() {
            self.run_default_action(&mut ptr_event);
        }
        self.process_queue();
    }

    fn process_queue(&mut self) {
        while let Some(mut event) = self.queue.pop_front() {
            let event_state = self.run_handler_event(&mut event, EventState::default());
            if !event_state.is_cancelled() {
                self.run_default_action(&mut event);
            }
        }
    }

    fn adjust_element_coords(
        &self,
        target: NodeId,
        coords: &PointerCoords,
        element: &mut Point<f32>,
    ) {
        if let Some(rect) = self.doc.inner().get_client_bounding_rect(target) {
            element.x = coords.client_x - rect.x as f32;
            element.y = coords.client_y - rect.y as f32;
        }
    }

    fn run_handler_event(
        &mut self,
        event: &mut DomEvent,
        initial_event_state: EventState,
    ) -> EventState {
        let chain = if event.bubbles {
            let doc = self.doc.inner();
            doc.node_chain(event.target)
        } else {
            vec![event.target]
        };

        match &mut event.data {
            DomEventData::PointerMove(data)
            | DomEventData::PointerDown(data)
            | DomEventData::PointerUp(data)
            | DomEventData::PointerCancel(data)
            | DomEventData::PointerEnter(data)
            | DomEventData::PointerLeave(data)
            | DomEventData::PointerOver(data)
            | DomEventData::PointerOut(data)
            | DomEventData::MouseMove(data)
            | DomEventData::MouseDown(data)
            | DomEventData::MouseUp(data)
            | DomEventData::MouseEnter(data)
            | DomEventData::MouseLeave(data)
            | DomEventData::MouseOver(data)
            | DomEventData::MouseOut(data)
            | DomEventData::TouchStart(data)
            | DomEventData::TouchEnd(data)
            | DomEventData::TouchMove(data)
            | DomEventData::TouchCancel(data)
            | DomEventData::Click(data)
            | DomEventData::ContextMenu(data)
            | DomEventData::DoubleClick(data) => {
                self.adjust_element_coords(event.target, &data.coords, &mut data.element)
            }
            DomEventData::Wheel(data) => {
                self.adjust_element_coords(event.target, &data.coords, &mut data.element)
            }
            _ => {}
        }

        let mut event_state = initial_event_state;
        self.handler
            .handle_event(&chain, event, self.doc, &mut event_state);

        event_state
    }

    fn run_default_action(&mut self, event: &mut DomEvent) {
        let mut doc = self.doc.inner_mut();
        doc.handle_dom_event(event, |new_evt| self.queue.push_back(new_evt));
    }
}
