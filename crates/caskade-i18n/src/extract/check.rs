//! The translator checks behind `just i18n-check` (`caskade-i18n extract
//! --check`, after the templates and catalogs are found current): every PO
//! file passes GNU gettext's `msgfmt --check`, every translation keeps its
//! English's placeholders and tags, and how much of each language is
//! translated. See `docs/translating.md`.

use super::{DOMAINS, ExtractError, Syntax, languages, read_catalog, sorted_entries};
use crate::message::{self, Token};
use crate::po::{Message, Po};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

/// What a text must keep: placeholder names (`#` for an ICU plural's
/// count) and numbered tags, in any order.
#[derive(Debug, Clone)]
struct Markers {
    placeholders: BTreeSet<String>,
    tags: Vec<String>,
}

/// The placeholders and tags of `text` (a gettext plural's form when
/// `plural`); `None` when ICU text doesn't parse.
fn markers(text: &str, syntax: Syntax, plural: bool) -> Option<Markers> {
    match syntax {
        Syntax::Rust => Some(Markers {
            placeholders: crate::format::placeholders(text)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            tags: Vec::new(),
        }),
        Syntax::Icu => {
            let tokens = message::try_parse(text, plural)?;
            let mut placeholders = BTreeSet::new();
            names(&tokens, plural, &mut placeholders);
            Some(Markers {
                placeholders,
                tags: tags(text),
            })
        }
    }
}

/// Argument names, nested ones too; `#` only in a gettext plural, where
/// it's the count.
fn names(tokens: &[Token], plural: bool, into: &mut BTreeSet<String>) {
    for token in tokens {
        match token {
            Token::Arg(name) => {
                into.insert(name.clone());
            }
            Token::Choice { arg, cases, .. } => {
                into.insert(arg.clone());
                for (_, tokens) in cases {
                    names(tokens, plural, into);
                }
            }
            Token::Hash if plural => {
                into.insert("#".to_owned());
            }
            Token::Hash | Token::Text(_) => {}
        }
    }
}

/// The numbered tags in `text` (`<0>`, `</0>`, `<1/>`), without spaces,
/// sorted.
fn tags(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        match tag_len(rest) {
            Some(len) => {
                found.push(rest[..len].split_whitespace().collect::<String>());
                rest = &rest[len..];
            }
            None => rest = &rest[1..],
        }
    }
    found.sort();
    found
}

/// The length of the tag at the start of `text`, which starts with `<`.
fn tag_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut at = 1;
    if bytes.get(at) == Some(&b'/') {
        at += 1;
    }
    let digits = bytes
        .get(at..)?
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    at += digits;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if bytes.get(at) == Some(&b'/') {
        at += 1;
    }
    (bytes.get(at) == Some(&b'>')).then_some(at + 1)
}

fn show(name: &str) -> String {
    if name == "#" {
        "#".to_owned()
    } else {
        format!("{{{name}}}")
    }
}

