//! `theme.css`: checked against the contract and rewritten for serving.
//!
//! - Selectors may use `data-cs`, `data-part` and the contract's other
//!   `data-*` attributes, pseudo-classes and pseudo-elements, and `*`.
//!   Classes, ids, element names and unknown roots, parts or attributes
//!   are errors, and so are the consent dialogs' roots.
//! - Custom properties named `--cs-*` must be declared tokens.
//! - `url()`s must name an image or font under `assets/`; they are rewritten
//!   to `/<id>/<path>` on the theme scheme. Remote and absolute URLs,
//!   `@import`, `!important` and at-rules other than `@media`, `@supports`,
//!   `@container`, `@keyframes` and `@font-face` are errors.
use crate::contract::contract;
use crate::{Problem, is_asset};
use cssparser::{Delimiter, ParseError, Parser, SourceLocation, Token};

const FILE: &str = "theme.css";

/// The consent dialogs' roots (CSK-08): a theme could hide what the user
/// agrees to, add false reassurance or swap the buttons, so it may not
/// target them, whether or not they are catalogued.
const PROTECTED: [&str; 2] = ["plugin-review", "install-confirmation"];

struct Lint<'a> {
    id: &'a str,
    files: &'a [String],
    problems: Vec<Problem>,
    /// Byte ranges of `url()`s and their replacements.
    edits: Vec<(usize, usize, String)>,
}

impl Lint<'_> {
    fn problem(&mut self, at: SourceLocation, message: impl Into<String>) {
        self.problems
            .push(Problem::at(FILE, at.line + 1, at.column, message));
    }
}

type Done = Result<(), ParseError<()>>;

