//! The on-screen keyboard's model: how a pad types. Ported from the
//! webview's `input/OnScreenKeyboard.tsx`; the native app draws it.
//!
//! It opens on a text field (A on the field, Y for the search, a tap in
//! console mode) and types into it while focus stays there. The d-pad moves
//! between keys, A types, B closes, X deletes, Y is a space, LB/RB switch
//! between letters and symbols, LT/RT move the cursor, Start is Done.

use crate::Action;
use crate::tree::FocusTree;

/// What a key does besides typing itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    Shift,
    Layer,
    Backspace,
    Space,
    Paste,
    Left,
    Right,
    Done,
}

impl Special {
    /// Its `data-key` name, as the webview's.
    pub fn name(self) -> &'static str {
        match self {
            Self::Shift => "shift",
            Self::Layer => "layer",
            Self::Backspace => "backspace",
            Self::Space => "space",
            Self::Paste => "paste",
            Self::Left => "left",
            Self::Right => "right",
            Self::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Char(char),
    Special(Special),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub kind: KeyKind,
    /// In key widths.
    pub width: u8,
}

impl Key {
    const fn special(special: Special, width: u8) -> Self {
        Self {
            kind: KeyKind::Special(special),
            width,
        }
    }

    /// Its `data-key` name: the character, or what it does.
    pub fn name(&self) -> String {
        match self.kind {
            KeyKind::Char(c) => c.to_string(),
            KeyKind::Special(special) => special.name().to_owned(),
        }
    }
}

fn chars(row: &str) -> Vec<Key> {
    row.chars()
        .map(|c| Key {
            kind: KeyKind::Char(c),
            width: 1,
        })
        .collect()
}

fn bottom() -> Vec<Key> {
    vec![
        Key::special(Special::Layer, 2),
        Key::special(Special::Left, 1),
        Key::special(Special::Space, 4),
        Key::special(Special::Paste, 2),
        Key::special(Special::Right, 1),
        Key::special(Special::Done, 2),
    ]
}

/// A language's letter rows (twelve, twelve and ten keys, beside shift) and
/// `more`: its other letters and the punctuation its letters pushed out, as
/// a row of the symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub letters: [&'static str; 3],
    pub more: Option<&'static str>,
}

const ENGLISH: Layout = Layout {
    letters: ["qwertyuiop-_", "asdfghjkl:./", "zxcvbnm,@'"],
    more: None,
};

/// A layout per language, as its people's keyboards are, with the letters
/// the language needs within a pad's reach. Others get English's.
pub const LAYOUTS: [(&str, Layout); 4] = [
    ("en", ENGLISH),
    // AZERTY, with é è à ç on the letters and the other accents on the
    // symbols.
    (
        "fr",
        Layout {
            letters: ["azertyuiopéè", "qsdfghjklmàç", "wxcvbn,.-'"],
            more: Some("âêîôûëïüùœ:_"),
        },
    ),
    // QWERTZ, with ü ö ä ß.
    (
        "de",
        Layout {
            letters: ["qwertzuiopü-", "asdfghjklöä.", "yxcvbnm,ß'"],
            more: Some(":_"),
        },
    ),
    // QWERTY, with å ä ö where a Swedish keyboard has them.
    (
        "sv",
        Layout {
            letters: ["qwertyuiopå-", "asdfghjklöä.", "zxcvbnm,@'"],
            more: Some(":_éü"),
        },
    ),
];

/// The layout for `locale` (`fr-CA` is French's), else English's.
pub fn layout_for(locale: &str) -> Layout {
    let language = locale.to_lowercase();
    let language = language.split(['-', '_']).next().unwrap_or("");
    LAYOUTS
        .iter()
        .find(|(name, _)| *name == language)
        .map_or(ENGLISH, |(_, layout)| *layout)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Letters,
    Symbols,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Off,
    Once,
    Lock,
}

/// Every row is twelve keys wide, so a column lines up from row to row.
fn rows_of(layout: Layout, layer: Layer) -> Vec<Vec<Key>> {
    let mut digits = chars("1234567890");
    digits.push(Key::special(Special::Backspace, 2));
    match layer {
        Layer::Letters => {
            let [top, middle, last] = layout.letters;
            let mut shifted = vec![Key::special(Special::Shift, 2)];
            shifted.extend(chars(last));
            vec![digits, chars(top), chars(middle), shifted, bottom()]
        }
        Layer::Symbols => {
            let mut symbols = chars("!@#$%^&*()");
            symbols.push(Key::special(Special::Backspace, 2));
            let mut rows = vec![symbols, chars("~`=+[]{}|\\<>"), chars(";\"'?,./€£¥°§")];
            if let Some(more) = layout.more {
                rows.push(chars(more));
            }
            rows.push(bottom());
            rows
        }
    }
}

