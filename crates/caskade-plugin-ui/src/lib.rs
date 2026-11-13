//! The UI plugins draw (#109). From plugin API 0.2 a plugin's UI
//! contributions are trees of Caskade's own components, which its backend
//! returns as JSON from the `ui` export (`render`, `act`). Caskade draws
//! them with its components: RSX in the native app, `@caskade/ui` in the
//! webview until the cutover. The theme, focus, pad input, console mode and
//! screen readers come with them, and plugin code runs only in its sandbox.
//!
//! A tree is one [`Node`] or a list of them, e.g.
//! `[{"type": "heading", "text": "Most played"}, {"type": "button",
//! "label": "Refresh", "action": "refresh"}]`. [`parse`] checks one before
//! anything is drawn: its size, depth and node count, and every action id.
//! Buttons and segmented controls name an action; using one calls the
//! backend's `act` with it, and the contribution is drawn again. The
//! builders ([`heading`], [`button`], …) are the Rust SDK's way to write
//! them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The largest tree accepted, as JSON.
pub const MAX_BYTES: usize = 256 * 1024;
/// The deepest nesting accepted.
pub const MAX_DEPTH: usize = 12;
/// The most nodes in one tree.
pub const MAX_NODES: usize = 2000;
/// The longest text in one node, in characters.
pub const MAX_TEXT: usize = 4000;
/// The longest action id.
pub const MAX_ACTION: usize = 64;

/// One of Caskade's components, as a plugin asks for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Node {
    /// Plain text, inline.
    Text { text: String },
    /// A heading: level 2 (the default) or 3.
    Heading {
        text: String,
        #[serde(default = "two")]
        level: u8,
    },
    /// A paragraph of inline text.
    Paragraph { text: String },
    /// Secondary text, muted.
    Note { text: String },
    /// Something that went wrong, announced (`role="alert"`).
    Alert { text: String },
    /// Children in a row or a column.
    Stack {
        #[serde(default)]
        direction: Direction,
        #[serde(default)]
        gap: Gap,
        children: Vec<Node>,
    },
    /// A labelled part of the contribution, which pad focus moves between.
    Section { label: String, children: Vec<Node> },
    /// Labelled values (a description list).
    Stats {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        items: Vec<Stat>,
    },
    /// One large value with what it is.
    Tile { value: String, label: String },
    /// A bar per item, sized by `value` against the largest, with `text`
    /// beside it; the values are text too, for screen readers.
    Bars { label: String, items: Vec<Bar> },
    /// A table; `rowHeaders` makes each row's first cell its header.
    Table {
        caption: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default)]
        row_headers: bool,
    },
    /// A list of nodes.
    List {
        #[serde(default)]
        ordered: bool,
        items: Vec<Node>,
    },
    /// A short status label.
    Badge {
        text: String,
        #[serde(default)]
        tone: Tone,
    },
    /// How far something is: `value` of `max`.
    Progress { label: String, value: f64, max: f64 },
    /// A button that calls `act` with `action` and `value`.
    Button {
        label: String,
        action: String,
        #[serde(default, skip_serializing_if = "Value::is_null")]
        #[cfg_attr(feature = "specta", specta(type = Option<serde_json::Value>))]
        value: Value,
        #[serde(default)]
        variant: Variant,
    },
    /// One of a few options; choosing one calls `act` with `action` and the
    /// option's `value`. In console mode LB and RB move between options.
    Segmented {
        label: String,
        action: String,
        value: String,
        options: Vec<Choice>,
    },
}

fn two() -> u8 {
    2
}

/// A [`Node::Stats`] row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct Stat {
    pub label: String,
    pub value: String,
}

/// A [`Node::Bars`] bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct Bar {
    pub label: String,
    pub value: f64,
    pub text: String,
}

