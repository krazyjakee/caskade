use super::{App, Fake};
use crate::Action;
use crate::osk::{Field, Keyboard, Layer, Outcome, Shift};

/// A focused text field in console mode.
fn field_app() -> (App, usize) {
    let mut ui = Fake::default();
    let form = ui.tree.add(0, "form", &[], "");
    let field = ui
        .tree
        .add(form, "input", &[("data-rect", "0 0 300 40")], "");
    let mut app = App::new(ui, true);
    app.focus(field);
    (app, field)
}

#[test]
fn a_on_a_field_opens_the_keyboard_keys_press_enter_without_it() {
    let (mut app, field) = field_app();
    // A key's Enter is the field's own (a native submit), not ours.
    assert!(!app.key("Enter"));
    assert_eq!(app.ui.keyboard, None);
    app.pad(Action::Confirm);
    assert_eq!(app.ui.keyboard, Some(field));
}

#[test]
fn a_tap_in_console_mode_opens_the_keyboard() {
    let (mut app, field) = field_app();
    assert!(app.input.tap(&mut app.ui, field));
    assert_eq!(app.ui.keyboard, Some(field));
    let mut desktop = App::new(Fake::default(), false);
    let field = desktop
        .ui
        .tree
        .add(0, "input", &[("data-rect", "0 0 300 40")], "");
    assert!(!desktop.input.tap(&mut desktop.ui, field));
}

#[test]
fn a_read_only_field_or_a_checkbox_gets_no_keyboard() {
    let mut ui = Fake::default();
    let locked = ui.tree.add(
        0,
        "input",
        &[("data-rect", "0 0 300 40"), ("readonly", "")],
        "",
    );
    let mut app = App::new(ui, true);
    app.focus(locked);
    app.pad(Action::Confirm);
    assert_eq!(app.ui.keyboard, None);
    assert!(!crate::osk::types_text(&app.ui, locked));
}

/// The keyboard over a field, driven by pad actions.
struct Typing {
    keyboard: Keyboard,
    field: Field,
}

impl Typing {
    fn new(locale: &str) -> Self {
        Self {
            keyboard: Keyboard::new(locale),
            field: Field::new(""),
        }
    }

    fn pad(&mut self, action: Action) -> Option<Outcome> {
        self.keyboard.action(action, &mut self.field)
    }

    fn tap_shift(&mut self) {
        let rows = self.keyboard.rows();
        let col = rows[3]
            .iter()
            .position(|key| key.name() == "shift")
            .unwrap();
        self.keyboard.tap(3, col, &mut self.field);
    }

    fn row(&self, row: usize) -> String {
        self.keyboard.rows()[row]
            .iter()
            .map(|key| key.name())
            .collect()
    }
}

#[test]
fn the_pad_types_deletes_spaces_and_moves_the_cursor() {
    let mut typing = Typing::new("en");
    // It starts on q.
    typing.pad(Action::Confirm);
    typing.pad(Action::NavRight);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "qw");
    // Up from w is 2: the columns line up.
    typing.pad(Action::NavUp);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "qw2");
    typing.pad(Action::Details);
    assert_eq!(typing.field.value, "qw");
    typing.pad(Action::Search);
    assert_eq!(typing.field.value, "qw ");
    // RB: symbols, on the same key position (@ above w's column).
    typing.pad(Action::TabNext);
    assert_eq!(typing.keyboard.layer, Layer::Symbols);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "qw @");
    // LT moves the cursor back; typing goes in there.
    typing.pad(Action::PagePrev);
    typing.pad(Action::PagePrev);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "qw@ @");
}

