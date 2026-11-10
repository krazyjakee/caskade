//! The components in a real Blitz document without a window (#93), inside
//! the input layer's root as in the app: each is pressed with a mouse, the
//! keyboard (Enter, Space as the APG has them) and a pad (A, through the
//! app's events), and read as a screen reader reads it (the AccessKit tree,
//! `crate::a11y_snapshot`). Their theming (anatomy and every built-in
//! theme) is checked by the gallery's screenshots and `crate::anatomy`.

use super::*;
use crate::input::InputRoot;
use blitz_dom::{DocumentConfig, LocalName, NodeId};
use blitz_traits::events::{BlitzKeyEvent, KeyState, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use caskade_app::commands::input::InputActionEvent;
use caskade_input::{InputAction, Source};
use caskade_ui_input::{Action, GlyphFamily};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use dioxus_native::prelude::*;
use std::sync::Mutex;
use std::time::Duration;

/// What the demo's controls did.
static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn did(what: impl Into<String>) {
    DONE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(what.into());
}

fn take_done() -> Vec<String> {
    std::mem::take(&mut *DONE.lock().unwrap_or_else(|e| e.into_inner()))
}

pub(crate) struct Window {
    doc: DioxusDocument,
}

impl Window {
    pub(crate) fn open(root: fn() -> Element) -> Self {
        let mut doc = DioxusDocument::new(
            VirtualDom::new(root),
            DocumentConfig {
                viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                ..Default::default()
            },
        );
        doc.initial_build();
        let mut window = Self { doc };
        window.settle();
        take_done();
        window
    }

    /// Renders, lays out, and lets the input layer's jobs run.
    pub(crate) fn settle(&mut self) {
        for _ in 0..40 {
            blitz_dom::Document::poll(&mut self.doc, None);
            self.doc.inner.borrow_mut().resolve(0.0);
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    pub(crate) fn id(&self, id: &str) -> NodeId {
        self.doc
            .inner
            .borrow()
            .get_element_by_id(id)
            .unwrap_or_else(|| panic!("no #{id}"))
    }

    pub(crate) fn attr(&self, id: &str, name: &str) -> Option<String> {
        let node = self.id(id);
        let doc = self.doc.inner.borrow();
        doc.get_node(node)
            .and_then(|node| node.attr(LocalName::from(name)))
            .map(str::to_owned)
    }

    pub(crate) fn focus(&mut self, id: &str) {
        let node = self.id(id);
        self.doc.inner.borrow_mut().set_focus_to(node);
        self.settle();
    }

    fn key_event(&mut self, key: Key, state: KeyState) {
        let event = BlitzKeyEvent {
            key,
            code: Code::Unidentified,
            modifiers: Modifiers::empty(),
            location: Location::Standard,
            is_auto_repeating: false,
            is_composing: false,
            state,
            text: None,
        };
        let event = match state {
            KeyState::Pressed => UiEvent::KeyDown(event),
            KeyState::Released => UiEvent::KeyUp(event),
        };
        blitz_dom::Document::handle_ui_event(&mut self.doc, event);
    }

    /// A key pressed and released on the focused element.
    pub(crate) fn key(&mut self, key: Key) {
        self.key_event(key.clone(), KeyState::Pressed);
        self.settle();
        self.key_event(key, KeyState::Released);
        self.settle();
    }

    /// A key held down (not released).
    pub(crate) fn key_down(&mut self, key: Key) {
        self.key_event(key, KeyState::Pressed);
        self.settle();
    }

    pub(crate) fn key_up(&mut self, key: Key) {
        self.key_event(key, KeyState::Released);
        self.settle();
    }

    /// A pad's action, as the app's events bring it.
    pub(crate) fn pad(&mut self, action: Action) {
        let _ = crate::shell::events().send(caskade_app::events::Event::InputActionEvent(
            InputActionEvent(InputAction {
                action,
                source: Source::Pad,
                repeat: false,
                glyphs: GlyphFamily::Xbox,
            }),
        ));
        self.settle();
    }

    fn middle(&self, id: &str) -> (f32, f32) {
        let node = self.id(id);
        let rect = self
            .doc
            .inner
            .borrow()
            .get_client_bounding_rect(node)
            .unwrap_or_else(|| panic!("#{id} has no box"));
        (
            (rect.x + rect.width / 2.0) as f32,
            (rect.y + rect.height / 2.0) as f32,
        )
    }

    /// The mouse moved over the middle of `id`.
    pub(crate) fn hover(&mut self, id: &str) {
        let (x, y) = self.middle(id);
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerMove(pointer(x, y, false)),
        );
        self.settle();
    }

    /// A mouse click on the middle of `id`.
    pub(crate) fn click(&mut self, id: &str) {
        let (x, y) = self.middle(id);
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerMove(pointer(x, y, false)),
        );
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerDown(pointer(x, y, true)),
        );
        self.settle();
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerUp(pointer(x, y, false)),
        );
        self.settle();
    }

    /// The mouse button held down on `id` (not released).
    pub(crate) fn mouse_down(&mut self, id: &str) {
        let (x, y) = self.middle(id);
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerMove(pointer(x, y, false)),
        );
        blitz_dom::Document::handle_ui_event(
            &mut self.doc,
            UiEvent::PointerDown(pointer(x, y, true)),
        );
        self.settle();
    }

    /// What a screen reader gets from `id`'s subtree.
    pub(crate) fn heard(&self, id: &str) -> String {
        crate::a11y_snapshot::snapshot_of(&self.doc.inner.borrow(), self.id(id))
    }
}

