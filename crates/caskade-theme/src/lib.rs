//! Theme packages (TECH-PLAN §5, `docs/theming/README.md`). A theme is a
//! folder with `theme.json`, an optional DTCG `tokens.json`, an optional
//! `theme.css` and `assets/`: tokens, CSS, fonts and images, never code.
//!
//! [`Theme::load`] validates a theme against the theming contract (the
//! token manifest from `packages/tokens` and the anatomy catalogue from
//! `packages/ui`) and keeps its compiled parts; [`compile`] joins an
//! `extends` chain into one stylesheet in the `cs.theme` cascade layer.
//! Every problem names its file and, where it has one, its line and column.
//!
//! [`Locales`] reads a theme's translations of its name and description
//! (`locales/<lang>.po`, #219). Their problems never stop a theme applying.

mod contract;
mod css;
mod files;
mod locales;
mod manifest;
mod tokens;

pub use files::{DirFiles, Files, MemFiles};
pub use locales::Locales;
pub use manifest::ThemeManifest;
use std::fmt;

/// The theme every other theme builds on unless it says otherwise.
pub const DEFAULT_THEME: &str = "caskade.default";
/// Image types a theme may use and show as previews.
const IMAGE_TYPES: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "avif", "svg"];
/// Font types a theme may use.
const FONT_TYPES: &[&str] = &["woff2", "woff", "ttf", "otf"];

fn extension(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

pub(crate) fn is_image(path: &str) -> bool {
    IMAGE_TYPES.contains(&extension(path).as_str())
}

pub(crate) fn is_asset(path: &str) -> bool {
    is_image(path) || FONT_TYPES.contains(&extension(path).as_str())
}
const MAX_TEXT: usize = 1 << 20;
const MAX_CHAIN: usize = 8;

/// One thing wrong with a theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// Relative to the theme folder: `theme.json`, `theme.css`, `assets/x.js`.
    pub file: String,
    /// 1-based, when the problem has a position in the file.
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}

impl Problem {
    pub(crate) fn new(file: &str, message: impl Into<String>) -> Self {
        Self {
            file: file.to_owned(),
            line: None,
            column: None,
            message: message.into(),
        }
    }

    pub(crate) fn at(file: &str, line: u32, column: u32, message: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            column: Some(column),
            ..Self::new(file, message)
        }
    }

    /// A JSON syntax or shape error in `file`, at its position.
    pub(crate) fn json(file: &str, error: &serde_json::Error) -> Self {
        let line = u32::try_from(error.line()).unwrap_or(u32::MAX);
        let column = u32::try_from(error.column()).unwrap_or(u32::MAX);
        let message = error.to_string();
        // serde_json appends " at line L column C"; the position is shown already.
        let message = message
            .rsplit_once(" at line ")
            .map_or(message.as_str(), |(text, _)| text);
        Self::at(file, line, column, message)
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file)?;
        if let (Some(line), Some(column)) = (self.line, self.column) {
            write!(f, ":{line}:{column}")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// A theme that passed validation, ready to compile.
#[derive(Clone, Debug)]
pub struct Theme {
    pub manifest: ThemeManifest,
    /// `--cs-*` declarations from `tokens.json`.
    tokens: Vec<(String, String)>,
    /// `theme.css` with its `url()`s pointing at `/<id>/<path>`.
    css: String,
}

impl Theme {
    /// Reads and checks a theme. Every problem is reported, not just the first.
    pub fn load(files: &dyn Files) -> Result<Self, Vec<Problem>> {
        let mut problems = Vec::new();
        let list = files.list();
        for path in &list {
            if let Some(problem) = forbidden(path) {
                problems.push(problem);
            }
        }
        let manifest = match files.read("theme.json") {
            None => {
                problems.push(Problem::new("theme.json", "a theme needs a theme.json"));
                None
            }
            Some(bytes) => manifest::parse(&bytes, &list)
                .map_err(|mut found| problems.append(&mut found))
                .ok(),
        };
        let tokens = match read_text(files, "tokens.json", &mut problems) {
            Some(text) => tokens::compile(&text).unwrap_or_else(|mut found| {
                problems.append(&mut found);
                Vec::new()
            }),
            None => Vec::new(),
        };
        let id = manifest
            .as_ref()
            .map_or("", |manifest| manifest.id.as_str());
        let css = match read_text(files, "theme.css", &mut problems) {
            Some(text) => css::lint(&text, id, &list).unwrap_or_else(|mut found| {
                problems.append(&mut found);
                String::new()
            }),
            None => String::new(),
        };
        match manifest {
            Some(manifest) if problems.is_empty() => Ok(Self {
                manifest,
                tokens,
                css,
            }),
            _ => Err(problems),
        }
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }
}

fn read_text(files: &dyn Files, path: &str, problems: &mut Vec<Problem>) -> Option<String> {
    let bytes = files.read(path)?;
    if bytes.len() > MAX_TEXT {
        problems.push(Problem::new(path, "is larger than 1 MiB"));
        return None;
    }
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(_) => {
            problems.push(Problem::new(path, "is not UTF-8"));
            None
        }
    }
}

