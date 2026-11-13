//! Choices, values, tabs, toasts and notices.

use super::*;

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
    // A pad moves through the group spatially, and its A checks.
    window.pad(Action::NavDown);
    assert_eq!(window.focus_text(), "Medium");
    assert_eq!(take_done(), Vec::<String>::new());
    window.pad(Action::Confirm);
    assert_eq!(take_done(), ["size md"]);
    window.key(Key::Named(NamedKey::ArrowUp));
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
    // A pad's A hides them again, and shows them.
    window.pad(Action::Confirm);
    assert_eq!(
        window.attr("failure", "data-expanded").as_deref(),
        Some("false")
    );
    window.pad(Action::Confirm);
    assert_eq!(
        window.attr("failure", "data-expanded").as_deref(),
        Some("true")
    );
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