/// A [`Node::Segmented`] option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Column,
    Row,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Gap {
    None,
    Small,
    #[default]
    Medium,
    Large,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Variant {
    #[default]
    Secondary,
    Primary,
}

/// Where a contribution is drawn, as `render` and `act` get it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Context {
    /// `desktop` or `console`.
    #[serde(default)]
    pub mode: Mode,
    /// The game, in a game slot (`game.details.tabs`, `game.actions`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_id: Option<String>,
    /// That game's title, which the host fills in: the plugin needs no
    /// library access to name the game it's shown on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_title: Option<String>,
    /// What the plugin's last `act` returned as `state`, `null` before.
    #[serde(default)]
    #[cfg_attr(feature = "specta", specta(type = serde_json::Value))]
    pub state: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Desktop,
    Console,
}

/// What `act` returns: the contribution's new state (kept by Caskade and
/// passed to the next `render`), and a notice to show.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Outcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "specta", specta(type = Option<serde_json::Value>))]
    pub state: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

/// Why a tree or an outcome was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiError(pub String);

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UiError {}

/// A tree as JSON: one node or a list of them.
#[derive(Deserialize)]
#[serde(untagged)]
enum Tree {
    One(Box<Node>),
    Many(Vec<Node>),
}

/// Checks and reads a tree a plugin returned.
pub fn parse(json: &str) -> Result<Vec<Node>, UiError> {
    if json.len() > MAX_BYTES {
        return Err(UiError(format!(
            "the UI is {} bytes; at most {MAX_BYTES} are drawn",
            json.len()
        )));
    }
    // Nesting is checked on the raw value first: a deep tree must not
    // reach serde's recursion.
    let raw: Value =
        serde_json::from_str(json).map_err(|error| UiError(format!("not JSON: {error}")))?;
    depth(&raw, 0)?;
    let nodes = match Tree::deserialize(&raw) {
        Ok(Tree::One(node)) => vec![*node],
        Ok(Tree::Many(nodes)) => nodes,
        Err(_) => explain(&raw)?,
    };
    let mut count = 0;
    for node in &nodes {
        check(node, &mut count, 1)?;
    }
    Ok(nodes)
}

/// Checks and reads what `act` returned; an empty answer changes nothing.
pub fn parse_outcome(json: &str) -> Result<Outcome, UiError> {
    if json.trim().is_empty() {
        return Ok(Outcome::default());
    }
    if json.len() > MAX_BYTES {
        return Err(UiError(format!("the answer is over {MAX_BYTES} bytes")));
    }
    let outcome: Outcome =
        serde_json::from_str(json).map_err(|error| UiError(format!("not an outcome: {error}")))?;
    if let Some(notice) = &outcome.notice {
        check_text(notice)?;
    }
    Ok(outcome)
}

/// Which node is wrong, when the tree doesn't read.
fn explain(raw: &Value) -> Result<Vec<Node>, UiError> {
    let items: Vec<&Value> = match raw {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    for (index, item) in items.into_iter().enumerate() {
        if let Err(error) = serde_json::from_value::<Node>(item.clone()) {
            let kind = item.get("type").and_then(Value::as_str).unwrap_or("?");
            return Err(UiError(format!("node {index} ({kind}): {error}")));
        }
    }
    Err(UiError("not a node or a list of nodes".into()))
}

fn depth(value: &Value, at: usize) -> Result<(), UiError> {
    if at > MAX_DEPTH * 3 {
        return Err(UiError(format!("nested deeper than {MAX_DEPTH} nodes")));
    }
    match value {
        Value::Array(items) => items.iter().try_for_each(|item| depth(item, at + 1)),
        Value::Object(fields) => fields.values().try_for_each(|item| depth(item, at + 1)),
        _ => Ok(()),
    }
}

fn check_text(text: &str) -> Result<(), UiError> {
    if text.chars().count() > MAX_TEXT {
        return Err(UiError(format!("a text is over {MAX_TEXT} characters")));
    }
    Ok(())
}

fn action(action: &str) -> Result<(), UiError> {
    let valid = !action.is_empty()
        && action.len() <= MAX_ACTION
        && action
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'));
    if valid {
        Ok(())
    } else {
        Err(UiError(format!(
            "action {action:?} isn't 1 to {MAX_ACTION} letters, digits, '-', '_', '.' or ':'"
        )))
    }
}

fn number(value: f64) -> Result<(), UiError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(UiError("a number isn't finite".into()))
    }
}

