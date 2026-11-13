//! Switches and checkboxes.

use super::*;

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