/// Code and documents a browser could run: a theme ships none.
fn forbidden(path: &str) -> Option<Problem> {
    let extension = extension(path);
    let code = [
        "js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts", "wasm", "html", "htm", "xhtml",
        "exe", "dll", "so", "dylib", "sh", "bat", "cmd", "ps1",
    ];
    code.contains(&extension.as_str()).then(|| {
        Problem::new(
            path,
            "themes contain no code: scripts, HTML and binaries are refused",
        )
    })
}

/// The `extends` chain of `id`, base first, resolved with `lookup`.
pub fn chain<'a>(
    id: &str,
    lookup: impl Fn(&str) -> Option<&'a Theme>,
) -> Result<Vec<&'a Theme>, String> {
    let mut chain: Vec<&Theme> = Vec::new();
    let mut next = Some(id.to_owned());
    while let Some(id) = next {
        if chain.iter().any(|theme| theme.id() == id) {
            let path: Vec<&str> = chain.iter().map(|theme| theme.id()).collect();
            return Err(format!("extends cycle: {} → {id}", path.join(" → ")));
        }
        if chain.len() == MAX_CHAIN {
            return Err(format!("extends chain is longer than {MAX_CHAIN} themes"));
        }
        let theme = lookup(&id).ok_or_else(|| match chain.last() {
            Some(child) => format!(
                "{} extends {id}, which isn't installed or isn't valid",
                child.id()
            ),
            None => format!("theme {id} isn't installed or isn't valid"),
        })?;
        next = theme.manifest.extends.clone();
        chain.push(theme);
    }
    chain.reverse();
    Ok(chain)
}

/// One stylesheet for a chain from [`chain`]: the tokens (a later theme's
/// value wins) and then each theme's CSS, all in `@layer cs.theme`.
pub fn compile(chain: &[&Theme]) -> String {
    let Some(leaf) = chain.last() else {
        return String::new();
    };
    let mut tokens: Vec<(&str, &str)> = Vec::new();
    for theme in chain {
        for (name, value) in &theme.tokens {
            match tokens.iter_mut().find(|(known, _)| known == name) {
                Some(entry) => entry.1 = value,
                None => tokens.push((name, value)),
            }
        }
    }
    let mut out = format!(
        "/* {} {}, compiled by caskade-theme. */\n@layer cs.theme {{\n",
        leaf.manifest.id, leaf.manifest.version
    );
    if !tokens.is_empty() {
        out.push_str(":root {\n");
        for (name, value) in tokens {
            out.push_str(&format!("  {name}: {value};\n"));
        }
        out.push_str("}\n");
    }
    for theme in chain {
        if !theme.css.trim().is_empty() {
            out.push_str(&format!("\n/* {} */\n{}", theme.manifest.id, theme.css));
            if !theme.css.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out.push_str("}\n");
    out
}

/// Whether `path` is a file the theme's scheme may serve: an asset under
/// `assets/` or a preview, with an asset type.
pub fn servable(theme: &Theme, path: &str) -> bool {
    is_asset(path)
        && (path.starts_with("assets/")
            || theme
                .manifest
                .previews
                .iter()
                .any(|preview| preview == path))
}
