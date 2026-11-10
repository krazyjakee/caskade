//! The app's hrefs as the webview writes them: a path and a query string
//! in TanStack Router's form, so links, "Open to → last" and the game
//! pages' Back chain read the same either way.
//!
//! TanStack writes each value as JSON unless it's a string that wouldn't
//! read back as JSON (`q=pines`, but `q=%22123%22` for the text "123", and
//! `opening=true` for the boolean), and reads each the other way.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

/// What `encodeURIComponent` leaves alone.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// A query value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Bool(bool),
    Number(f64),
}

impl Value {
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// A path and its query, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Href {
    pub path: String,
    pub query: Vec<(String, Value)>,
}

impl Href {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            query: Vec::new(),
        }
    }

    /// Adds `key` when there's a value.
    pub fn with(mut self, key: &str, value: Option<Value>) -> Self {
        if let Some(value) = value {
            self.query.push((key.to_owned(), value));
        }
        self
    }

    pub fn text(self, key: &str, value: Option<&str>) -> Self {
        self.with(key, value.map(|value| Value::Text(value.to_owned())))
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.query
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    pub fn get_text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::text)
    }

    /// `path?query`, or nothing for anything but an app path (no scheme,
    /// host or protocol-relative link).
    pub fn parse(href: &str) -> Option<Self> {
        if href.len() > 4096 || !href.starts_with('/') || href.starts_with("//") {
            return None;
        }
        if href.contains('\\') {
            return None;
        }
        let href = href.split('#').next().unwrap_or_default();
        let (path, query) = href.split_once('?').unwrap_or((href, ""));
        let query = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                (decode(key), read(&decode(value)))
            })
            .collect();
        Some(Self {
            path: path.to_owned(),
            query,
        })
    }

    /// The path's segments, each decoded.
    pub fn segments(&self) -> Vec<String> {
        self.path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(decode)
            .collect()
    }
}

impl std::fmt::Display for Href {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.path)?;
        for (index, (key, value)) in self.query.iter().enumerate() {
            let value = write(value);
            f.write_str(if index == 0 { "?" } else { "&" })?;
            write!(
                f,
                "{}={}",
                utf8_percent_encode(key, COMPONENT),
                utf8_percent_encode(&value, COMPONENT)
            )?;
        }
        Ok(())
    }
}

/// A path segment, encoded (`encodeURIComponent`).
pub fn segment(text: &str) -> String {
    utf8_percent_encode(text, COMPONENT).to_string()
}

fn decode(text: &str) -> String {
    percent_decode_str(&text.replace('+', " "))
        .decode_utf8_lossy()
        .into_owned()
}

/// A value as TanStack reads it: JSON where it parses, else the text.
fn read(text: &str) -> Value {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Bool(value)) => Value::Bool(value),
        Ok(serde_json::Value::Number(number)) => number
            .as_f64()
            .map_or(Value::Text(text.to_owned()), Value::Number),
        Ok(serde_json::Value::String(value)) => Value::Text(value),
        _ => Value::Text(text.to_owned()),
    }
}

/// A value as TanStack writes it.
fn write(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        // As JavaScript writes numbers: 6, not 6.0.
        Value::Number(number) if number.fract() == 0.0 && number.abs() < 1e15 => {
            format!("{number:.0}")
        }
        Value::Number(number) => number.to_string(),
        Value::Text(text) => {
            if serde_json::from_str::<serde_json::Value>(text).is_ok() {
                serde_json::Value::String(text.clone()).to_string()
            } else {
                text.clone()
            }
        }
    }
}

/// The games a Back chain keeps (`views/game/origin.ts`'s `MAX_GAMES`).
pub const MAX_GAMES: usize = 4;

fn is_game(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    matches!(segments.as_slice(), ["games", _] | ["store", "games", _])
}

/// The pages a game can go back to.
fn is_page(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    matches!(
        segments.as_slice(),
        [] | ["library"]
            | ["store"]
            | ["collections"]
            | ["collections", _]
            | ["activity"]
            | ["plugins", _, _]
    ) || is_game(path)
}

fn page(value: &str) -> Option<Href> {
    Href::parse(value).filter(|href| is_page(&href.path))
}

/// The pages `value` goes back through, nearest first, each without its
/// own `from`: game pages, then the browsing page, if any.
fn chain(value: Option<&str>) -> Vec<Href> {
    let mut pages = Vec::new();
    let mut next = value.map(str::to_owned);
    // A bound, so a crafted href can't loop.
    for _ in 0..16 {
        let Some(mut href) = next.as_deref().and_then(page) else {
            break;
        };
        if !is_game(&href.path) {
            pages.push(href);
            break;
        }
        next = href.get_text("from").map(str::to_owned);
        href.query.retain(|(key, _)| key != "from");
        pages.push(href);
    }
    pages
}

/// Nests `pages` back into one href, keeping the nearest [`MAX_GAMES`]
/// games and the browsing page.
fn build(mut pages: Vec<Href>) -> Option<String> {
    let root = pages
        .last()
        .filter(|last| !is_game(&last.path))
        .cloned()
        .inspect(|_| {
            pages.pop();
        });
    pages.truncate(MAX_GAMES);
    pages
        .into_iter()
        .rev()
        .fold(root.map(|root| root.to_string()), |from, mut page| {
            let Some(from) = from else {
                return Some(page.to_string());
            };
            page.query.retain(|(key, _)| key != "from");
            page.query.push(("from".into(), Value::Text(from)));
            Some(page.to_string())
        })
}

/// `from` as a game page accepts it: an app page, or nothing.
pub fn valid_return(value: Option<&str>) -> Option<String> {
    build(chain(value))
}

/// Where game `id`'s page (under `base`, `/games` or `/store/games`) goes
/// back to, given its `from`: never to itself.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the game pages (#100, #102) use it")
)]
pub fn back_target(id: &str, from: Option<&str>, base: &str) -> Option<String> {
    let own = format!("{base}/{}", segment(id));
    let mut pages = chain(from);
    while pages.first().is_some_and(|page| page.path == own) {
        pages.remove(0);
    }
    build(pages)
}