fn pointer(x: f32, y: f32, down: bool) -> blitz_traits::events::BlitzPointerEvent {
    use blitz_traits::events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, PointerCoords,
        PointerDetails,
    };
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons: if down {
            MouseEventButtons::from(MouseEventButton::Main)
        } else {
            MouseEventButtons::None
        },
        mods: Modifiers::empty(),
        details: PointerDetails::default(),
        element: Default::default(),
        active_pointers: Default::default(),
    }
}

/// Pad actions reach every input root listening: one window at a time.
fn one_window() -> std::sync::MutexGuard<'static, ()> {
    crate::input::tests::ONE_WINDOW
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

const ENTER: Key = Key::Named(NamedKey::Enter);

fn space() -> Key {
    Key::Character(" ".into())
}

// ── Buttons and links ──

fn buttons() -> Element {
    rsx! {
        InputRoot {
            Button { id: "save", variant: ButtonVariant::Primary, on_press: |_| did("save"), "Save" }
            Button { id: "off", disabled: true, on_press: |_| did("off"), "Can't" }
            Button { id: "busy", pending: true, on_press: |_| did("busy"), "Saving" }
            Link { id: "home", href: "/home", on_press: |_| did("home"), current: true, "Home" }
        }
    }
}

#[test]
fn a_button_is_pressed_by_mouse_enter_space_and_a_pad() {
    let _one = one_window();
    let mut window = Window::open(buttons);
    window.click("save");
    assert_eq!(take_done(), ["save"]);
    window.focus("save");
    window.key(ENTER);
    assert_eq!(take_done(), ["save"]);
    // Space presses on release, and shows it pressed meanwhile.
    window.key_down(space());
    assert_eq!(window.attr("save", "data-pressed").as_deref(), Some("true"));
    assert!(take_done().is_empty());
    window.key_up(space());
    assert_eq!(take_done(), ["save"]);
    assert_eq!(window.attr("save", "data-pressed"), None);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["save"]);
    // Disabled and pending buttons do nothing, and say so.
    window.click("off");
    window.click("busy");
    assert!(take_done().is_empty());
    assert_eq!(window.attr("off", "data-disabled").as_deref(), Some("true"));
    assert_eq!(window.attr("busy", "data-pending").as_deref(), Some("true"));
}

