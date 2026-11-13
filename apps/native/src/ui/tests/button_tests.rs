//! Buttons and links.

use super::*;

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