pub(crate) fn lint(text: &str, id: &str, files: &[String]) -> Result<String, Vec<Problem>> {
    let mut parser = Parser::new(text);
    let mut lint = Lint {
        id,
        files,
        problems: Vec::new(),
        edits: Vec::new(),
    };
    rules(&mut parser, &mut lint, false);
    if !lint.problems.is_empty() {
        return Err(lint.problems);
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    lint.edits.sort_by_key(|edit| edit.0);
    for (start, end, replacement) in lint.edits {
        out.push_str(&text[at..start]);
        out.push_str(&replacement);
        at = end;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

fn nested<'i>(p: &mut Parser<'i>, walk: impl FnOnce(&mut Parser<'i>)) {
    let _ = p.parse_nested_block(|p| -> Done {
        walk(p);
        while p.next_including_whitespace_and_comments().is_ok() {}
        Ok(())
    });
}

fn rules(p: &mut Parser<'_>, lint: &mut Lint, keyframes: bool) {
    loop {
        let state = p.state();
        let at = p.current_source_location();
        let Ok(token) = p.next_including_whitespace_and_comments().cloned() else {
            return;
        };
        match token {
            Token::WhiteSpace(_)
            | Token::Comment(_)
            | Token::CDO
            | Token::CDC
            | Token::Semicolon => {}
            Token::AtKeyword(name) => at_rule(p, lint, &name.to_ascii_lowercase(), at),
            _ if keyframes => keyframe(p, lint, at, token),
            _ => {
                p.reset(&state);
                style_rule(p, lint);
            }
        }
    }
}

/// Consumes an at-rule's prelude; true when a block follows.
fn prelude(p: &mut Parser<'_>) -> bool {
    loop {
        match p.next_including_whitespace_and_comments() {
            Ok(Token::CurlyBracketBlock) => return true,
            Ok(Token::Semicolon) | Err(_) => return false,
            Ok(_) => {}
        }
    }
}

/// Consumes an at-rule's prelude; true when a block follows, a problem when
/// none does.
fn block(p: &mut Parser<'_>, lint: &mut Lint, name: &str, at: SourceLocation) -> bool {
    let block = prelude(p);
    if !block {
        lint.problem(at, format!("@{name} needs a block"));
    }
    block
}

fn at_rule(p: &mut Parser<'_>, lint: &mut Lint, name: &str, at: SourceLocation) {
    match name {
        "media" | "supports" | "container" => {
            if block(p, lint, name, at) {
                nested(p, |p| rules(p, lint, false));
            }
        }
        "keyframes" | "-webkit-keyframes" => {
            if block(p, lint, name, at) {
                nested(p, |p| rules(p, lint, true));
            }
        }
        "font-face" => {
            if block(p, lint, name, at) {
                nested(p, |p| declarations(p, lint));
            }
        }
        "import" => {
            lint.problem(
                at,
                "@import isn't allowed: a theme is one theme.css with local assets",
            );
            prelude(p).then(|| nested(p, |_| {}));
        }
        _ => {
            lint.problem(at, format!("@{name} isn't allowed in a theme"));
            prelude(p).then(|| nested(p, |_| {}));
        }
    }
}

/// `from`, `to` or percentages, then declarations.
fn keyframe(p: &mut Parser<'_>, lint: &mut Lint, at: SourceLocation, first: Token<'_>) {
    let mut token = Ok(first);
    loop {
        match token {
            Ok(Token::CurlyBracketBlock) => return nested(p, |p| declarations(p, lint)),
            Err(()) => return lint.problem(at, "keyframe needs a block"),
            Ok(
                Token::Percentage { .. } | Token::Comma | Token::WhiteSpace(_) | Token::Comment(_),
            ) => {}
            Ok(Token::Ident(ref name))
                if name.eq_ignore_ascii_case("from") || name.eq_ignore_ascii_case("to") => {}
            Ok(_) => lint.problem(at, "a keyframe selector is from, to or a percentage"),
        }
        token = p
            .next_including_whitespace_and_comments()
            .cloned()
            .map_err(|_| ());
    }
}

fn style_rule(p: &mut Parser<'_>, lint: &mut Lint) {
    let at = p.current_source_location();
    if selector(p, lint, None, true) {
        nested(p, |p| declarations(p, lint));
    } else {
        lint.problem(at, "expected a rule block");
    }
}

/// Checks a selector list. At the top level it stops at the rule's block
/// and returns whether one was found; nested (inside `:is()`) it runs to
/// the end. `outer` is the root an enclosing selector already matched.
fn selector(p: &mut Parser<'_>, lint: &mut Lint, outer: Option<String>, top: bool) -> bool {
    let initial = outer;
    let mut outer = initial.clone();
    let mut current: Option<String> = None;
    let mut start = true;
    loop {
        let at = p.current_source_location();
        let Ok(token) = p.next_including_whitespace_and_comments().cloned() else {
            return false;
        };
        match token {
            Token::CurlyBracketBlock if top => return true,
            Token::WhiteSpace(_) | Token::Comment(_) | Token::Delim('>' | '+' | '~') => {
                boundary(&mut outer, &mut current);
                start = true;
            }
            Token::Comma => {
                outer = initial.clone();
                current = None;
                start = true;
            }
            Token::Delim('*') => start = false,
            Token::Delim('.') => {
                let name = match p.next_including_whitespace_and_comments() {
                    Ok(Token::Ident(name)) => format!(".{name}"),
                    _ => ".".to_owned(),
                };
                lint.problem(
                    at,
                    format!("class selector {name}: class names aren't API; target data-cs and data-part"),
                );
                start = false;
            }
            Token::IDHash(name) | Token::Hash(name) => {
                lint.problem(
                    at,
                    format!("id selector #{name}: target data-cs and data-part"),
                );
                start = false;
            }
            Token::Ident(name) => {
                let what = if start {
                    "element selector"
                } else {
                    "unexpected"
                };
                lint.problem(
                    at,
                    format!(
                        "{what} {name}: target data-cs and data-part (use :root for the document)"
                    ),
                );
                start = false;
            }
            Token::Colon => {
                pseudo(p, lint, at, outer.clone().or(current.clone()));
                start = false;
            }
            Token::SquareBracketBlock => {
                let root = outer.clone();
                nested(p, |p| attribute(p, lint, at, root.as_deref(), &mut current));
                start = false;
            }
            Token::Delim('&') => {
                lint.problem(at, "nested rules aren't supported; write a separate rule")
            }
            other => lint.problem(at, format!("unexpected {} in a selector", describe(&other))),
        }
    }
}

/// A combinator ends a compound: its root becomes the outer root.
fn boundary(outer: &mut Option<String>, current: &mut Option<String>) {
    if let Some(root) = current.take() {
        *outer = Some(root);
    }
}

fn pseudo(p: &mut Parser<'_>, lint: &mut Lint, at: SourceLocation, root: Option<String>) {
    let element = matches!(p.try_parse(|p| p.expect_colon()), Ok(()));
    match p.next_including_whitespace_and_comments().cloned() {
        Ok(Token::Ident(_)) => {}
        Ok(Token::Function(name)) if !element => match name.to_ascii_lowercase().as_str() {
            "is" | "where" | "not" | "has" => nested(p, |p| {
                selector(p, lint, root, false);
            }),
            "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" | "lang"
            | "dir" => {
                nested(p, |_| {});
            }
            other => {
                lint.problem(at, format!(":{other}() isn't supported in a theme"));
                nested(p, |_| {});
            }
        },
        Ok(Token::Function(name)) => {
            lint.problem(at, format!("::{name}() isn't supported in a theme"));
            nested(p, |_| {});
        }
        _ => lint.problem(at, "expected a pseudo-class or pseudo-element name"),
    }
}

fn attribute(
    p: &mut Parser<'_>,
    lint: &mut Lint,
    at: SourceLocation,
    outer: Option<&str>,
    current: &mut Option<String>,
) {
    let mut name: Option<String> = None;
    let mut matcher: Option<&str> = None;
    let mut value: Option<String> = None;
    while let Ok(token) = p.next_including_whitespace_and_comments().cloned() {
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            Token::Ident(text) if name.is_none() => name = Some(text.to_string()),
            Token::Delim('=') if name.is_some() && matcher.is_none() => matcher = Some("="),
            Token::IncludeMatch
            | Token::DashMatch
            | Token::PrefixMatch
            | Token::SuffixMatch
            | Token::SubstringMatch
                if name.is_some() && matcher.is_none() =>
            {
                matcher = Some("~")
            }
            Token::Ident(text) | Token::QuotedString(text)
                if matcher.is_some() && value.is_none() =>
            {
                value = Some(text.to_string())
            }
            Token::Ident(flag) if value.is_some() && (&*flag == "i" || &*flag == "s") => {}
            other => {
                return lint.problem(
                    at,
                    format!("unexpected {} in an attribute selector", describe(&other)),
                );
            }
        }
    }
    let Some(name) = name else {
        return lint.problem(at, "empty attribute selector");
    };
    let contract = contract();
    if !contract.anatomy.attributes.contains(&name) {
        return lint.problem(
            at,
            format!("[{name}] is not a contract attribute (see docs/theming/anatomy.md)"),
        );
    }
    let exact = matcher == Some("=");
    match (name.as_str(), value) {
        ("data-cs", Some(root)) if exact => {
            if PROTECTED
                .iter()
                .any(|protected| protected.eq_ignore_ascii_case(&root))
            {
                lint.problem(
                    at,
                    format!("data-cs \"{root}\" is a consent dialog: themes can't restyle it"),
                );
            } else if contract.anatomy.roots.contains_key(&root) {
                *current = Some(root);
            } else {
                lint.problem(
                    at,
                    format!("data-cs \"{root}\" is not a catalogued component"),
                );
            }
        }
        ("data-part", Some(part)) if exact => match outer
            .and_then(|root| contract.anatomy.roots.get(root).map(|entry| (root, entry)))
        {
            Some((root, entry)) if !entry.parts.contains(&part) => {
                lint.problem(at, format!("data-part \"{part}\" is not a part of {root}"));
            }
            None if !contract.is_part(&part) => {
                lint.problem(at, format!("data-part \"{part}\" is not a catalogued part"));
            }
            _ => {}
        },
        ("data-cs" | "data-part", _) => {
            lint.problem(at, format!("match [{name}] exactly, with =\"…\""));
        }
        _ => {}
    }
}

fn declarations(p: &mut Parser<'_>, lint: &mut Lint) {
    loop {
        let at = p.current_source_location();
        let Ok(token) = p.next_including_whitespace_and_comments().cloned() else {
            return;
        };
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) | Token::Semicolon => {}
            Token::Ident(property) => {
                let property = property.to_string();
                let _ = p.parse_until_after(Delimiter::Semicolon, |p| -> Done {
                    if p.try_parse(|p| p.expect_colon()).is_err() {
                        lint.problem(at, format!("expected : after {property}"));
                    }
                    declaration(p, lint, at, &property);
                    Ok(())
                });
            }
            other => {
                lint.problem(
                    at,
                    format!(
                        "expected a declaration, found {} (nested rules aren't supported)",
                        describe(&other)
                    ),
                );
                let _ = p.parse_until_after(Delimiter::Semicolon, |p| -> Done {
                    while p.next_including_whitespace_and_comments().is_ok() {}
                    Ok(())
                });
            }
        }
    }
}