#[test]
fn a_button_shows_hover_press_and_focus_as_the_webviews_attributes() {
    let _one = one_window();
    let mut window = Window::open(buttons);
    assert_eq!(window.attr("save", "data-cs").as_deref(), Some("button"));
    assert_eq!(
        window.attr("save", "data-variant").as_deref(),
        Some("primary")
    );
    assert_eq!(window.attr("save", "data-size").as_deref(), Some("md"));
    assert_eq!(window.attr("save", "data-hovered"), None);
    window.hover("save");
    assert_eq!(window.attr("save", "data-hovered").as_deref(), Some("true"));
    window.mouse_down("save");
    assert_eq!(window.attr("save", "data-pressed").as_deref(), Some("true"));
    // A click puts focus on the control (the input layer's, as Blitz
    // focuses only fields).
    window.click("save");
    assert_eq!(window.attr("save", "data-pressed"), None);
    assert_eq!(window.attr("save", "data-focused").as_deref(), Some("true"));
    window.hover("home");
    assert_eq!(window.attr("save", "data-hovered"), None);
}

#[test]
fn buttons_and_links_as_a_screen_reader_hears_them() {
    let _one = one_window();
    let window = Window::open(buttons);
    assert_eq!(window.heard("save"), "Button \"Save\" [focus click]\n");
    // Disabled: no actions offered.
    assert_eq!(window.heard("off"), "Button \"Can't\" disabled\n");
    assert_eq!(
        window.heard("home"),
        "Link \"Home\" current=Page [focus click]\n"
    );
}

#[test]
fn a_link_follows_on_enter_and_a_pad_but_not_space() {
    let _one = one_window();
    let mut window = Window::open(buttons);
    window.click("home");
    assert_eq!(take_done(), ["home"]);
    window.focus("home");
    window.key(ENTER);
    assert_eq!(take_done(), ["home"]);
    window.key(space());
    assert!(take_done().is_empty());
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["home"]);
}

// ── Switches and checkboxes ──

#[component]
fn Toggles() -> Element {
    let mut on = use_signal(|| false);
    let mut ticked = use_signal(|| true);
    let mut mixed = use_signal(|| true);
    rsx! {
        InputRoot {
            Switch {
                id: "wifi",
                checked: on(),
                on_change: move |value| { on.set(value); did(format!("wifi {value}")) },
                label: "Wi-Fi",
            }
            Switch { id: "locked", checked: true, readonly: true, on_change: |_| did("locked"), label: "Locked" }
            Checkbox {
                id: "hidden",
                checked: ticked(),
                on_change: move |value| { ticked.set(value); did(format!("hidden {value}")) },
                label: "Show hidden games",
            }
            Checkbox {
                id: "all",
                checked: false,
                indeterminate: mixed(),
                on_change: move |value| { mixed.set(false); did(format!("all {value}")) },
                label: "Select all",
            }
        }
    }
}

fn toggles() -> Element {
    rsx! { Toggles {} }
}

#[test]
fn a_switch_toggles_by_mouse_space_and_a_pad_but_not_enter() {
    let _one = one_window();
    let mut window = Window::open(toggles);
    window.click("wifi");
    assert_eq!(take_done(), ["wifi true"]);
    assert_eq!(
        window.attr("wifi", "data-selected").as_deref(),
        Some("true")
    );
    assert_eq!(window.attr("wifi", "aria-checked").as_deref(), Some("true"));
    window.focus("wifi");
    window.key(space());
    assert_eq!(take_done(), ["wifi false"]);
    assert_eq!(window.attr("wifi", "data-selected"), None);
    window.key(ENTER);
    assert!(take_done().is_empty());
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["wifi true"]);
    // Read-only: shown, not changed.
    window.click("locked");
    assert!(take_done().is_empty());
}

#[test]
fn a_checkbox_ticks_and_an_indeterminate_one_ticks_first() {
    let _one = one_window();
    let mut window = Window::open(toggles);
    window.focus("hidden");
    window.key(space());
    assert_eq!(take_done(), ["hidden false"]);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["hidden true"]);
    assert_eq!(window.attr("all", "aria-checked").as_deref(), Some("mixed"));
    assert_eq!(
        window.attr("all", "data-indeterminate").as_deref(),
        Some("true")
    );
    window.click("all");
    assert_eq!(take_done(), ["all true"]);
}