#[test]
fn shift_types_one_capital_twice_locks_and_start_submits() {
    let mut typing = Typing::new("en");
    typing.tap_shift();
    assert_eq!(typing.keyboard.shift, Shift::Once);
    // Up from shift to s, then up again to w.
    typing.pad(Action::NavUp);
    typing.pad(Action::NavUp);
    // Once in capitals, then lower case again.
    typing.pad(Action::Confirm);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "Ww");
    assert_eq!(typing.keyboard.shift, Shift::Off);
    typing.tap_shift();
    typing.tap_shift();
    assert_eq!(typing.keyboard.shift, Shift::Lock);
    // The tap moved the pad's place to shift; back up to w.
    typing.pad(Action::NavUp);
    typing.pad(Action::NavUp);
    typing.pad(Action::Confirm);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "WwWW");
    assert_eq!(typing.pad(Action::Menu), Some(Outcome::Submit));
}

#[test]
fn b_closes_the_keyboard_and_keeps_the_text() {
    let mut typing = Typing::new("en");
    typing.pad(Action::Confirm);
    assert_eq!(typing.pad(Action::Back), Some(Outcome::Close));
    assert_eq!(typing.field.value, "q");
    // The guide button is the app's, not the keyboard's.
    assert_eq!(typing.pad(Action::Guide), None);
}

#[test]
fn french_is_azerty_and_a_pad_types_its_accents() {
    let mut typing = Typing::new("fr");
    assert_eq!(typing.row(1), "azertyuiopéè");
    assert_eq!(typing.row(2), "qsdfghjklmàç");
    // It starts on a; left wraps round to è, then é.
    typing.pad(Action::Confirm);
    typing.pad(Action::NavLeft);
    typing.pad(Action::Confirm);
    typing.pad(Action::NavLeft);
    typing.pad(Action::Confirm);
    // Down from é is à, and ç is beside it.
    typing.pad(Action::NavDown);
    typing.pad(Action::Confirm);
    typing.pad(Action::NavRight);
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "aèéàç");
    // Shift gives the capital: from shift up to s, z, then left to é.
    typing.tap_shift();
    typing.pad(Action::NavUp);
    typing.pad(Action::NavUp);
    for _ in 0..3 {
        typing.pad(Action::NavLeft);
    }
    typing.pad(Action::Confirm);
    assert_eq!(typing.field.value, "aèéàçÉ");
    // The other accents are a row of the symbols.
    typing.pad(Action::TabNext);
    assert_eq!(typing.row(3), "âêîôûëïüùœ:_");
}

#[test]
fn german_is_qwertz_and_swedish_has_its_letters() {
    let german = Typing::new("de-AT");
    assert_eq!(german.row(1), "qwertzuiopü-");
    assert_eq!(german.row(2), "asdfghjklöä.");
    assert_eq!(german.row(3), "shiftyxcvbnm,ß'");
    let mut swedish = Typing::new("sv");
    assert_eq!(swedish.row(1), "qwertyuiopå-");
    // From q, left twice wraps round to å; down to ä, left to ö.
    swedish.pad(Action::NavLeft);
    swedish.pad(Action::NavLeft);
    swedish.pad(Action::Confirm);
    swedish.pad(Action::NavDown);
    swedish.pad(Action::Confirm);
    swedish.pad(Action::NavLeft);
    swedish.pad(Action::Confirm);
    assert_eq!(swedish.field.value, "åäö");
}

#[test]
fn a_language_without_a_layout_gets_english() {
    assert_eq!(Typing::new("ja").row(1), "qwertyuiop-_");
}

#[test]
fn paste_makes_one_line_for_a_one_line_field() {
    assert_eq!(crate::osk::pasted("\nhttps://a\n", false), "https://a");
    assert_eq!(crate::osk::pasted("a  \r\n  b\nc", false), "a b c");
    assert_eq!(crate::osk::pasted("a  b", false), "a  b");
    assert_eq!(crate::osk::pasted("a\nb", true), "a\nb");
    let mut field = Field::new("ab");
    field.move_caret(-1);
    field.insert(&crate::osk::pasted("x\ny", false));
    assert_eq!((field.value.as_str(), field.start), ("ax yb", 4));
}