fn list<'a>(names: impl IntoIterator<Item = &'a String>) -> String {
    names
        .into_iter()
        .map(|name| show(name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn tag_list(tags: &[String]) -> String {
    if tags.is_empty() {
        "none".to_owned()
    } else {
        tags.join(" ")
    }
}

/// What's wrong with a message's translations, if anything. Each non-empty
/// form keeps the English's placeholders (none dropped, none added or
/// renamed) and its tags. A plural's forms are checked against `msgid` and
/// `msgid_plural`: each may use the placeholders of either and must keep
/// those both have, except the count, and the last (the language's most
/// general form) must keep every one `msgid_plural` has, the count
/// included. Fuzzy and untranslated messages show English, so pass;
/// English that doesn't parse is the code's to fix, not the translator's.
pub fn entry_problems(message: &Message, syntax: Syntax) -> Vec<String> {
    if message.is_fuzzy() {
        return Vec::new();
    }
    let plural = message.plural.is_some();
    let Some(one) = markers(&message.id, syntax, plural) else {
        return Vec::new();
    };
    let other = match &message.plural {
        Some(english) => match markers(english, syntax, true) {
            Some(other) => other,
            None => return Vec::new(),
        },
        None => one.clone(),
    };
    let allowed: BTreeSet<&String> = one.placeholders.iter().chain(&other.placeholders).collect();
    // The count (ICU's `#`, the core's `{n}`) may be left out of a
    // singular form.
    let count = match syntax {
        Syntax::Rust => "n",
        Syntax::Icu => "#",
    };
    let shared: BTreeSet<String> = one
        .placeholders
        .iter()
        .filter(|name| *name != count && other.placeholders.contains(*name))
        .cloned()
        .collect();
    let mut problems = Vec::new();
    for (index, text) in message.translations.iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let form = if plural {
            format!("msgstr[{index}]")
        } else {
            "msgstr".to_owned()
        };
        let Some(found) = markers(text, syntax, plural) else {
            problems.push(format!("{form} doesn't parse as a message"));
            continue;
        };
        let last = index + 1 == message.translations.len();
        let required = if plural && last {
            &other.placeholders
        } else if plural {
            &shared
        } else {
            &one.placeholders
        };
        let dropped: Vec<&String> = required
            .iter()
            .filter(|name| !found.placeholders.contains(*name))
            .collect();
        let added: Vec<&String> = found
            .placeholders
            .iter()
            .filter(|name| !allowed.contains(name))
            .collect();
        if !dropped.is_empty() {
            problems.push(format!("{form} drops {}", list(dropped)));
        }
        if !added.is_empty() {
            problems.push(format!(
                "{form} has {}, which the English doesn't",
                list(added)
            ));
        }
        if found.tags != one.tags && found.tags != other.tags {
            problems.push(format!(
                "{form} has the tags {}, the English {}",
                tag_list(&found.tags),
                tag_list(&other.tags)
            ));
        }
    }
    problems
}

/// `msgctxt "…" msgid "…"`, shortened, to name an entry.
pub fn describe(message: &Message) -> String {
    let short = |text: &str| {
        if text.chars().count() > 60 {
            format!(
                "{:?}",
                format!("{}...", text.chars().take(57).collect::<String>())
            )
        } else {
            format!("{text:?}")
        }
    };
    match &message.context {
        Some(context) => format!("msgctxt {} msgid {}", short(context), short(&message.id)),
        None => format!("msgid {}", short(&message.id)),
    }
}

/// The placeholder and tag problems in a catalog, one line each:
/// `<file>: <entry>: <problem>`.
pub fn catalog_problems(file: &str, po: &Po, syntax: Syntax) -> Vec<String> {
    po.messages
        .iter()
        .flat_map(|message| {
            entry_problems(message, syntax)
                .into_iter()
                .map(move |problem| format!("{file}: {}: {problem}", describe(message)))
        })
        .collect()
}

/// `msgfmt --check` on a PO file (relative to `root`): what it reports
/// (`<file>:<line>: …`), nothing when the file is valid; its warnings
/// (header fields a hand-started file lacks) don't count. `Err` when GNU
/// gettext isn't installed.
pub fn msgfmt_problems(root: &Path, file: &str) -> Result<Vec<String>, String> {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = Command::new("msgfmt")
        .arg("--check")
        .arg(format!("--output-file={null}"))
        .arg(file)
        .current_dir(root)
        .stdin(Stdio::null())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(
                "msgfmt not found: install GNU gettext (`gettext` on Linux and Homebrew, MSYS2 on Windows)"
                    .to_owned(),
            );
        }
        Err(error) => return Err(format!("msgfmt: {error}")),
    };
    if output.status.success() {
        return Ok(Vec::new());
    }
    let problems: Vec<String> = String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.contains("warning:"))
        .map(str::to_owned)
        .collect();
    Ok(if problems.is_empty() {
        vec![format!("{file}: msgfmt --check failed")]
    } else {
        problems
    })
}