#[test]
fn switches_and_checkboxes_as_a_screen_reader_hears_them() {
    let _one = one_window();
    let window = Window::open(toggles);
    assert_eq!(
        window.heard("wifi"),
        "Switch \"Wi-Fi\" toggled=False [focus click]\n"
    );
    assert_eq!(
        window.heard("hidden"),
        "CheckBox \"Show hidden games\" toggled=True [focus click]\n"
    );
    assert_eq!(
        window.heard("all"),
        "CheckBox \"Select all\" toggled=Mixed [focus click]\n"
    );
}

// ── Menus, selects, dialogs and tooltips ──

fn menu_items() -> Vec<MenuEntry> {
    let mut remove = MenuItem::new("remove", "Remove");
    remove.danger = true;
    let mut hidden = MenuItem::new("hide", "Hide");
    hidden.disabled = true;
    vec![
        MenuEntry::Item(MenuItem::new("play", "Play")),
        MenuEntry::Item(MenuItem::new("favourite", "Favourite")),
        MenuEntry::Item(hidden),
        MenuEntry::Separator,
        MenuEntry::Section {
            title: Some("Manage".into()),
            items: vec![MenuItem::new("edit", "Edit details"), remove],
        },
    ]
}

#[component]
fn Overlays() -> Element {
    let mut sort = use_signal(|| Some("title".to_owned()));
    let mut asking = use_signal(|| false);
    rsx! {
        InputRoot {
            MenuButton { id: "more", label: "More", items: menu_items(), on_action: |id| did(format!("menu {id}")) }
            Select {
                id: "sort",
                label: "Sort by",
                options: vec![
                    SelectOption::new("title", "Title"),
                    SelectOption::new("played", "Last played"),
                    SelectOption::new("added", "Recently added"),
                ],
                value: sort(),
                on_change: move |value: String| { did(format!("sort {value}")); sort.set(Some(value)) },
            }
            Button { id: "ask", on_press: move |_| asking.set(true), "Remove game" }
            Tooltip { text: "Opens the store page",
                Button { id: "store", "Store" }
            }
            if asking() {
                ConfirmDialog {
                    title: "Remove Hollow Pines?",
                    confirm_label: "Remove",
                    on_confirm: move |_| { did("removed"); asking.set(false) },
                    on_close: move |_| { did("kept"); asking.set(false) },
                    p { "Its saves stay." }
                }
            }
        }
    }
}

fn overlays() -> Element {
    rsx! { Overlays {} }
}

impl Window {
    /// The id of the element with focus, or its text.
    fn focus_text(&self) -> String {
        let doc = self.doc.inner.borrow();
        doc.get_focussed_node_id()
            .and_then(|node| doc.get_node(node))
            .map(|node| node.text_content().trim().to_owned())
            .unwrap_or_default()
    }

    fn exists(&self, role: &str) -> bool {
        let doc = self.doc.inner.borrow();
        let mut found = false;
        doc.visit(|_, node| {
            found |= node.attr(LocalName::from("role")) == Some(role);
        });
        found
    }
}