fn check(node: &Node, count: &mut usize, level: usize) -> Result<(), UiError> {
    *count += 1;
    if *count > MAX_NODES {
        return Err(UiError(format!("more than {MAX_NODES} nodes")));
    }
    if level > MAX_DEPTH {
        return Err(UiError(format!("nested deeper than {MAX_DEPTH} nodes")));
    }
    let children = |children: &[Node], count: &mut usize| {
        children
            .iter()
            .try_for_each(|child| check(child, count, level + 1))
    };
    match node {
        Node::Text { text: t }
        | Node::Paragraph { text: t }
        | Node::Note { text: t }
        | Node::Alert { text: t }
        | Node::Badge { text: t, .. } => check_text(t),
        Node::Heading { text: t, level } => {
            if !matches!(level, 2 | 3) {
                return Err(UiError(format!("heading level {level} isn't 2 or 3")));
            }
            check_text(t)
        }
        Node::Stack { children: c, .. } => children(c, count),
        Node::Section { label, children: c } => {
            check_text(label)?;
            children(c, count)
        }
        Node::List { items, .. } => children(items, count),
        Node::Stats { label, items } => {
            if let Some(label) = label {
                check_text(label)?;
            }
            *count += items.len();
            items.iter().try_for_each(|item| {
                check_text(&item.label)?;
                check_text(&item.value)
            })
        }
        Node::Tile { value, label } => {
            check_text(value)?;
            check_text(label)
        }
        Node::Bars { label, items } => {
            check_text(label)?;
            *count += items.len();
            items.iter().try_for_each(|item| {
                check_text(&item.label)?;
                check_text(&item.text)?;
                number(item.value)?;
                if item.value < 0.0 {
                    return Err(UiError("a bar's value is negative".into()));
                }
                Ok(())
            })
        }
        Node::Table {
            caption,
            columns,
            rows,
            ..
        } => {
            check_text(caption)?;
            *count += columns.len() + rows.iter().map(Vec::len).sum::<usize>();
            if *count > MAX_NODES {
                return Err(UiError(format!("more than {MAX_NODES} nodes")));
            }
            columns.iter().try_for_each(|column| check_text(column))?;
            rows.iter().try_for_each(|row| {
                if row.len() != columns.len() {
                    return Err(UiError(format!(
                        "a table row has {} cells for {} columns",
                        row.len(),
                        columns.len()
                    )));
                }
                row.iter().try_for_each(|cell| check_text(cell))
            })
        }
        Node::Progress { label, value, max } => {
            check_text(label)?;
            number(*value)?;
            number(*max)?;
            if *max <= 0.0 {
                return Err(UiError("a progress's max isn't above 0".into()));
            }
            Ok(())
        }
        Node::Button {
            label,
            action: a,
            value,
            ..
        } => {
            check_text(label)?;
            action(a)?;
            if value.to_string().len() > MAX_TEXT {
                return Err(UiError(format!(
                    "a button's value is over {MAX_TEXT} bytes"
                )));
            }
            Ok(())
        }
        Node::Segmented {
            label,
            action: a,
            value,
            options,
        } => {
            check_text(label)?;
            action(a)?;
            if options.is_empty() || options.len() > 8 {
                return Err(UiError("a segmented control has 1 to 8 options".into()));
            }
            *count += options.len();
            options.iter().try_for_each(|option| {
                check_text(&option.label)?;
                check_text(&option.value)
            })?;
            if !options.iter().any(|option| &option.value == value) {
                return Err(UiError(format!("no option has the value {value:?}")));
            }
            Ok(())
        }
    }
}

/// Builders for trees, for plugins written in Rust.
pub fn text(content: impl Into<String>) -> Node {
    Node::Text {
        text: content.into(),
    }
}

pub fn heading(content: impl Into<String>) -> Node {
    Node::Heading {
        text: content.into(),
        level: 2,
    }
}

pub fn subheading(content: impl Into<String>) -> Node {
    Node::Heading {
        text: content.into(),
        level: 3,
    }
}

pub fn paragraph(content: impl Into<String>) -> Node {
    Node::Paragraph {
        text: content.into(),
    }
}