/// Plugins' and themes' catalogs (`<folder>/locales/<lang>.po`, #219),
/// relative to `root`, sorted.
pub fn package_catalogs(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &str, depth: usize, found: &mut Vec<String>) {
        if depth > 4 {
            return;
        }
        let Ok(entries) = sorted_entries(&root.join(dir)) else {
            return;
        };
        for path in entries {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let relative = format!("{dir}/{name}");
            if path.is_dir() {
                if name != "node_modules" && name != "target" {
                    walk(root, &relative, depth + 1, found);
                }
            } else if name.ends_with(".po") && dir.ends_with("locales") {
                found.push(relative);
            }
        }
    }
    let mut found = Vec::new();
    walk(root, "plugins", 0, &mut found);
    walk(root, "themes", 0, &mut found);
    found.sort();
    found
}

/// Every problem `just i18n-check` reports past stale files: `msgfmt
/// --check` on each language's catalogs and on plugins' and themes', and
/// the placeholder and tag checks on the former. (Plugins' and themes'
/// catalogs get those once their domains' syntax is settled, #219.) `Err`
/// when msgfmt isn't installed or a catalog can't be read.
pub fn check_catalogs(root: &Path) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    for language in languages(root).map_err(|error| error.to_string())? {
        for domain in DOMAINS {
            let path = domain.catalog_path(root, &language);
            if !path.is_file() {
                continue;
            }
            let file = format!("locales/{language}/{}.po", domain.name);
            problems.extend(msgfmt_problems(root, &file)?);
            match read_catalog(&path) {
                Ok(Some(po)) => problems.extend(catalog_problems(&file, &po, domain.syntax)),
                Ok(None) => {}
                Err(error) => problems.push(error.to_string()),
            }
        }
    }
    for file in package_catalogs(root) {
        problems.extend(msgfmt_problems(root, &file)?);
    }
    Ok(problems)
}

/// One language's translated messages in one domain, over the template's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub domain: &'static str,
    pub translated: usize,
    pub total: usize,
}