/// The key a column position falls on in `row`, by the keys' widths.
fn key_at(row: &[Key], x: f32) -> usize {
    let mut end = 0.0;
    for (index, key) in row.iter().enumerate() {
        end += f32::from(key.width);
        if x < end {
            return index;
        }
    }
    row.len().saturating_sub(1)
}

/// Where the middle of key `index` is along its row.
fn middle(row: &[Key], index: usize) -> f32 {
    let start: f32 = row.iter().take(index).map(|key| f32::from(key.width)).sum();
    start + row.get(index).map_or(1.0, |key| f32::from(key.width)) / 2.0
}

/// The field being typed into: its text and selection, in characters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Field {
    pub value: String,
    pub start: usize,
    pub end: usize,
}

impl Field {
    /// `value` with the cursor at its end.
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let end = value.chars().count();
        Self {
            value,
            start: end,
            end,
        }
    }

    fn len(&self) -> usize {
        self.value.chars().count()
    }

    fn byte(&self, at: usize) -> usize {
        self.value
            .char_indices()
            .nth(at)
            .map_or(self.value.len(), |(index, _)| index)
    }

    fn write(&mut self, from: usize, to: usize, text: &str) {
        let (from_byte, to_byte) = (self.byte(from), self.byte(to));
        self.value.replace_range(from_byte..to_byte, text);
        let caret = from + text.chars().count();
        self.start = caret;
        self.end = caret;
    }

    /// Types `text` over the selection.
    pub fn insert(&mut self, text: &str) {
        let (start, end) = self.selection();
        self.write(start, end, text);
    }

    /// Deletes the selection, or the character before the cursor.
    pub fn delete_back(&mut self) {
        let (start, end) = self.selection();
        let from = if start == end {
            start.saturating_sub(1)
        } else {
            start
        };
        self.write(from, end, "");
    }

    /// Moves the cursor `by` characters, within the text.
    pub fn move_caret(&mut self, by: isize) {
        let at = self.start.saturating_add_signed(by).min(self.len());
        self.start = at;
        self.end = at;
    }

    fn selection(&self) -> (usize, usize) {
        let len = self.len();
        let (start, end) = (self.start.min(len), self.end.min(len));
        (start.min(end), start.max(end))
    }
}

/// What a key press asks of the field's owner beyond editing the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The keyboard changed or the text did: draw it again.
    Changed,
    /// Paste what the clipboard holds ([`pasted`]).
    Paste,
    /// Close the keyboard, keeping the text.
    Close,
    /// Close the keyboard and press Enter on the field.
    Submit,
}