pub fn note(content: impl Into<String>) -> Node {
    Node::Note {
        text: content.into(),
    }
}

pub fn alert(content: impl Into<String>) -> Node {
    Node::Alert {
        text: content.into(),
    }
}

pub fn column(children: impl IntoIterator<Item = Node>) -> Node {
    Node::Stack {
        direction: Direction::Column,
        gap: Gap::Medium,
        children: children.into_iter().collect(),
    }
}

pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    Node::Stack {
        direction: Direction::Row,
        gap: Gap::Medium,
        children: children.into_iter().collect(),
    }
}

pub fn section(label: impl Into<String>, children: impl IntoIterator<Item = Node>) -> Node {
    Node::Section {
        label: label.into(),
        children: children.into_iter().collect(),
    }
}

/// Labelled values: `stats([("Main story", "12 h")])`.
pub fn stats<L: Into<String>, V: Into<String>>(items: impl IntoIterator<Item = (L, V)>) -> Node {
    Node::Stats {
        label: None,
        items: items
            .into_iter()
            .map(|(label, value)| Stat {
                label: label.into(),
                value: value.into(),
            })
            .collect(),
    }
}

pub fn tile(value: impl Into<String>, label: impl Into<String>) -> Node {
    Node::Tile {
        value: value.into(),
        label: label.into(),
    }
}

pub fn bars(label: impl Into<String>, items: impl IntoIterator<Item = Bar>) -> Node {
    Node::Bars {
        label: label.into(),
        items: items.into_iter().collect(),
    }
}

pub fn bar(label: impl Into<String>, value: f64, text: impl Into<String>) -> Bar {
    Bar {
        label: label.into(),
        value,
        text: text.into(),
    }
}

pub fn table(
    caption: impl Into<String>,
    columns: impl IntoIterator<Item = String>,
    rows: impl IntoIterator<Item = Vec<String>>,
) -> Node {
    Node::Table {
        caption: caption.into(),
        columns: columns.into_iter().collect(),
        rows: rows.into_iter().collect(),
        row_headers: false,
    }
}

pub fn list(items: impl IntoIterator<Item = Node>) -> Node {
    Node::List {
        ordered: false,
        items: items.into_iter().collect(),
    }
}

pub fn badge(content: impl Into<String>, tone: Tone) -> Node {
    Node::Badge {
        text: content.into(),
        tone,
    }
}

pub fn progress(label: impl Into<String>, value: f64, max: f64) -> Node {
    Node::Progress {
        label: label.into(),
        value,
        max,
    }
}

pub fn button(label: impl Into<String>, action: impl Into<String>) -> Node {
    Node::Button {
        label: label.into(),
        action: action.into(),
        value: Value::Null,
        variant: Variant::Secondary,
    }
}