#[test]
fn a_menu_opens_moves_types_ahead_runs_and_gives_focus_back() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    window.focus("more");
    window.key(ENTER);
    assert!(window.exists("menu"));
    assert_eq!(
        window.attr("more", "aria-expanded").as_deref(),
        Some("true")
    );
    assert_eq!(window.focus_text(), "Play");
    window.key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(window.focus_text(), "Favourite");
    // Disabled items are skipped.
    window.key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(window.focus_text(), "Edit details");
    window.key(Key::Named(NamedKey::End));
    assert_eq!(window.focus_text(), "Remove");
    window.key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(window.focus_text(), "Play");
    window.key(Key::Character("e".into()));
    assert_eq!(window.focus_text(), "Edit details");
    window.key(ENTER);
    assert_eq!(take_done(), ["menu edit"]);
    assert!(!window.exists("menu"));
    assert_eq!(window.focus_text(), "More");
    // Escape, and a pad's Back, close it without choosing.
    window.key(Key::Named(NamedKey::ArrowDown));
    assert!(window.exists("menu"));
    window.key(Key::Named(NamedKey::Escape));
    assert!(!window.exists("menu"));
    assert_eq!(window.focus_text(), "More");
    window.pad(Action::Confirm);
    assert!(window.exists("menu"));
    window.pad(Action::NavDown);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["menu favourite"]);
    window.click("more");
    window.pad(Action::Back);
    assert!(!window.exists("menu"));
    assert!(take_done().is_empty());
}

#[test]
fn a_menu_as_a_screen_reader_hears_it() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    assert_eq!(
        window.heard("more"),
        "Button \"More\" expanded=false popup=Menu [focus click expand]\n"
    );
    window.click("more");
    let doc = window.doc.inner.borrow();
    let tree = crate::a11y_snapshot::snapshot(&doc);
    assert!(
        tree.contains(
            "Menu \"More\"\n  MenuItem \"Play\" [click]\n  MenuItem \"Favourite\" [click]\n  \
             MenuItem \"Hide\" disabled\n  Splitter\n  Group \"Manage\"\n    \
             MenuItem \"Edit details\" [click]\n    MenuItem \"Remove\" [click]\n"
        ),
        "{tree}"
    );
}

#[test]
fn a_select_opens_on_its_choice_and_changes_it() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    let trigger = {
        let doc = window.doc.inner.borrow();
        let select = window.id("sort");
        doc.get_node(select).unwrap().children[1]
    };
    window.doc.inner.borrow_mut().set_focus_to(trigger);
    window.key(space());
    assert!(window.exists("listbox"));
    assert_eq!(window.focus_text(), "Title");
    window.key(Key::Named(NamedKey::ArrowDown));
    window.key(ENTER);
    assert_eq!(take_done(), ["sort played"]);
    assert!(!window.exists("listbox"));
    assert!(window.focus_text().starts_with("Last played"));
    // Opened again, focus is on the new choice; Escape keeps it.
    window.pad(Action::Confirm);
    assert_eq!(window.focus_text(), "Last played");
    window.key(Key::Named(NamedKey::Escape));
    assert!(take_done().is_empty());
}

#[test]
fn a_confirm_dialog_starts_on_cancel_and_closes_on_escape_or_back() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    window.click("ask");
    assert!(window.exists("alertdialog"));
    assert_eq!(window.focus_text(), "Cancel");
    window.key(ENTER);
    assert_eq!(take_done(), ["kept"]);
    assert!(!window.exists("alertdialog"));
    assert_eq!(window.focus_text(), "Remove game");
    window.key(ENTER);
    window.key(Key::Named(NamedKey::Escape));
    assert_eq!(take_done(), ["kept"]);
    window.pad(Action::Confirm);
    window.pad(Action::Back);
    assert_eq!(take_done(), ["kept"]);
    window.pad(Action::Confirm);
    window.pad(Action::NavRight);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["removed"]);
}

#[test]
fn a_dialog_as_a_screen_reader_hears_it() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    window.click("ask");
    let doc = window.doc.inner.borrow();
    let tree = crate::a11y_snapshot::snapshot(&doc);
    assert!(
        tree.contains("AlertDialog \"Remove Hollow Pines?\" modal\n"),
        "{tree}"
    );
}

#[test]
fn a_tooltip_shows_on_focus_and_describes_its_trigger() {
    let _one = one_window();
    let mut window = Window::open(overlays);
    assert!(!window.exists("tooltip"));
    // Focus on its trigger (the input layer's, as a click or a pad puts it).
    window.click("store");
    assert!(window.exists("tooltip"));
    assert!(window.heard("store").contains("Button \"Store\""));
    window.key(Key::Named(NamedKey::Escape));
    assert!(!window.exists("tooltip"));
    // Hovering shows it after a moment.
    window.hover("ask");
    window.hover("store");
    std::thread::sleep(Duration::from_millis(800));
    window.settle();
    assert!(window.exists("tooltip"));
}