/// Each translated language's completion by domain, from the templates and
/// catalogs on disk. A message counts when every form is translated and
/// it isn't fuzzy; obsolete entries and messages no template has don't.
pub fn completion(root: &Path) -> Result<Vec<(String, Vec<Completion>)>, ExtractError> {
    let mut templates = Vec::new();
    for domain in DOMAINS {
        let keys: HashSet<(Option<String>, String)> = read_catalog(&domain.template_path(root))?
            .map(|po| {
                po.messages
                    .iter()
                    .map(|message| (message.context.clone(), message.id.clone()))
                    .collect()
            })
            .unwrap_or_default();
        templates.push(keys);
    }
    let mut result = Vec::new();
    for language in languages(root)? {
        let mut domains = Vec::new();
        for (domain, template) in DOMAINS.iter().zip(&templates) {
            let translated = read_catalog(&domain.catalog_path(root, &language))?
                .map(|po| {
                    po.messages
                        .iter()
                        .filter(|message| {
                            !message.is_fuzzy()
                                && !message.translations.is_empty()
                                && message.translations.iter().all(|form| !form.is_empty())
                                && template.contains(&(message.context.clone(), message.id.clone()))
                        })
                        .count()
                })
                .unwrap_or(0);
            domains.push(Completion {
                domain: domain.name,
                translated,
                total: template.len(),
            });
        }
        result.push((language, domains));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problems(syntax: Syntax, entry: &str) -> Vec<String> {
        let po = Po::parse(&format!(
            "msgid \"\"\nmsgstr \"Plural-Forms: nplurals=2; plural=(n != 1);\\n\"\n\n{entry}"
        ))
        .unwrap();
        entry_problems(&po.messages[0], syntax)
    }

    #[test]
    fn translations_keep_placeholders() {
        let ok = "msgid \"Stop {title}\"\nmsgstr \"Stoppa {title}\"\n";
        assert!(problems(Syntax::Icu, ok).is_empty());
        let dropped = problems(Syntax::Icu, "msgid \"Stop {title}\"\nmsgstr \"Stoppa\"\n");
        assert_eq!(dropped, ["msgstr drops {title}"]);
        let renamed = problems(
            Syntax::Icu,
            "msgid \"Stop {title}\"\nmsgstr \"Stoppa {titel}\"\n",
        );
        assert_eq!(
            renamed,
            [
                "msgstr drops {title}",
                "msgstr has {titel}, which the English doesn't"
            ]
        );
        // The core's `{{` is a brace, not a placeholder.
        let braces = "msgid \"Use {{name}}\"\nmsgstr \"Använd {{name}}\"\n";
        assert!(problems(Syntax::Rust, braces).is_empty());
        let rust = problems(
            Syntax::Rust,
            "msgid \"Open {path}\"\nmsgstr \"Öppna {folder}\"\n",
        );
        assert_eq!(rust.len(), 2, "{rust:?}");
    }

    #[test]
    fn plural_forms_may_leave_the_count_out_but_the_last() {
        let ok = "msgid \"# game\"\nmsgid_plural \"# games\"\nmsgstr[0] \"Ett spel\"\nmsgstr[1] \"# spel\"\n";
        assert!(problems(Syntax::Icu, ok).is_empty());
        let last = "msgid \"# game\"\nmsgid_plural \"# games\"\nmsgstr[0] \"# spel\"\nmsgstr[1] \"Spel\"\n";
        assert_eq!(problems(Syntax::Icu, last), ["msgstr[1] drops #"]);
        let core = "msgid \"One game\"\nmsgid_plural \"{n} games\"\nmsgstr[0] \"Ett spel\"\nmsgstr[1] \"{n} spel\"\n";
        assert!(problems(Syntax::Rust, core).is_empty());
    }

    #[test]
    fn translations_keep_tags_and_parse() {
        let ok = "msgid \"Go to <0>Library</0>\"\nmsgstr \"Gå till <0 >Bibliotek</0>\"\n";
        assert!(problems(Syntax::Icu, ok).is_empty());
        let lost = problems(
            Syntax::Icu,
            "msgid \"Go to <0>Library</0>\"\nmsgstr \"Gå till Bibliotek\"\n",
        );
        assert_eq!(lost, ["msgstr has the tags none, the English </0> <0>"]);
        let broken = problems(
            Syntax::Icu,
            "msgid \"Stop {title}\"\nmsgstr \"Stoppa {title\"\n",
        );
        assert_eq!(broken, ["msgstr doesn't parse as a message"]);
        // Fuzzy entries show English.
        let fuzzy = "#, fuzzy\nmsgid \"Stop {title}\"\nmsgstr \"Stoppa\"\n";
        assert!(problems(Syntax::Icu, fuzzy).is_empty());
        // A nested plural's names count.
        let nested = "msgid \"{n, plural, one {# game} other {# games}} by {who}\"\nmsgstr \"{n, plural, one {# spel} other {# spel}} av {who}\"\n";
        assert!(problems(Syntax::Icu, nested).is_empty());
    }

    #[test]
    fn tags_are_numbered_only() {
        assert_eq!(
            tags("a <0>b</0> <1/> <b>c</b> < 2>"),
            ["</0>", "<0>", "<1/>"]
        );
    }

    #[test]
    fn describes_entries_shortly() {
        let mut message = Message::new(Some("verb"), &"x".repeat(80), None);
        assert_eq!(
            describe(&message),
            format!("msgctxt \"verb\" msgid \"{}...\"", "x".repeat(57))
        );
        message.context = None;
        message.id = "Open".to_owned();
        assert_eq!(describe(&message), "msgid \"Open\"");
    }
}
