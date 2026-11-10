//! The user's CSS (#142): one stylesheet in `@layer cs.user`, which beats
//! every theme and component style without `!important`. Like themes, it
//! can't load anything from other origins: a remote `@import` or `url()`
//! refuses the whole sheet, with a message. The webview's
//! `appearance/userCss.ts`, rule for rule.

/// The most custom CSS kept, in characters (`core.appearance.customCss`).
pub const USER_CSS_MAX: usize = 32 * 1024;

/// The contract's layer order (`styles/index.css`), restated so the user
/// layer stays last even if this sheet loads first.
pub const LAYER_ORDER: &str =
    "@layer cs.reset, cs.tokens, cs.base, cs.components, cs.plugins, cs.theme, cs.user;";

/// Something that stops the user's CSS from applying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssProblem {
    /// 1-based.
    pub line: usize,
    pub message: String,
}

const LOCAL_SCHEMES: [&str; 6] = [
    "data",
    "blob",
    "caskade-theme",
    "caskade-asset",
    "caskade-plugin",
    "asset",
];

/// Whether a URL stays inside Caskade: relative, `data:`, the app's own
/// schemes, or a `localhost` host (how Windows serves them).
pub fn is_local_url(raw: &str) -> bool {
    // Browsers drop tabs and newlines in URLs and read `\` as `/`.
    let url: String = raw
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let url = url.trim();
    let mut chars = url.chars();
    if matches!(
        (chars.next(), chars.next()),
        (Some('\\' | '/'), Some('\\' | '/'))
    ) {
        return false;
    }
    let Some(scheme) = scheme(url) else {
        return true;
    };
    if LOCAL_SCHEMES.contains(&scheme.as_str()) {
        return true;
    }
    if scheme != "http" && scheme != "https" {
        return false;
    }
    match host(&url.replace('\\', "/")) {
        Some(host) => host == "localhost" || host.ends_with(".localhost"),
        None => false,
    }
}

/// `^([a-zA-Z][a-zA-Z0-9+.-]*):`, lower-cased.
fn scheme(url: &str) -> Option<String> {
    let end = url.find(':')?;
    let scheme = &url[..end];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')))
    .then(|| scheme.to_ascii_lowercase())
}

/// The host of an `http(s)://` URL as WHATWG URL parsing reads it, lower
/// case; `None` when it has none.
fn host(url: &str) -> Option<String> {
    let rest = &url[url.find(':')? + 1..];
    let rest = rest.trim_start_matches('/');
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next()?
    } else {
        host_port.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Functions whose string arguments are URLs.
const URL_FUNCTIONS: [&str; 3] = ["image-set", "-webkit-image-set", "src"];

fn is_name_start(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c >= '\u{80}')
}

fn is_name(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c >= '\u{80}')
}

fn is_hex(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_hexdigit())
}

struct Import {
    line: usize,
    target: Option<String>,
}

struct Scan {
    css: Vec<char>,
    i: usize,
    line: usize,
    problems: Vec<CssProblem>,
    pending_import: Option<Import>,
}

impl Scan {
    fn at(&self, offset: usize) -> Option<char> {
        self.css.get(self.i + offset).copied()
    }

    fn read_escape(&mut self) -> String {
        self.i += 1; // the backslash
        let Some(c) = self.at(0) else {
            return "\u{FFFD}".into();
        };
        if c == '\n' {
            self.line += 1;
            self.i += 1;
            return String::new();
        }
        if c.is_ascii_hexdigit() {
            let mut hex = String::new();
            while hex.len() < 6 && is_hex(self.at(0)) {
                hex.push(self.css[self.i]);
                self.i += 1;
            }
            if matches!(self.at(0), Some(' ' | '\t' | '\n')) {
                if self.at(0) == Some('\n') {
                    self.line += 1;
                }
                self.i += 1;
            }
            let code = u32::from_str_radix(&hex, 16).unwrap_or(0);
            return match char::from_u32(code) {
                Some(c) if code != 0 => c.into(),
                _ => "\u{FFFD}".into(),
            };
        }
        self.i += 1;
        c.into()
    }