fn declaration(p: &mut Parser<'_>, lint: &mut Lint, at: SourceLocation, property: &str) {
    let lower = property.to_ascii_lowercase();
    if lower.starts_with("--cs-") && !contract().names.contains(property) {
        lint.problem(
            at,
            format!("{property} is not a token of the theming contract"),
        );
    }
    if ["behavior", "-moz-binding"].contains(&lower.as_str()) {
        lint.problem(at, format!("{property} isn't allowed"));
    }
    value(p, lint, false);
}

/// A value's tokens: `var()` names, `url()`s and `!important`.
fn value(p: &mut Parser<'_>, lint: &mut Lint, strings_are_urls: bool) {
    loop {
        let start = p.position();
        let at = p.current_source_location();
        let Ok(token) = p.next_including_whitespace_and_comments().cloned() else {
            return;
        };
        match token {
            Token::Delim('!') => {
                if p.try_parse(|p| p.expect_ident_matching("important"))
                    .is_ok()
                {
                    lint.problem(at, "!important isn't allowed: the cs.theme layer already beats component styles");
                }
            }
            Token::UnquotedUrl(url) => {
                let end = p.position().byte_index();
                url_value(lint, at, &url, start.byte_index(), end);
            }
            Token::QuotedString(url) if strings_are_urls => {
                let end = p.position().byte_index();
                url_value(lint, at, &url, start.byte_index(), end);
            }
            Token::Function(name) => match name.to_ascii_lowercase().as_str() {
                "url" => {
                    let mut url = None;
                    nested(p, |p| {
                        if let Ok(Token::QuotedString(text)) = p.next() {
                            url = Some(text.to_string());
                        }
                    });
                    let end = p.position().byte_index();
                    match url {
                        Some(url) => url_value(lint, at, &url, start.byte_index(), end),
                        None => lint.problem(at, "url() needs a quoted path"),
                    }
                }
                "var" => nested(p, |p| {
                    let at = p.current_source_location();
                    if let Ok(Token::Ident(name)) = p.next().cloned()
                        && name.starts_with("--cs-")
                        && !contract().names.contains(name.as_ref())
                    {
                        lint.problem(at, format!("{name} is not a token of the theming contract"));
                    }
                    value(p, lint, false);
                }),
                "image-set" | "-webkit-image-set" => nested(p, |p| value(p, lint, true)),
                "expression" => {
                    lint.problem(at, "expression() isn't allowed");
                    nested(p, |_| {});
                }
                _ => nested(p, |p| value(p, lint, false)),
            },
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                nested(p, |p| value(p, lint, false));
            }
            Token::BadUrl(_) | Token::BadString(_) => lint.problem(at, "malformed url or string"),
            _ => {}
        }
    }
}

