//! Messages in the webview's syntax (Lingui's ICU MessageFormat), so the
//! native UI can share the `app` and `ui` domains' msgids and their
//! translations: `{name}` placeholders, `#` for the count in a plural, and
//! `{n, plural, one {…} other {…}}` / `{x, select, a {…} other {…}}`
//! inside a message. Quoting is ICU's: `''` is an apostrophe, and an
//! apostrophe before `{`, `}` (or `#` in a plural) quotes up to the next
//! one; any other apostrophe is itself ("can't").
//!
//! Something that doesn't parse shows as it is, never panics.

use std::fmt::Write as _;

/// A piece of a parsed message.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Text(String),
    /// `#`: the count of the plural around it.
    Hash,
    /// `{name}`.
    Arg(String),
    /// `{name, plural, …}` (with `offset:`), or `{name, select, …}`.
    Choice {
        arg: String,
        plural: bool,
        offset: f64,
        cases: Vec<(String, Vec<Token>)>,
    },
}

/// Parses `source`; a message with a syntax error is all text.
pub fn parse(source: &str) -> Vec<Token> {
    parse_with(source, false)
}

/// Parses one form of a gettext plural (`msgid_plural`), where `#` is the
/// count, as Lingui writes its simple plurals.
pub fn parse_counted(source: &str) -> Vec<Token> {
    parse_with(source, true)
}

fn parse_with(source: &str, counted: bool) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut at = 0;
    match parse_tokens(&chars, &mut at, counted, true) {
        Some(tokens) if at == chars.len() => tokens,
        _ => vec![Token::Text(source.to_owned())],
    }
}

fn parse_tokens(chars: &[char], at: &mut usize, in_plural: bool, top: bool) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut text = String::new();
    while *at < chars.len() {
        let c = chars[*at];
        match c {
            '\'' => {
                let next = chars.get(*at + 1).copied();
                if next == Some('\'') {
                    text.push('\'');
                    *at += 2;
                } else if matches!(next, Some('{' | '}')) || (in_plural && next == Some('#')) {
                    // Quoted up to the next lone apostrophe.
                    *at += 1;
                    while *at < chars.len() {
                        if chars[*at] == '\'' {
                            if chars.get(*at + 1) == Some(&'\'') {
                                text.push('\'');
                                *at += 2;
                                continue;
                            }
                            *at += 1;
                            break;
                        }
                        text.push(chars[*at]);
                        *at += 1;
                    }
                } else {
                    text.push('\'');
                    *at += 1;
                }
            }
            '#' if in_plural => {
                flush(&mut text, &mut tokens);
                tokens.push(Token::Hash);
                *at += 1;
            }
            '{' => {
                flush(&mut text, &mut tokens);
                *at += 1;
                tokens.push(parse_argument(chars, at)?);
            }
            '}' => {
                if top {
                    return None;
                }
                break;
            }
            _ => {
                text.push(c);
                *at += 1;
            }
        }
    }
    flush(&mut text, &mut tokens);
    Some(tokens)
}

fn flush(text: &mut String, tokens: &mut Vec<Token>) {
    if !text.is_empty() {
        tokens.push(Token::Text(std::mem::take(text)));
    }
}

fn skip_space(chars: &[char], at: &mut usize) {
    while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
        *at += 1;
    }
}

fn word(chars: &[char], at: &mut usize) -> String {
    skip_space(chars, at);
    let mut word = String::new();
    while let Some(&c) = chars.get(*at) {
        if c.is_whitespace() || matches!(c, ',' | '{' | '}') {
            break;
        }
        word.push(c);
        *at += 1;
    }
    skip_space(chars, at);
    word
}

/// After `{`: an argument, a plural or a select, up to its `}`.
fn parse_argument(chars: &[char], at: &mut usize) -> Option<Token> {
    let arg = word(chars, at);
    if arg.is_empty() {
        return None;
    }
    match chars.get(*at)? {
        '}' => {
            *at += 1;
            Some(Token::Arg(arg))
        }
        ',' => {
            *at += 1;
            let kind = word(chars, at);
            let plural = match kind.as_str() {
                "plural" | "selectordinal" => true,
                "select" => false,
                // Formats the native UI doesn't take (`{d, date}`): shown as
                // the bare argument.
                _ => {
                    while *chars.get(*at)? != '}' {
                        *at += 1;
                    }
                    *at += 1;
                    return Some(Token::Arg(arg));
                }
            };
            if *chars.get(*at)? != ',' {
                return None;
            }
            *at += 1;
            let mut offset = 0.0;
            let mut cases = Vec::new();
            loop {
                skip_space(chars, at);
                if *chars.get(*at)? == '}' {
                    *at += 1;
                    break;
                }
                let key = word(chars, at);
                if let Some(value) = key.strip_prefix("offset:") {
                    offset = value.parse().ok()?;
                    continue;
                }
                if key.is_empty() || *chars.get(*at)? != '{' {
                    return None;
                }
                *at += 1;
                let tokens = parse_tokens(chars, at, plural, false)?;
                if *chars.get(*at)? != '}' {
                    return None;
                }
                *at += 1;
                cases.push((key, tokens));
            }
            Some(Token::Choice {
                arg,
                plural,
                offset,
                cases,
            })
        }
        _ => None,
    }
}

/// How a language renders: its plural categories and its numbers.
pub trait Locale {
    /// The CLDR plural category of `n`: `one`, `few`, `other`, …
    fn category(&self, n: f64) -> &'static str;
    /// `n` as the language writes a count.
    fn number(&self, n: f64) -> String;
}

/// An argument's value.
pub trait Value {
    fn text(&self) -> String;
    /// As a number, for a plural; `None` for text.
    fn number(&self) -> Option<f64>;
}

