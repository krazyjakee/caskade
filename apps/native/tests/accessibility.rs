//! What screen readers get from the native UI (#83): Blitz's AccessKit tree
//! for demo elements built as Caskade's components build theirs (roles,
//! names, states, values and live regions), kept current as the UI changes,
//! and screen-reader actions reaching the elements' Dioxus handlers. Blitz
//! builds the tree from the vendored patch series (`vendor/blitz`).
//!
//! `cargo test -p caskade-native --test accessibility`

use blitz_dom::{Document, DocumentConfig, NodeId};
use blitz_traits::events::{BlitzAccessibilityAction, BlitzAccessibilityEvent, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;

#[path = "accessibility/demo.rs"]
mod demo;

/// The snapshot format, shared with the components' tests.
#[path = "../src/a11y_snapshot.rs"]
mod a11y_snapshot;

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

    fn snapshot(&self) -> String {
        a11y_snapshot::snapshot(&self.doc.inner.borrow())
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