// ── Choices, values, tabs, toasts and notices ──

#[component]
fn Forms() -> Element {
    let mut view = use_signal(|| "grid".to_owned());
    let mut size = use_signal(|| Some("md".to_owned()));
    let mut volume = use_signal(|| 40.0);
    let mut tab = use_signal(|| "overview".to_owned());
    rsx! {
        InputRoot {
            ToastRegion { on_action: |id| did(format!("toast {id}")),
                SegmentedControl {
                    id: "view",
                    "aria-label": "View",
                    options: vec![Segment::new("grid", "Grid"), Segment::new("list", "List")],
                    value: view(),
                    on_change: move |value: String| { did(format!("view {value}")); view.set(value) },
                }
                RadioGroup {
                    id: "size",
                    label: "Card size",
                    options: vec![
                        RadioOption::new("sm", "Small"),
                        RadioOption::new("md", "Medium"),
                        RadioOption::new("lg", "Large"),
                    ],
                    value: size(),
                    on_change: move |value: String| { did(format!("size {value}")); size.set(Some(value)) },
                }
                Slider {
                    id: "volume",
                    label: "Volume",
                    value: volume(),
                    step: 10.0,
                    format: move |value: f64| format!("{value}%"),
                    on_change: move |value: f64| { did(format!("volume {value}")); volume.set(value) },
                }
                Progress { id: "copying", label: "Copying Celeste", value: Some(0.25) }
                Tabs {
                    label: "Game",
                    tabs: vec![Tab::new("overview", "Overview"), Tab::new("saves", "Saves"), Tab::new("mods", "Mods")],
                    selected: tab(),
                    on_change: move |value: String| { did(format!("tab {value}")); tab.set(value) },
                    p { "{tab}" }
                }
                StatusBadge { id: "badge", status: GameStatus::Available }
                ErrorNotice {
                    id: "failure",
                    title: "Couldn't reach RomM",
                    summary: "The server didn't answer.",
                    hints: vec!["Check that RomM is running.".to_owned()],
                    detail: "connect timed out",
                    reference: "E-42",
                }
                ToastButton {}
            }
        }
    }
}

#[component]
fn ToastButton() -> Element {
    let toasts = use_toasts();
    rsx! {
        Button {
            id: "toast",
            on_press: move |_| {
                toasts.show("Installed Celeste", false, Some(("Play".to_owned(), "play".to_owned())));
            },
            "Notify"
        }
    }
}

fn forms() -> Element {
    rsx! { Forms {} }
}

impl Window {
    fn child(&self, id: &str, index: usize) -> NodeId {
        let doc = self.doc.inner.borrow();
        let node = doc.get_node(self.id(id)).unwrap();
        node.children
            .iter()
            .copied()
            .filter(|child| doc.get_node(*child).is_some_and(|node| node.is_element()))
            .nth(index)
            .unwrap()
    }

    fn focus_node(&mut self, node: NodeId) {
        self.doc.inner.borrow_mut().set_focus_to(node);
        self.settle();
    }

    fn tree(&self) -> String {
        crate::a11y_snapshot::snapshot(&self.doc.inner.borrow())
    }
}

#[test]
fn a_segmented_control_picks_by_mouse_keys_and_pad() {
    let _one = one_window();
    let mut window = Window::open(forms);
    let list = window.child("view", 1);
    window.focus_node(list);
    window.key(space());
    assert_eq!(take_done(), ["view list"]);
    window.pad(Action::NavLeft);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["view grid"]);
    let tree = window.heard("view");
    assert!(
        tree.contains("Button \"Grid\" toggled=True")
            && tree.contains("Button \"List\" toggled=False"),
        "{tree}"
    );
}

