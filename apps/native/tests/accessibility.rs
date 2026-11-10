//! What screen readers get from the native UI (#83): Blitz's AccessKit tree
//! for demo elements built as Caskade's components build theirs (roles,
//! names, states, values and live regions), kept current as the UI changes,
//! and screen-reader actions reaching the elements' Dioxus handlers. Blitz
//! builds the tree from the vendored patch series (`vendor/blitz`).
//!
//! `cargo test -p caskade-native --test accessibility`

use accesskit::Action;
use blitz_dom::{Document, DocumentConfig, NodeId};
use blitz_traits::events::{BlitzAccessibilityAction, BlitzAccessibilityEvent, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;
use std::fmt::Write as _;

#[path = "accessibility/demo.rs"]
mod demo;

use demo::{Demo, take_done};

struct Window {
    doc: DioxusDocument,
}

impl Window {
    fn open() -> Self {
        let mut doc = DioxusDocument::new(
            VirtualDom::new(Demo),
            DocumentConfig {
                viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                ..Default::default()
            },
        );
        doc.initial_build();
        let mut window = Self { doc };
        window.settle();
        window
    }

    fn settle(&mut self) {
        for _ in 0..5 {
            Document::poll(&mut self.doc, None);
            self.doc.inner.borrow_mut().resolve(0.0);
        }
    }

    fn id(&self, id: &str) -> NodeId {
        self.doc
            .inner
            .borrow()
            .get_element_by_id(id)
            .unwrap_or_else(|| panic!("no #{id}"))
    }

    fn act(&mut self, id: &str, action: BlitzAccessibilityAction) {
        let target = self.id(id);
        Document::handle_ui_event(
            &mut self.doc,
            UiEvent::Accessibility(BlitzAccessibilityEvent { target, action }),
        );
        self.settle();
    }

    /// The tree as text: one line per node, children indented. Text that
    /// names its parent is folded into the name; other text (a live
    /// region's) is a quoted line. Ids and bounds (fonts) are left out.
    fn snapshot(&self) -> String {
        let doc = self.doc.inner.borrow();
        let update = doc.build_accessibility_tree();
        let nodes: std::collections::HashMap<_, _> = update.nodes.iter().cloned().collect();
        let naming: std::collections::HashSet<_> = update
            .nodes
            .iter()
            .flat_map(|(_, node)| node.labelled_by().iter().copied())
            .collect();
        let mut out = String::new();
        let mut stack = vec![(update.tree.as_ref().unwrap().root, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            let node = &nodes[&id];
            let role = format!("{:?}", node.role());
            // Text is a label without a tag.
            if node.role() == accesskit::Role::Label && node.html_tag().is_none() {
                let text = node.value().unwrap_or_default().trim();
                if !naming.contains(&id) && !text.is_empty() {
                    let _ = writeln!(out, "{}{text:?}", "  ".repeat(depth));
                }
                continue;
            }
            // Wrappers without semantics add depth but nothing to hear.
            let quiet = matches!(role.as_str(), "GenericContainer" | "Unknown" | "Window")
                && node.label().is_none()
                && node.live().is_none();
            if !quiet {
                let mut line = format!("{}{role}", "  ".repeat(depth));
                let text: Vec<_> = node
                    .labelled_by()
                    .iter()
                    .filter_map(|id| nodes.get(id))
                    .filter_map(|node| node.value())
                    .collect();
                let name = node
                    .label()
                    .map(str::to_owned)
                    .or_else(|| (!text.is_empty()).then(|| text.join(" ").trim().to_owned()));
                if let Some(name) = name.filter(|name| !name.is_empty()) {
                    let _ = write!(line, " {name:?}");
                }
                let mut states = Vec::new();
                macro_rules! state {
                    ($name:literal, $value:expr) => {
                        if let Some(value) = $value {
                            states.push(format!("{}={:?}", $name, value));
                        }
                    };
                }
                state!("toggled", node.toggled());
                state!("selected", node.is_selected());
                state!("expanded", node.is_expanded());
                state!("current", node.aria_current());
                state!("live", node.live());
                state!("popup", node.has_popup());
                state!("orientation", node.orientation());
                state!("invalid", node.invalid());
                state!("value", node.value());
                state!("now", node.numeric_value());
                state!("min", node.min_numeric_value());
                state!("max", node.max_numeric_value());
                state!("placeholder", node.placeholder());
                state!("pos", node.position_in_set());
                state!("of", node.size_of_set());
                for (flag, on) in [
                    ("modal", node.is_modal()),
                    ("busy", node.is_busy()),
                    ("atomic", node.is_live_atomic()),
                    ("required", node.is_required()),
                    ("disabled", node.is_disabled()),
                ] {
                    if on {
                        states.push(flag.to_owned());
                    }
                }
                if !node.controls().is_empty() {
                    states.push("controls".to_owned());
                }
                if node.active_descendant().is_some() {
                    states.push("activedescendant".to_owned());
                }
                if node.bounds().is_none() && role != "Window" {
                    states.push("NO BOUNDS".to_owned());
                }
                let actions: Vec<_> = [
                    ("focus", Action::Focus),
                    ("click", Action::Click),
                    ("expand", Action::Expand),
                    ("collapse", Action::Collapse),
                    ("step", Action::Increment),
                ]
                .into_iter()
                .filter(|(_, action)| node.supports_action(*action))
                .map(|(name, _)| name)
                .collect();
                if !actions.is_empty() {
                    states.push(format!("[{}]", actions.join(" ")));
                }
                if !states.is_empty() {
                    let _ = write!(line, " {}", states.join(" "));
                }
                out.push_str(&line);
                out.push('\n');
            }
            let depth = if quiet { depth } else { depth + 1 };
            for child in node.children().iter().rev() {
                stack.push((*child, depth));
            }
        }
        out
    }
}

/// The demo's tree as a screen reader gets it.
const TREE: &str = r#"Navigation "Library"
  Link "All games" current=Page [focus click]
  Link "Installed" [focus click]
Main
  Heading "Hollow Knight"
  Button "Favourite" toggled=False [focus click]
  Button "Sort by name" expanded=false popup=Listbox [focus click expand]
  Label "Show hidden games"
    CheckBox "Show hidden games" toggled=True [focus click]
  Slider "Volume" value="40%" now=40.0 min=0.0 max=100.0 [focus click step]
  TextInput "Search" invalid=True value="" placeholder="Search your library" required [focus click]
  TabList orientation=Horizontal
    Tab "Overview" selected=true [focus click]
    Tab "Achievements" selected=false [focus click]
  Dialog "Uninstall Hollow Knight?" modal
    Button "Uninstall" [focus click]
Footer
  Status live=Polite busy atomic
    "Installing Celeste"
    ProgressIndicator "Celeste" now=25.0 min=0.0 max=100.0
  Alert live=Assertive atomic
    "Couldn't reach RomM"
"#;

#[test]
fn the_tree_has_roles_names_states_and_values() {
    let window = Window::open();
    let tree = window.snapshot();
    assert_eq!(tree, TREE, "the tree is now:\n{tree}");
}

#[test]
fn the_tree_follows_the_ui_and_actions_reach_the_handlers() {
    let mut window = Window::open();
    take_done();

    // A screen reader's click runs the button's handler, focuses it, and
    // the tree has its new state.
    window.act("favourite", BlitzAccessibilityAction::Click);
    assert_eq!(take_done(), ["favourite"]);
    assert_eq!(
        window.doc.inner.borrow().get_focussed_node_id(),
        Some(window.id("favourite"))
    );
    let tree = window.snapshot();
    assert!(
        tree.contains(r#"Button "Favourite" toggled=True"#),
        "{tree}"
    );

    // Expanding the sort button shows its options.
    window.act("sort", BlitzAccessibilityAction::Expand);
    let tree = window.snapshot();
    assert!(tree.contains("expanded=true"), "{tree}");
    assert!(
        tree.contains(r#"ListBoxOption "Recently played""#),
        "{tree}"
    );

    // Stepping the slider presses its arrow keys.
    window.act("volume", BlitzAccessibilityAction::Increment);
    window.act("volume", BlitzAccessibilityAction::Increment);
    window.act("volume", BlitzAccessibilityAction::Decrement);
    let tree = window.snapshot();
    assert!(tree.contains(r#"value="50%" now=50.0"#), "{tree}");

    // Focus moves the focus; nothing else happens.
    window.act("confirm", BlitzAccessibilityAction::Focus);
    assert_eq!(
        window.doc.inner.borrow().get_focussed_node_id(),
        Some(window.id("volume")),
        "a dialog without tabindex can't take the focus"
    );
    assert!(take_done().is_empty());
}