impl<T: std::fmt::Display + ?Sized> Value for T {
    fn text(&self) -> String {
        self.to_string()
    }

    fn number(&self) -> Option<f64> {
        self.to_string().trim().parse().ok()
    }
}

/// `tokens` with the arguments filled in. `count` is the `#` outside any
/// plural (a gettext plural's). Text pieces go through `text` (the
/// pseudo-locale's transform, or nothing).
pub fn render(
    tokens: &[Token],
    args: &[(&str, &dyn Value)],
    count: Option<f64>,
    locale: &dyn Locale,
    text: &dyn Fn(&str) -> String,
) -> String {
    let mut out = String::new();
    render_into(&mut out, tokens, args, count, locale, text);
    out
}

fn render_into(
    out: &mut String,
    tokens: &[Token],
    args: &[(&str, &dyn Value)],
    count: Option<f64>,
    locale: &dyn Locale,
    text: &dyn Fn(&str) -> String,
) {
    let value = |name: &str| args.iter().find(|(arg, _)| *arg == name).map(|(_, v)| *v);
    for token in tokens {
        match token {
            Token::Text(piece) => out.push_str(&text(piece)),
            Token::Hash => match count {
                Some(n) => out.push_str(&locale.number(n)),
                None => out.push('#'),
            },
            Token::Arg(name) => match value(name) {
                Some(value) => out.push_str(&value.text()),
                None => {
                    let _ = write!(out, "{{{name}}}");
                }
            },
            Token::Choice {
                arg,
                plural,
                offset,
                cases,
            } => {
                let Some(value) = value(arg) else {
                    let _ = write!(out, "{{{arg}}}");
                    continue;
                };
                let case = if *plural {
                    let n = value.number().unwrap_or(0.0);
                    let exact = format!("={}", trim_number(n));
                    let category = locale.category(n - offset);
                    let chosen = cases
                        .iter()
                        .find(|(key, _)| *key == exact)
                        .or_else(|| cases.iter().find(|(key, _)| key == category))
                        .or_else(|| cases.iter().find(|(key, _)| key == "other"));
                    chosen.map(|(_, tokens)| (tokens, Some(n - offset)))
                } else {
                    let key = value.text();
                    cases
                        .iter()
                        .find(|(case, _)| *case == key)
                        .or_else(|| cases.iter().find(|(case, _)| case == "other"))
                        .map(|(_, tokens)| (tokens, count))
                };
                if let Some((tokens, hash)) = case {
                    render_into(out, tokens, args, hash, locale, text);
                }
            }
        }
    }
}

fn trim_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

/// The argument names a message uses, for checking a translation keeps
/// them.
pub fn arguments(tokens: &[Token]) -> Vec<String> {
    let mut names = Vec::new();
    collect(tokens, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect(tokens: &[Token], names: &mut Vec<String>) {
    for token in tokens {
        match token {
            Token::Arg(name) => names.push(name.clone()),
            Token::Choice { arg, cases, .. } => {
                names.push(arg.clone());
                for (_, tokens) in cases {
                    collect(tokens, names);
                }
            }
            Token::Text(_) | Token::Hash => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct English;

    impl Locale for English {
        fn category(&self, n: f64) -> &'static str {
            if n == 1.0 { "one" } else { "other" }
        }

        fn number(&self, n: f64) -> String {
            trim_number(n)
        }
    }

    fn show(source: &str, args: &[(&str, &dyn Value)], count: Option<f64>) -> String {
        let tokens = if count.is_some() {
            parse_counted(source)
        } else {
            parse(source)
        };
        render(&tokens, args, count, &English, &|text| text.to_owned())
    }

    #[test]
    fn fills_arguments_plurals_and_selects() {
        assert_eq!(
            show("Stop {title}", &[("title", &"Celeste")], None),
            "Stop Celeste"
        );
        assert_eq!(show("# games", &[], Some(3.0)), "3 games");
        let message = "{owned, plural, one {# game owned} other {# games owned}}, {installed} installed. Last refreshed {0}.";
        assert_eq!(
            show(
                message,
                &[("owned", &1), ("installed", &4), ("0", &"today")],
                None
            ),
            "1 game owned, 4 installed. Last refreshed today."
        );
        assert_eq!(
            show(
                message,
                &[("owned", &12), ("installed", &4), ("0", &"today")],
                None
            ),
            "12 games owned, 4 installed. Last refreshed today."
        );
        let exact = "{n, plural, =0 {None} one {One} other {# of them}}";
        assert_eq!(show(exact, &[("n", &0)], None), "None");
        assert_eq!(show(exact, &[("n", &5)], None), "5 of them");
        let select = "{kind, select, game {A game} other {Something}}";
        assert_eq!(show(select, &[("kind", &"game")], None), "A game");
        assert_eq!(show(select, &[("kind", &"tool")], None), "Something");
    }

    #[test]
    fn quotes_as_icu_does_and_keeps_bad_messages_as_text() {
        assert_eq!(
            show("can't stop {it}", &[("it", &"me")], None),
            "can't stop me"
        );
        assert_eq!(show("'{literal}' and ''", &[], None), "{literal} and '");
        assert_eq!(show("{unclosed", &[], None), "{unclosed");
        assert_eq!(show("a } b", &[], None), "a } b");
        // A placeholder without its argument shows as typed.
        assert_eq!(show("Hi {name}", &[], None), "Hi {name}");
        // `#` outside a plural is a character.
        assert_eq!(show("Track #2", &[], None), "Track #2");
        assert_eq!(
            arguments(&parse("{a} {n, plural, one {# {b}} other {{c}}}")),
            ["a", "b", "c", "n"]
        );
    }
}
