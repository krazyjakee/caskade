//! The input rules' view of Blitz's document: elements, attributes and
//! layout boxes ([`FocusTree`]), and what an action does to them ([`Ui`]).
//!
//! Anything that runs app code (a click's or a focus change's listeners,
//! going back, the on-screen keyboard) is left as an [`Effect`] for after
//! the document is let go: a listener may read the document itself.

use blitz_dom::{
    BaseDocument, LocalName, NodeId, QualName, ScrollBehavior, ScrollLogicalPosition, ns,
};
use caskade_ui_input::dispatch::Ui;
use caskade_ui_input::{FocusTree, Rect};

/// Shows the focus ring: Blitz never matches `:focus-visible`, so styles
/// select `[data-focus-visible]` instead.
pub const RING: &str = "data-focus-visible";

/// Whether no control has focus: nothing, the input root, or an element
/// around it (Blitz starts with focus on the document's own).
pub fn idle(doc: &BaseDocument, holder: NodeId) -> bool {
    let Some(focused) = doc.get_focussed_node_id() else {
        return true;
    };
    let mut at = Some(holder);
    while let Some(here) = at {
        if here == focused {
            return true;
        }
        at = doc.get_node(here).and_then(|node| node.parent);
    }
    false
}

/// What an action asked for that runs app code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Focus moved: blur and focus listeners run.
    Focus { from: Option<NodeId>, to: NodeId },
    /// Focus left `from` for nothing.
    Blur(NodeId),
    /// A click on the element: its listeners run.
    Click(NodeId),
    /// The on-screen keyboard opens on a field.
    Keyboard(NodeId),
    /// The previous view.
    Back,
    /// A box was scrolled by the input layer: Blitz sends no scroll event
    /// for that.
    Scrolled(NodeId),
}

/// The document while an action is handled.
pub struct BlitzUi<'d> {
    doc: &'d mut BaseDocument,
    body: NodeId,
    /// The input root, which holds focus when no control does, so keys
    /// still reach the keyboard map.
    holder: Option<NodeId>,
    /// Focus moves show the ring: the action came from keys or a pad.
    pub ring: bool,
    pub effects: Vec<Effect>,
}

impl<'d> BlitzUi<'d> {
    pub fn new(doc: &'d mut BaseDocument, holder: Option<NodeId>) -> Self {
        let html = doc.root_element().id;
        let body = doc
            .get_node(html)
            .and_then(|node| {
                node.children.iter().copied().find(|&child| {
                    doc.get_node(child)
                        .and_then(|child| child.element_data())
                        .is_some_and(|element| &*element.name.local == "body")
                })
            })
            .unwrap_or(html);
        Self {
            doc,
            body,
            holder,
            ring: true,
            effects: Vec::new(),
        }
    }

    pub fn doc(&mut self) -> &mut BaseDocument {
        self.doc
    }

    /// Moves focus to `to` without the dispatcher: a click on a control.
    pub fn take_focus(&mut self, to: NodeId) {
        let from = self.focused();
        if from == Some(to) {
            return;
        }
        self.doc.set_focus_to(to);
        self.mark_ring(from, Some(to));
        self.effects.push(Effect::Focus { from, to });
    }

    fn set_attr(&mut self, node: NodeId, name: &str, value: Option<&str>) {
        // A node gone from the document (focus handed back to a control the
        // view has since drawn again) has nothing to mark.
        if self.doc.get_node(node).is_none() {
            return;
        }
        let name = QualName::new(None, ns!(), LocalName::from(name));
        let mut mutator = self.doc.mutate();
        match value {
            Some(value) => mutator.set_attribute(node, name, value),
            None => mutator.clear_attribute(node, name),
        }
    }

    /// The ring leaves `from` and shows on `to` when the move came from keys
    /// or a pad.
    fn mark_ring(&mut self, from: Option<NodeId>, to: Option<NodeId>) {
        if let Some(from) = from
            && self.attr(from, RING).is_some()
        {
            self.set_attr(from, RING, None);
        }
        if self.ring
            && let Some(to) = to
            && Some(to) != self.holder
        {
            self.set_attr(to, RING, Some(""));
        }
    }

    fn is_element(&self, node: NodeId) -> bool {
        self.doc
            .get_node(node)
            .is_some_and(|node| node.is_element())
    }