/// The clipboard's text as `multiline` or one-line field takes it: one line
/// has no line breaks at either end, and a space for those within.
pub fn pasted(text: &str, multiline: bool) -> String {
    if multiline {
        return text.to_owned();
    }
    let breaks = |c: char| c == '\r' || c == '\n';
    let trimmed = text.trim_matches(breaks);
    let mut out = String::with_capacity(trimmed.len());
    let mut rest = trimmed;
    while let Some(c) = rest.chars().next() {
        if c.is_whitespace() {
            // A run of spaces with a line break in it becomes one space.
            let run = rest
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(rest.len());
            let (spaces, after) = rest.split_at(run);
            out.push_str(if spaces.contains(breaks) { " " } else { spaces });
            rest = after;
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// The keyboard's state: its layout, layer, shift and the key the pad is on.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyboard {
    layout: Layout,
    pub layer: Layer,
    pub shift: Shift,
    /// The row and column of the key the pad is on.
    pub at: (usize, usize),
}

impl Keyboard {
    pub fn new(locale: &str) -> Self {
        Self {
            layout: layout_for(locale),
            layer: Layer::Letters,
            shift: Shift::Off,
            at: (1, 0),
        }
    }

    pub fn rows(&self) -> Vec<Vec<Key>> {
        rows_of(self.layout, self.layer)
    }

    /// Whether letters type as capitals now.
    pub fn upper(&self) -> bool {
        self.layer == Layer::Letters && self.shift != Shift::Off
    }

    /// What `key` shows: letters in the case they type.
    pub fn label(&self, key: &Key) -> Option<String> {
        match key.kind {
            KeyKind::Char(c) if self.upper() => Some(c.to_uppercase().collect()),
            KeyKind::Char(c) => Some(c.to_string()),
            KeyKind::Special(_) => None,
        }
    }

    /// Letters to symbols and back, staying over the same column.
    pub fn switch_layer(&mut self) {
        let rows = self.rows();
        self.layer = match self.layer {
            Layer::Letters => Layer::Symbols,
            Layer::Symbols => Layer::Letters,
        };
        let next = self.rows();
        let (row, col) = self.at;
        let to = row.min(next.len() - 1);
        let col = rows
            .get(row)
            .map_or(0, |from| key_at(&next[to], middle(from, col)));
        self.at = (to, col);
    }

    /// Moves between keys: across wraps round, up and down keep the column.
    pub fn step(&mut self, action: Action) {
        let rows = self.rows();
        let (row, col) = self.at;
        let keys = &rows[row];
        self.at = match action {
            Action::NavLeft => (row, (col + keys.len() - 1) % keys.len()),
            Action::NavRight => (row, (col + 1) % keys.len()),
            Action::NavUp | Action::NavDown => {
                let next = if action == Action::NavDown {
                    (row + 1).min(rows.len() - 1)
                } else {
                    row.saturating_sub(1)
                };
                (next, key_at(&rows[next], middle(keys, col)))
            }
            _ => return,
        };
    }

    /// The key the pad is on.
    pub fn current(&self) -> Option<Key> {
        let (row, col) = self.at;
        self.rows().get(row)?.get(col).copied()
    }

    /// Presses `key` on `field`.
    pub fn press(&mut self, key: Key, field: &mut Field) -> Outcome {
        match key.kind {
            KeyKind::Special(Special::Shift) => {
                self.shift = match self.shift {
                    Shift::Off => Shift::Once,
                    Shift::Once => Shift::Lock,
                    Shift::Lock => Shift::Off,
                };
            }
            KeyKind::Special(Special::Layer) => self.switch_layer(),
            KeyKind::Special(Special::Backspace) => field.delete_back(),
            KeyKind::Special(Special::Space) => field.insert(" "),
            KeyKind::Special(Special::Paste) => return Outcome::Paste,
            KeyKind::Special(Special::Left) => field.move_caret(-1),
            KeyKind::Special(Special::Right) => field.move_caret(1),
            KeyKind::Special(Special::Done) => return Outcome::Submit,
            KeyKind::Char(c) => {
                if self.upper() {
                    field.insert(&c.to_uppercase().collect::<String>());
                } else {
                    let mut buffer = [0; 4];
                    field.insert(c.encode_utf8(&mut buffer));
                }
                if self.shift == Shift::Once {
                    self.shift = Shift::Off;
                }
            }
        }
        Outcome::Changed
    }

    /// A tap or click on the key at `row`, `col`.
    pub fn tap(&mut self, row: usize, col: usize, field: &mut Field) -> Outcome {
        self.at = (row, col);
        match self.current() {
            Some(key) => self.press(key, field),
            None => Outcome::Changed,
        }
    }

    /// What a pad action does while the keyboard is open; `None` for one it
    /// leaves to the app (the guide button).
    pub fn action(&mut self, action: Action, field: &mut Field) -> Option<Outcome> {
        Some(match action {
            Action::NavUp | Action::NavDown | Action::NavLeft | Action::NavRight => {
                self.step(action);
                Outcome::Changed
            }
            Action::Confirm => match self.current() {
                Some(key) => self.press(key, field),
                None => Outcome::Changed,
            },
            Action::Back => Outcome::Close,
            Action::Details => {
                field.delete_back();
                Outcome::Changed
            }
            Action::Search => {
                field.insert(" ");
                Outcome::Changed
            }
            Action::TabPrev | Action::TabNext => {
                self.switch_layer();
                Outcome::Changed
            }
            Action::PagePrev => {
                field.move_caret(-1);
                Outcome::Changed
            }
            Action::PageNext => {
                field.move_caret(1);
                Outcome::Changed
            }
            Action::Menu => Outcome::Submit,
            Action::View | Action::Guide => return None,
        })
    }
}

/// Whether the keyboard should take `node`: a field that types text.
pub fn types_text<T: FocusTree + ?Sized>(tree: &T, node: T::Node) -> bool {
    let locked = tree.attr(node, "readonly").is_some_and(|v| v != "false")
        || tree.attr(node, "disabled").is_some_and(|v| v != "false");
    match tree.tag(node) {
        "textarea" => !locked,
        "input" => {
            !locked
                && matches!(
                    tree.attr(node, "type").unwrap_or(""),
                    "text" | "search" | "url" | "email" | "password" | "tel" | "number" | ""
                )
        }
        _ => false,
    }
}