#[test]
fn a_radio_group_moves_and_checks_with_arrows_and_is_one_stop() {
    let _one = one_window();
    let mut window = Window::open(forms);
    assert_eq!(window.attr("size", "role").as_deref(), Some("radiogroup"));
    let medium = window.child("size", 2);
    let tabindex = |window: &Window, node: NodeId| {
        let doc = window.doc.inner.borrow();
        doc.get_node(node)
            .and_then(|n| n.attr(LocalName::from("tabindex")))
            .map(str::to_owned)
    };
    assert_eq!(tabindex(&window, medium).as_deref(), Some("0"));
    assert_eq!(
        tabindex(&window, window.child("size", 1)).as_deref(),
        Some("-1")
    );
    window.focus_node(medium);
    window.key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(take_done(), ["size lg"]);
    assert_eq!(window.focus_text(), "Large");
    window.key(Key::Named(NamedKey::ArrowDown));
    assert_eq!(take_done(), ["size sm"]);
    let tree = window.heard("size");
    assert!(
        tree.contains("RadioGroup \"Card size\"")
            && tree.contains("RadioButton \"Small\" toggled=True"),
        "{tree}"
    );
}

#[test]
fn a_slider_steps_by_keys_and_a_pad_and_says_its_value() {
    let _one = one_window();
    let mut window = Window::open(forms);
    let thumb = window.id("volume");
    window.focus_node(thumb);
    window.key(Key::Named(NamedKey::ArrowRight));
    assert_eq!(take_done(), ["volume 50"]);
    window.key(Key::Named(NamedKey::End));
    assert_eq!(take_done(), ["volume 100"]);
    window.key(Key::Named(NamedKey::Home));
    assert_eq!(take_done(), ["volume 0"]);
    window.pad(Action::NavRight);
    assert_eq!(take_done(), ["volume 10"]);
    assert_eq!(
        window.heard("volume"),
        "Slider \"Volume\" orientation=Horizontal value=\"10%\" now=10.0 min=0.0 max=100.0 [focus click step]\n"
    );
}

#[test]
fn progress_tabs_badges_and_notices_as_a_screen_reader_hears_them() {
    let _one = one_window();
    let window = Window::open(forms);
    assert_eq!(
        window.heard("copying").lines().next(),
        Some("ProgressIndicator \"Copying Celeste\" now=25.0 min=0.0 max=100.0")
    );
    let tree = window.tree();
    assert!(
        tree.contains("TabList \"Game\" orientation=Horizontal\n"),
        "{tree}"
    );
    assert!(tree.contains("Tab \"Overview\" selected=true"), "{tree}");
    assert!(tree.contains("TabPanel \"Overview\""), "{tree}");
    assert_eq!(
        window.attr("badge", "data-status").as_deref(),
        Some("available")
    );
    assert!(window.heard("badge").contains("Available to Buy"));
    let notice = window.heard("failure");
    assert!(notice.starts_with("Alert"), "{notice}");
    assert!(
        notice.contains("Button \"Show details\" expanded=false"),
        "{notice}"
    );
}

#[test]
fn tabs_move_with_arrows_and_a_pads_bumpers() {
    let _one = one_window();
    let mut window = Window::open(forms);
    let doc_tab = {
        let doc = window.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if found.is_none() && node.attr(LocalName::from("role")) == Some("tab") {
                found = Some(id);
            }
        });
        found.unwrap()
    };
    window.focus_node(doc_tab);
    window.key(Key::Named(NamedKey::ArrowRight));
    assert_eq!(take_done(), ["tab saves"]);
    assert_eq!(window.focus_text(), "Saves");
    window.pad(Action::TabNext);
    assert_eq!(take_done(), ["tab mods"]);
    window.key(Key::Named(NamedKey::Home));
    assert_eq!(take_done(), ["tab overview"]);
}