    /// Whether `node` scrolls its content vertically.
    fn scrolls(&self, node: NodeId) -> bool {
        let Some(node) = self.doc.get_node(node) else {
            return false;
        };
        // Stylo's computed `overflow-y`, read through its name: the crate
        // isn't a dependency of the app's.
        let overflow = node
            .primary_styles()
            .map(|styles| format!("{:?}", styles.get_box().overflow_y));
        matches!(overflow.as_deref(), Some("Auto" | "Scroll"))
            && node.scroll_height() > node.client_height() + 0.5
    }
}

impl FocusTree for BlitzUi<'_> {
    type Node = NodeId;

    fn root(&self) -> NodeId {
        self.body
    }

    fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.doc.get_node(node)?.parent
    }

    fn children(&self, node: NodeId) -> Vec<NodeId> {
        self.doc.get_node(node).map_or_else(Vec::new, |node| {
            node.children
                .iter()
                .copied()
                .filter(|&child| self.is_element(child))
                .collect()
        })
    }

    fn tag(&self, node: NodeId) -> &str {
        self.doc
            .get_node(node)
            .and_then(|node| node.element_data())
            .map_or("", |element| &element.name.local)
    }

    fn attr(&self, node: NodeId, name: &str) -> Option<&str> {
        self.doc.get_node(node)?.attr(LocalName::from(name))
    }

    fn rect(&self, node: NodeId) -> Rect {
        self.doc
            .get_client_bounding_rect(node)
            .map_or_else(Rect::default, |rect| {
                Rect::new(
                    rect.x as f32,
                    rect.y as f32,
                    rect.width as f32,
                    rect.height as f32,
                )
            })
    }

    fn text(&self, node: NodeId) -> String {
        self.doc
            .get_node(node)
            .map(|node| node.text_content())
            .unwrap_or_default()
    }
}

impl Ui for BlitzUi<'_> {
    fn focused(&self) -> Option<NodeId> {
        if self.holder.is_some_and(|holder| idle(self.doc, holder)) {
            return None;
        }
        self.doc.get_focussed_node_id()
    }

    fn focus(&mut self, node: NodeId, instant: bool) {
        if !self
            .doc
            .get_node(node)
            .is_some_and(|found| found.flags.is_in_document())
        {
            return;
        }
        let from = self.focused();
        if from != Some(node) {
            self.doc.set_focus_to(node);
            self.effects.push(Effect::Focus { from, to: node });
        }
        self.mark_ring(from, Some(node));
        let behavior = if instant {
            ScrollBehavior::Instant
        } else {
            ScrollBehavior::Smooth
        };
        let target = caskade_ui_input::spatial::stand_in(self, node);
        self.doc.scroll_into_view(
            target,
            behavior,
            ScrollLogicalPosition::Nearest,
            ScrollLogicalPosition::Nearest,
        );
    }

    fn blur(&mut self) {
        let Some(from) = self.focused() else {
            return;
        };
        self.mark_ring(Some(from), None);
        match self.holder {
            Some(holder) => {
                self.doc.set_focus_to(holder);
            }
            None => self.doc.clear_focus(),
        }
        self.effects.push(Effect::Blur(from));
    }

    fn press(&mut self, node: NodeId, key: &str) -> bool {
        // Blitz has no way for app code to send a key through the page's
        // listeners: native controls take actions through bindings instead.
        tracing::debug!(?node, key, "no key press in the native UI");
        false
    }

    fn click(&mut self, node: NodeId) {
        self.effects.push(Effect::Click(node));
    }

    fn value(&self, node: NodeId) -> String {
        self.doc
            .get_node(node)
            .and_then(|node| node.element_data())
            .and_then(|element| element.text_input_data())
            .map(|input| input.editor.raw_text().to_owned())
            .or_else(|| self.attr(node, "value").map(str::to_owned))
            .unwrap_or_default()
    }

    fn go_back(&mut self) {
        self.effects.push(Effect::Back);
    }

    fn scroller(&self, node: Option<NodeId>) -> Option<NodeId> {
        let mut at = node.and_then(|node| self.parent(node));
        while let Some(here) = at {
            if self.scrolls(here) {
                return Some(here);
            }
            at = self.parent(here);
        }
        caskade_ui_input::tree::descendants(self, self.body)
            .into_iter()
            .find(|&node| self.attr(node, "data-cs") == Some("view"))
    }

    fn scroll_by(&mut self, node: NodeId, by: f32) {
        self.doc
            .scroll_by(node, 0.0, f64::from(by), ScrollBehavior::Instant);
        self.effects.push(Effect::Scrolled(node));
    }

    fn open_keyboard(&mut self, field: NodeId) {
        self.effects.push(Effect::Keyboard(field));
    }
}