/// `options` are (value, label) pairs; `value` is the one chosen now.
pub fn segmented<V: Into<String>, L: Into<String>>(
    label: impl Into<String>,
    action: impl Into<String>,
    value: impl Into<String>,
    options: impl IntoIterator<Item = (V, L)>,
) -> Node {
    Node::Segmented {
        label: label.into(),
        action: action.into(),
        value: value.into(),
        options: options
            .into_iter()
            .map(|(value, label)| Choice {
                value: value.into(),
                label: label.into(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_tree_reads_as_one_node_or_a_list() {
        let one = parse(r#"{"type": "heading", "text": "Most played"}"#).unwrap();
        assert_eq!(one, [heading("Most played")]);
        let many = parse(
            &json!([
                {"type": "note", "text": "From howlongtobeat.com"},
                {"type": "stats", "items": [{"label": "Main story", "value": "12 h"}]},
                {"type": "button", "label": "Refresh", "action": "refresh", "variant": "primary"},
            ])
            .to_string(),
        )
        .unwrap();
        assert_eq!(many.len(), 3);
        assert_eq!(many[1], stats([("Main story", "12 h")]));
    }

    #[test]
    fn what_the_builders_write_reads_back() {
        let tree = vec![section(
            "Playtime",
            [
                tile("12 h", "across 3 games"),
                segmented(
                    "Show as",
                    "view",
                    "chart",
                    [("chart", "Chart"), ("table", "Table")],
                ),
                bars("Hours played", [bar("Celeste", 12.0, "12 h")]),
                table(
                    "Most played",
                    ["Game".to_owned(), "Hours".to_owned()],
                    [vec!["Celeste".to_owned(), "12.0".to_owned()]],
                ),
                row([badge("New", Tone::Accent), button("Again", "again")]),
                progress("Downloading", 2.0, 8.0),
                list([
                    text("one"),
                    paragraph("two"),
                    alert("three"),
                    subheading("four"),
                ]),
            ],
        )];
        let json = serde_json::to_string(&tree).unwrap();
        assert_eq!(parse(&json).unwrap(), tree);
    }

    #[test]
    fn a_bad_node_says_which_and_why() {
        let error = parse(r#"[{"type": "note", "text": "ok"}, {"type": "marquee"}]"#).unwrap_err();
        assert!(error.0.starts_with("node 1 (marquee):"), "{error}");
        let error = parse(r#"{"type": "note", "text": "x", "colour": "red"}"#).unwrap_err();
        assert!(error.0.contains("colour"), "{error}");
        assert!(parse("<p>hi</p>").unwrap_err().0.starts_with("not JSON"));
    }

    #[test]
    fn limits_hold() {
        let long = "x".repeat(MAX_TEXT + 1);
        assert!(parse(&serde_json::to_string(&note(long)).unwrap()).is_err());
        let mut deep = text("bottom");
        for _ in 0..MAX_DEPTH + 1 {
            deep = column([deep]);
        }
        let error = parse(&serde_json::to_string(&deep).unwrap()).unwrap_err();
        assert!(error.0.contains("deeper"), "{error}");
        let wide: Vec<Node> = (0..=MAX_NODES).map(|i| text(i.to_string())).collect();
        let error = parse(&serde_json::to_string(&wide).unwrap()).unwrap_err();
        assert!(error.0.contains("nodes"), "{error}");
        let huge = format!("[{}]", vec!["1"; MAX_BYTES].join(","));
        assert!(parse(&huge).unwrap_err().0.contains("bytes"));
        // Very deep raw JSON is refused before it's read into nodes.
        let nested = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
        assert!(parse(&nested).is_err());
    }

    #[test]
    fn actions_and_controls_are_checked() {
        assert!(parse(&serde_json::to_string(&button("Go", "")).unwrap()).is_err());
        assert!(parse(&serde_json::to_string(&button("Go", "has space")).unwrap()).is_err());
        assert!(parse(&serde_json::to_string(&button("Go", "lookup:again")).unwrap()).is_ok());
        let unknown = segmented("Show as", "view", "pie", [("chart", "Chart")]);
        assert!(parse(&serde_json::to_string(&unknown).unwrap()).is_err());
        let ragged =
            json!({"type": "table", "caption": "c", "columns": ["a", "b"], "rows": [["1"]]});
        assert!(parse(&ragged.to_string()).unwrap_err().0.contains("cells"));
        let nan = r#"{"type": "progress", "label": "x", "value": 1, "max": 0}"#;
        assert!(parse(nan).is_err());
        let heading = r#"{"type": "heading", "text": "x", "level": 1}"#;
        assert!(parse(heading).is_err());
    }

    #[test]
    fn an_outcome_keeps_state_and_a_notice() {
        assert_eq!(parse_outcome("").unwrap(), Outcome::default());
        let outcome = parse_outcome(r#"{"state": {"view": "table"}, "notice": "Saved"}"#).unwrap();
        assert_eq!(outcome.state, Some(json!({"view": "table"})));
        assert_eq!(outcome.notice.as_deref(), Some("Saved"));
        assert!(parse_outcome(r#"{"reload": true}"#).is_err());
    }

    #[test]
    fn the_context_is_camel_case() {
        let context = Context {
            mode: Mode::Console,
            game_id: Some("g1".into()),
            game_title: None,
            state: json!({"view": "table"}),
        };
        assert_eq!(
            serde_json::to_value(&context).unwrap(),
            json!({"mode": "console", "gameId": "g1", "state": {"view": "table"}})
        );
    }
}