#[test]
fn an_error_notice_shows_its_details_and_a_toast_comes_and_goes() {
    let _one = one_window();
    let mut window = Window::open(forms);
    assert!(!window.tree().contains("E-42"));
    window.click("failure");
    let toggle = {
        let doc = window.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.attr(LocalName::from("data-part")) == Some("details-toggle") {
                found = Some(id);
            }
        });
        found.unwrap()
    };
    window.focus_node(toggle);
    window.key(ENTER);
    assert_eq!(
        window.attr("failure", "data-expanded").as_deref(),
        Some("true")
    );
    assert!(window.tree().contains("Reference: E-42"));
    window.click("toast");
    let tree = window.tree();
    assert!(
        tree.contains("Status live=Polite") && tree.contains("Installed Celeste"),
        "{tree}"
    );
    let play = {
        let doc = window.doc.inner.borrow();
        let mut found = None;
        doc.visit(|id, node| {
            if node.is_element()
                && node.text_content().trim() == "Play"
                && node.attr(LocalName::from("data-cs")) == Some("button")
            {
                found = Some(id);
            }
        });
        found.unwrap()
    };
    window.focus_node(play);
    window.key(ENTER);
    assert_eq!(take_done(), ["toast play"]);
    assert!(!window.tree().contains("Installed Celeste"));
}

// ── Game components ──

fn card_game(id: &str, title: &str) -> CardGame {
    CardGame {
        id: id.to_owned(),
        title: title.to_owned(),
        subtitle: Some("SNES".to_owned()),
        status: GameStatus::Installed,
        favorite: true,
        playtime_ms: 7_200_000,
        ..CardGame::default()
    }
}

#[component]
fn Games() -> Element {
    rsx! {
        InputRoot {
            GameCard {
                id: "card",
                game: card_game("g1", "Hollow Pines"),
                focus_key: "game-g1",
                "data-index": "0",
                "data-focus-default": "true",
                on_open: |_| did("open card"),
                on_focus: |_| did("focus card"),
            }
            GameCard {
                id: "store",
                game: CardGame { stores: Some(2), size_bytes: Some(4_200_000_000), status: GameStatus::Available, ..card_game("g2", "Tidewater") },
                variant: CardVariant::Store,
            }
            GameCard { id: "chosen", game: card_game("g3", "Moss & Iron"), selected: Some(true) }
            GameRow { id: "row", game: card_game("g4", "Glasswing"), on_open: |_| did("open row") }
        }
    }
}

fn games() -> Element {
    rsx! { Games {} }
}

#[test]
fn a_game_card_opens_by_mouse_enter_and_a_pad_and_reports_focus() {
    let _one = one_window();
    let mut window = Window::open(games);
    assert_eq!(window.attr("card", "data-index").as_deref(), Some("0"));
    assert_eq!(
        window.attr("card", "data-focus-key").as_deref(),
        Some("game-g1")
    );
    assert_eq!(window.attr("card", "data-cs").as_deref(), Some("game-card"));
    assert_eq!(
        window.attr("card", "data-variant").as_deref(),
        Some("library")
    );
    // The input layer puts focus on a clicked control after the click.
    window.click("card");
    assert_eq!(take_done(), ["open card", "focus card"]);
    window.key(ENTER);
    assert_eq!(take_done(), ["open card"]);
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["open card"]);
    window.click("row");
    assert_eq!(take_done(), ["open row"]);
}

#[test]
fn game_cards_and_rows_as_a_screen_reader_hears_them() {
    let _one = one_window();
    let window = Window::open(games);
    let card = window.heard("card");
    assert!(
        card.starts_with("Link \"Hollow Pines\" [focus click]"),
        "{card}"
    );
    assert!(card.contains("Image \"Favorite\""), "{card}");
    assert_eq!(
        window.attr("chosen", "data-selected").as_deref(),
        Some("true")
    );
    assert_eq!(
        window.attr("chosen", "aria-pressed").as_deref(),
        Some("true")
    );
    let store = window.heard("store");
    assert!(
        store.contains("Available to Buy") && store.contains("2 stores"),
        "{store}"
    );
    let row = window.heard("row");
    assert!(row.starts_with("Link \"Glasswing\""), "{row}");
}
