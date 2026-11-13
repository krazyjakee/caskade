//! The lint for text in `rsx!` calls that isn't translated: a check, not
//! extraction.

use super::{
    ExtractError, each_source,
    scan::{literal_text, test_module},
};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::path::Path;
use std::str::FromStr;

/// Attributes whose text people read (or hear): a literal there must be a
/// translated message.
const SHOWN_ATTRIBUTES: &[&str] = &[
    "aria_label",
    "aria-label",
    "aria_description",
    "aria-description",
    "aria_placeholder",
    "aria-placeholder",
    "aria_roledescription",
    "aria-roledescription",
    "aria_valuetext",
    "aria-valuetext",
    "title",
    "placeholder",
    "alt",
    "label",
];

/// Whether `text` has words: letters outside its `{placeholders}`. A
/// placeholder or a symbol (`{label}`, `⇧`) needs no translation.
fn has_words(text: &str) -> bool {
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            c if depth == 0 && c.is_alphabetic() => return true,
            _ => {}
        }
    }
    false
}

/// The string literals people would see in `rsx!` calls in `text`: text
/// nodes, and the values of [`SHOWN_ATTRIBUTES`], that have words. Text in
/// a translation macro's call is a message, not a literal, so it isn't
/// listed.
pub fn rsx_literals(text: &str) -> Result<Vec<String>, String> {
    let tokens = TokenStream::from_str(text).map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    find_rsx(tokens, &mut found);
    Ok(found)
}

fn find_rsx(tokens: TokenStream, found: &mut Vec<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        let TokenTree::Group(group) = tree else {
            continue;
        };
        if test_module(&trees, at) {
            continue;
        }
        if let [.., TokenTree::Ident(name), TokenTree::Punct(bang)] = &trees[..at]
            && name == "rsx"
            && bang.as_char() == '!'
        {
            rsx_body(group.stream(), found);
            continue;
        }
        find_rsx(group.stream(), found);
    }
}

/// One `rsx!` body (and the element bodies in it).
fn rsx_body(tokens: TokenStream, found: &mut Vec<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    // The attribute whose value the tokens are in, from its `:` to the
    // next `,`.
    let mut value_of: Option<String> = None;
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Punct(punct) if punct.as_char() == ',' => value_of = None,
            TokenTree::Punct(punct) if punct.as_char() == ':' => {
                value_of = match at.checked_sub(1).map(|at| &trees[at]) {
                    Some(TokenTree::Ident(name)) => Some(name.to_string()),
                    Some(TokenTree::Literal(name)) => literal_text(name),
                    _ => value_of.take(),
                };
            }
            TokenTree::Literal(literal) => {
                let Some(text) = literal_text(literal) else {
                    continue;
                };
                // An attribute's name (`"data-cs": …`).
                if matches!(trees.get(at + 1), Some(TokenTree::Punct(colon)) if colon.as_char() == ':')
                {
                    continue;
                }
                let shown = match &value_of {
                    Some(name) => SHOWN_ATTRIBUTES.contains(&name.as_str()),
                    // A text node: first, or after another child.
                    None => match at.checked_sub(1).map(|at| &trees[at]) {
                        None | Some(TokenTree::Group(_) | TokenTree::Literal(_)) => true,
                        Some(TokenTree::Punct(comma)) => comma.as_char() == ',',
                        Some(TokenTree::Ident(_)) => false,
                    },
                };
                if shown && has_words(&text) {
                    found.push(text);
                }
            }
            TokenTree::Group(group) => {
                let after_bang = matches!(
                    at.checked_sub(1).map(|at| &trees[at]),
                    Some(TokenTree::Punct(bang)) if bang.as_char() == '!'
                );
                let closure = matches!(
                    at.checked_sub(1).map(|at| &trees[at]),
                    Some(TokenTree::Punct(bar)) if bar.as_char() == '|'
                );
                if let Some(name) = &value_of {
                    // An attribute's value: shown text if the attribute is
                    // (`title: if a { "One" } else { "Other" }`).
                    if SHOWN_ATTRIBUTES.contains(&name.as_str()) && !after_bang {
                        value_literals(group.stream(), found);
                    } else {
                        find_rsx(group.stream(), found);
                    }
                } else if after_bang || closure || group.delimiter() == Delimiter::Parenthesis {
                    // A macro's call (a translation macro's text is a
                    // message), a closure or a call's arguments: code.
                    find_rsx(group.stream(), found);
                } else {
                    // An element's, an `if`'s or a `for`'s body.
                    rsx_body(group.stream(), found);
                }
            }
            _ => {}
        }
    }
}

/// The literals with words in a shown attribute's value, outside macro
/// calls.
fn value_literals(tokens: TokenStream, found: &mut Vec<String>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        match tree {
            TokenTree::Literal(literal) => {
                if let Some(text) = literal_text(literal)
                    && has_words(&text)
                {
                    found.push(text);
                }
            }
            TokenTree::Group(group) => {
                if matches!(
                    at.checked_sub(1).map(|at| &trees[at]),
                    Some(TokenTree::Punct(bang)) if bang.as_char() == '!'
                ) {
                    find_rsx(group.stream(), found);
                } else {
                    value_literals(group.stream(), found);
                }
            }
            _ => {}
        }
    }
}

/// Every `.rs` file under `sources` (relative to `root`) but tests, with
/// the user-visible string literals in its `rsx!` calls.
pub fn untranslated_rsx(
    root: &Path,
    sources: &[&str],
) -> Result<Vec<(String, String)>, ExtractError> {
    let mut found = Vec::new();
    each_source(root, sources, true, |reference, text| {
        let literals = rsx_literals(text)
            .map_err(|error| ExtractError::Source(reference.to_owned(), error))?;
        found.extend(
            literals
                .into_iter()
                .map(|literal| (reference.to_owned(), literal)),
        );
        Ok(())
    })?;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_rsx_text_that_isnt_translated() {
        let found = rsx_literals(
            r##"
            fn view() -> Element {
                rsx! {
                    div { "data-cs": "panel", class: "wide", style: "flex: 1;",
                        "Untranslated text"
                        h2 { "{title}" }
                        p { {t!("Translated")} }
                        button { title: "Untranslated title", aria_label: t!("Fine"), "⇧" }
                        img { alt: if big { "Big picture" } else { "{name}" } }
                        span { "data-focused": if on { "true" }, key: "{c}", onclick: move |_| log("clicked"), "{n} · {m}" }
                        if empty { "Nothing here" }
                        for item in items { li { "{item}" } }
                    }
                }
            }
            "##,
        )
        .unwrap();
        assert_eq!(
            found,
            [
                "Untranslated text",
                "Untranslated title",
                "Big picture",
                "Nothing here"
            ]
        );
    }
}