fn url_value(lint: &mut Lint, at: SourceLocation, raw: &str, start: usize, end: usize) {
    let url = raw.trim();
    if let Some(data) = url.strip_prefix("data:") {
        if !(data.starts_with("image/") || data.starts_with("font/")) {
            lint.problem(at, "only image and font data: URLs are allowed");
        }
        return;
    }
    if url.is_empty() || url.contains(':') || url.starts_with('/') || url.starts_with('\\') {
        return lint.problem(
            at,
            format!("url({url}) leaves the theme: remote and absolute URLs are refused; ship the file under assets/"),
        );
    }
    let (path, fragment) = match url.find(['?', '#']) {
        Some(split) if url[split..].starts_with('#') => url.split_at(split),
        Some(split) => (&url[..split], ""),
        None => (url, ""),
    };
    let mut segments: Vec<String> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return lint.problem(at, format!("url({url}) escapes the theme folder"));
                }
            }
            _ => match decode(segment) {
                Some(segment) if !segment.contains(['/', '\\', '\0']) => segments.push(segment),
                _ => return lint.problem(at, format!("url({url}) has an invalid path")),
            },
        }
    }
    let relative = segments.join("/");
    if !relative.starts_with("assets/") {
        return lint.problem(at, format!("url({url}) must point under assets/"));
    }
    if !is_asset(&relative) {
        return lint.problem(at, format!("url({url}) must be an image or font"));
    }
    if !lint.files.contains(&relative) {
        return lint.problem(
            at,
            format!("url({url}): {relative} doesn't exist in the theme"),
        );
    }
    let encoded: Vec<String> = segments.iter().map(|segment| encode(segment)).collect();
    let replacement = format!(
        "url(\"/{}/{}{fragment}\")",
        encode(lint.id),
        encoded.join("/")
    );
    lint.edits.push((start, end, replacement));
}

fn describe(token: &Token) -> String {
    match token {
        Token::Ident(text) => format!("\"{text}\""),
        Token::Delim(c) => format!("\"{c}\""),
        Token::QuotedString(_) => "a string".to_owned(),
        Token::Number { .. } | Token::Dimension { .. } | Token::Percentage { .. } => {
            "a number".to_owned()
        }
        Token::Function(name) => format!("{name}()"),
        Token::AtKeyword(name) => format!("@{name}"),
        Token::CurlyBracketBlock => "{".to_owned(),
        Token::ParenthesisBlock => "(".to_owned(),
        Token::SquareBracketBlock => "[".to_owned(),
        Token::Colon => "\":\"".to_owned(),
        Token::Semicolon => "\";\"".to_owned(),
        _ => "token".to_owned(),
    }
}

fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}