    fn name(&mut self) -> String {
        let mut text = String::new();
        while let Some(c) = self.at(0) {
            if c == '\\' && self.at(1).is_some_and(|next| next != '\n') {
                text.push_str(&self.read_escape());
            } else if is_name(Some(c)) {
                text.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        text
    }

    fn string(&mut self, quote: char) -> String {
        self.i += 1;
        let mut text = String::new();
        while let Some(c) = self.at(0) {
            if c == quote {
                self.i += 1;
                break;
            }
            if c == '\n' {
                break; // a bad string ends at the line
            }
            if c == '\\' {
                text.push_str(&self.read_escape());
            } else {
                text.push(c);
                self.i += 1;
            }
        }
        text
    }

    fn skip_space(&mut self) {
        while let Some(c @ (' ' | '\t' | '\n')) = self.at(0) {
            if c == '\n' {
                self.line += 1;
            }
            self.i += 1;
        }
    }

    /// After `url(`: the URL, through the closing parenthesis.
    fn url(&mut self) -> String {
        self.skip_space();
        let mut text = String::new();
        match self.at(0) {
            Some(quote @ ('"' | '\'')) => text = self.string(quote),
            _ => {
                while let Some(c) = self.at(0) {
                    if c == ')' || matches!(c, ' ' | '\t' | '\n') {
                        break;
                    }
                    if c == '\\' {
                        text.push_str(&self.read_escape());
                    } else {
                        text.push(c);
                        self.i += 1;
                    }
                }
            }
        }
        while let Some(c) = self.at(0) {
            if c == ')' {
                break;
            }
            if c == '\n' {
                self.line += 1;
            }
            self.i += 1;
        }
        self.i += 1;
        text
    }

    fn target(&mut self, value: String, at: usize) {
        if let Some(import) = &mut self.pending_import {
            import.target.get_or_insert(value);
        } else if !is_local_url(&value) {
            self.problems.push(CssProblem {
                line: at,
                message: caskade_core::tr!(
                    "url({url}) is refused: custom CSS can't load anything from other origins.",
                    url = value
                ),
            });
        }
    }

    fn end_import(&mut self) {
        let Some(Import { line, target }) = self.pending_import.take() else {
            return;
        };
        let message = match target {
            Some(url) if !is_local_url(&url) => caskade_core::tr!(
                "@import of {url} is refused: custom CSS can't load anything from other origins.",
                url = url
            ),
            _ => caskade_core::tr!(
                "@import isn't supported in custom CSS: paste the rules in instead."
            ),
        };
        self.problems.push(CssProblem { line, message });
    }
}

/// What stops `source` from being applied: remote `@import`s and `url()`s
/// (any `@import`, in fact: it can't work inside the user layer), and
/// braces that would close the layer early. Follows CSS tokenization, so
/// comments, strings and escapes (`\75 rl(`) can't hide a URL.
pub fn check(source: &str) -> Vec<CssProblem> {
    let css = source
        .replace("\r\n", "\n")
        .replace(['\r', '\u{c}'], "\n")
        .replace('\0', "\u{FFFD}");
    let mut scan = Scan {
        css: css.chars().collect(),
        i: 0,
        line: 1,
        problems: Vec::new(),
        pending_import: None,
    };
    let mut depth: i64 = 0;
    let mut stack: Vec<String> = Vec::new();
    while let Some(c) = scan.at(0) {
        let at = scan.line;
        if c == '\n' {
            scan.line += 1;
            scan.i += 1;
        } else if c == '/' && scan.at(1) == Some('*') {
            let end = (scan.i + 2..scan.css.len().saturating_sub(1))
                .find(|&j| scan.css[j] == '*' && scan.css[j + 1] == '/');
            let stop = end.map_or(scan.css.len(), |end| end + 2);
            scan.line += scan.css[scan.i..stop]
                .iter()
                .filter(|&&c| c == '\n')
                .count();
            if end.is_none() {
                scan.problems.push(CssProblem {
                    line: at,
                    message: caskade_core::tr!("A comment is never closed."),
                });
            }
            scan.i = stop;
        } else if c == '"' || c == '\'' {
            let value = scan.string(c);
            let in_url_function = stack
                .last()
                .is_some_and(|function| URL_FUNCTIONS.contains(&function.as_str()));
            if scan.pending_import.is_some() || in_url_function {
                scan.target(value, at);
            }
        } else if c == '@' {
            scan.i += 1;
            if scan.name().to_lowercase() == "import" {
                scan.end_import();
                scan.pending_import = Some(Import {
                    line: at,
                    target: None,
                });
            }
        } else if is_name_start(Some(c))
            || (c == '\\' && scan.at(1) != Some('\n'))
            || (c == '-' && (is_name_start(scan.at(1)) || matches!(scan.at(1), Some('-' | '\\'))))
        {
            let start = scan.i;
            let ident = scan.name().to_lowercase();
            if scan.i == start {
                scan.i += 1; // a lone backslash at the end
            } else if scan.at(0) == Some('(') {
                scan.i += 1;
                if ident == "url" {
                    let url = scan.url();
                    scan.target(url, at);
                } else {
                    stack.push(ident);
                }
            }
        } else if c == '(' {
            stack.push(String::new());
            scan.i += 1;
        } else if c == ')' {
            stack.pop();
            scan.i += 1;
        } else if c == '{' {
            scan.end_import();
            depth += 1;
            scan.i += 1;
        } else if c == '}' {
            depth -= 1;
            if depth < 0 {
                scan.problems.push(CssProblem {
                    line: at,
                    message: caskade_core::tr!(
                        "A closing brace closes nothing: custom CSS has to stay inside its layer."
                    ),
                });
                depth = 0;
            }
            scan.i += 1;
        } else if c == ';' {
            scan.end_import();
            scan.i += 1;
        } else {
            scan.i += 1;
        }
    }
    scan.end_import();
    if depth > 0 {
        scan.problems.push(CssProblem {
            line: scan.line,
            message: caskade_core::tr!("An opening brace is never closed."),
        });
    }
    scan.problems
}

/// The stylesheet for `css`: the layer order, then `css` in `cs.user`.
pub fn stylesheet(css: &str) -> String {
    format!("{LAYER_ORDER}\n@layer cs.user {{\n{css}\n}}\n")
}

/// The problems, one line each.
pub fn problem_text(problems: &[CssProblem]) -> String {
    problems
        .iter()
        .map(|problem| {
            caskade_core::tr!(
                "Line {line}: {problem}",
                line = problem.line,
                problem = problem.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The user stylesheet to apply for `css`: `Ok(None)` when there's none
/// (empty), `Err` with the problems when it's refused, which applies none.
pub fn applicable(css: &str) -> Result<Option<String>, Vec<CssProblem>> {
    let problems = check(css);
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok((!css.trim().is_empty()).then(|| stylesheet(css)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(css: &str) -> Vec<String> {
        check(css)
            .into_iter()
            .map(|problem| problem.message)
            .collect()
    }

    #[test]
    fn refuses_remote_imports_and_urls_with_a_message() {
        assert_eq!(
            check(r#"@import url("https://example.com/look.css");"#),
            [CssProblem {
                line: 1,
                message: "@import of https://example.com/look.css is refused: custom CSS can't load anything from other origins.".into(),
            }]
        );
        assert_eq!(
            messages("\n\n@import 'http://evil.test/a.css';"),
            [
                "@import of http://evil.test/a.css is refused: custom CSS can't load anything from other origins."
            ]
        );
        for css in [
            r#"[data-cs="hero"] { background: url(https://example.com/bg.png); }"#,
            r#"[data-cs="hero"] { background: url( "//example.com/bg.png" ); }"#,
            // `\\` is a backslash in CSS, which URLs read as a slash.
            r#"[data-cs="hero"] { background: URL(/\\example.com/bg.png); }"#,
            r#"[data-cs="hero"] { background: \75 rl(https://example.com/bg.png); }"#,
            r#"[data-cs="hero"] { background: u\rl("ht\74 ps://example.com/bg.png"); }"#,
            r#"[data-cs="hero"] { background: image-set("https://example.com/a.png" 1x); }"#,
            r#"@font-face { font-family: X; src: url("ftp://example.com/x.woff2"); }"#,
        ] {
            let found = messages(css);
            assert_eq!(found.len(), 1, "{css}: {found:?}");
            assert!(found[0].starts_with("url("), "{css}: {found:?}");
            assert!(
                found[0].contains(") is refused: custom CSS can't load anything"),
                "{css}: {found:?}"
            );
        }
        // Line numbers count from the top of the box.
        let css = "a {}\n\n[data-cs=hero] {\n  background: url(http://x.test/a.png);\n}";
        assert_eq!(check(css)[0].line, 4);
    }

    #[test]
    fn keeps_local_urls_url_looking_text_and_comments() {
        assert_eq!(
            check(
                r#"
      /* url(https://example.com/a.png) and @import "x"; are only a comment */
      [data-cs="hero"] {
        background: url(data:image/png;base64,AAAA), url("caskade-theme://localhost/x/a.png");
        mask: url(http://caskade-theme.localhost/x/m.svg);
        content: "https://example.com is just text";
        --cs-color-accent: #e0457b;
      }
      @media (min-width: 40rem) { [data-cs="game-card"] { color: red; } }
    "#
            ),
            []
        );
        for url in [
            "assets/a.png",
            "#frag",
            "data:font/woff2;base64,AA",
            "http://localhost:1420/x",
        ] {
            assert!(is_local_url(url), "{url}");
        }
        for url in [
            "https://example.com/a.png",
            "//example.com",
            "\\\\example.com",
            "http://localhost.example.com/",
            "file:///etc/passwd",
            " h\ttps://example.com",
        ] {
            assert!(!is_local_url(url), "{url}");
        }
    }

    #[test]
    fn refuses_any_import_and_braces_that_would_leave_the_user_layer() {
        assert_eq!(
            messages(r#"@import "local.css";"#),
            ["@import isn't supported in custom CSS: paste the rules in instead."]
        );
        assert_eq!(
            messages(r#"[data-cs="hero"] { color: red; } } [data-cs="hero"] { color: blue; }"#),
            ["A closing brace closes nothing: custom CSS has to stay inside its layer."]
        );
        assert_eq!(
            messages(r#"[data-cs="hero"] { color: red;"#),
            ["An opening brace is never closed."]
        );
        assert_eq!(
            messages(r#"[data-cs="hero"] { color: red; } /* open"#),
            ["A comment is never closed."]
        );
        assert_eq!(
            messages(r#"[data-cs="hero"] { content: "}"; }"#),
            Vec::<String>::new()
        );
        // Unfinished input ends the scan instead of hanging it.
        assert_eq!(
            messages(r#"[data-cs="hero"] { color: red; } \"#),
            Vec::<String>::new()
        );
        assert_eq!(
            messages(r#"[data-cs="hero"] { background: url(https://example.com"#),
            [
                "url(https://example.com) is refused: custom CSS can't load anything from other origins.",
                "An opening brace is never closed."
            ]
        );
    }

    #[test]
    fn applies_in_the_user_layer_and_not_at_all_when_empty_or_refused() {
        let rule = r#"[data-cs="hero"] { color: red; }"#;
        assert_eq!(
            applicable(rule),
            Ok(Some(format!(
                "{LAYER_ORDER}\n@layer cs.user {{\n{rule}\n}}\n"
            )))
        );
        let refused =
            applicable(&format!("{rule}\n@import url(https://example.com/x.css);")).unwrap_err();
        assert!(
            problem_text(&refused).starts_with("Line 2: @import of https://example.com/x.css "),
            "{}",
            problem_text(&refused)
        );
        assert_eq!(applicable("  \n"), Ok(None));
        // A stray } can't move rules out of the layer.
        assert_eq!(check(r#"} [data-cs="hero"] { color: hotpink; }"#).len(), 1);
    }

    /// The order the webview's entry declares (`apps/desktop/src/styles/index.css`).
    #[test]
    fn the_layer_order_is_the_entrys() {
        let entry = include_str!("../../../apps/desktop/src/styles/index.css");
        assert!(
            entry.contains(LAYER_ORDER),
            "index.css changed its layer order"
        );
    }
}
