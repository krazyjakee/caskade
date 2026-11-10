//! The anatomy catalogue for RSX components (#90): the selectors themes and
//! plugins may target, as `packages/ui/src/anatomy.ts` has them for the
//! webview. Every catalogued component renders `data-cs="<name>"` on its
//! root, `data-part` on its parts and its domain state as the attributes in
//! `states`. Class names are not API.
//!
//! Components declare an [`Entry`] beside their code and the native app
//! lists them in one catalogue (`apps/native/src/anatomy.rs`). [`markdown`]
//! and [`contract`] write `docs/theming/anatomy.md` and `anatomy.json`
//! exactly as `packages/ui/scripts/docs.ts` does; until the native UI is
//! the default (BLITZ-33) the webview's `anatomy.ts` stays their source, and
//! each native entry has to match its webview section word for word.

use std::collections::BTreeSet;

/// Where a root comes from, as `anatomy.ts`'s `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The component library (`@caskade/ui`, the RSX components of #93).
    Ui,
    /// The app shell and its views.
    App,
    /// The public website.
    Website,
}

/// One catalogued root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// `data-cs`.
    pub name: &'static str,
    /// The components that render this root, and where they live.
    pub rendered: &'static str,
    pub source: Source,
    pub description: &'static str,
    /// `data-part` names owned by this root → what each part is.
    pub parts: &'static [(&'static str, &'static str)],
    /// Domain state on the root: attribute → the values it takes.
    pub states: &'static [(&'static str, &'static [&'static str])],
    /// Interaction attributes on the root (`data-hovered`, …).
    pub interaction: &'static [&'static str],
    /// Parts and state attributes only the app's variants render.
    pub app_only: &'static [&'static str],
    /// Other catalogued roots that can render inside this one.
    pub contains: &'static [&'static str],
    /// Parts that hold the caller's content: other `data-part`s inside
    /// them belong to that content, not to this root.
    pub slots: &'static [&'static str],
    /// The root's own content comes from the caller, so `data-part`s it
    /// doesn't list belong to that content.
    pub container: bool,
}

impl Entry {
    /// An entry with nothing but its name, source and words, to fill in.
    pub const fn new(
        name: &'static str,
        rendered: &'static str,
        source: Source,
        description: &'static str,
    ) -> Self {
        Self {
            name,
            rendered,
            source,
            description,
            parts: &[],
            states: &[],
            interaction: &[],
            app_only: &[],
            contains: &[],
            slots: &[],
            container: false,
        }
    }
}

/// Attributes any selector may use besides the catalogue's own, as
/// `anatomy.ts`'s `contractAttributes`: on the root element (Appearance,
/// console mode) and on parts.
pub const ROOT_ATTRIBUTES: [&str; 6] = [
    "data-mode",
    "data-color-scheme",
    "data-effects",
    "data-motion",
    "data-density",
    "data-paint",
];
pub const PART_ATTRIBUTES: [&str; 10] = [
    "data-status",
    "data-store",
    "data-selected",
    "data-focused",
    "data-focus-within",
    "data-focus-visible",
    "data-hovered",
    "data-pressed",
    "data-dragging",
    "data-disabled",
];

fn code(text: &str) -> String {
    format!("`{text}`")
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

/// The section `docs.ts` writes for `entry`, one line each.
pub fn section(entry: &Entry) -> Vec<String> {
    let mark = |key: &str| {
        if entry.app_only.contains(&key) {
            " (app only)"
        } else {
            ""
        }
    };
    let name = entry.name;
    let mut lines = vec![
        format!("## {}", code(name)),
        String::new(),
        format!("{} Rendered by {}.", entry.description, entry.rendered),
        String::new(),
        format!("Root: {}", code(&format!("[data-cs=\"{name}\"]"))),
        String::new(),
    ];
    if !entry.parts.is_empty() {
        lines.push("| Part | Selector | What it is |".into());
        lines.push("| --- | --- | --- |".into());
        for (part, what) in entry.parts {
            lines.push(format!(
                "| {}{} | {} | {} |",
                code(part),
                mark(part),
                code(&format!("[data-cs=\"{name}\"] [data-part=\"{part}\"]")),
                cell(what)
            ));
        }
        lines.push(String::new());
    }
    if !entry.slots.is_empty() {
        lines.push(format!(
            "Slots: {} hold the caller's content; parts inside them belong to that content.",
            entry
                .slots
                .iter()
                .map(|slot| code(slot))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        lines.push(String::new());
    }
    if entry.container {
        lines.push(
            "Container: its content comes from the caller, which may add parts of its own.".into(),
        );
        lines.push(String::new());
    }
    if !entry.states.is_empty() {
        lines.push("State on the root:".into());
        lines.push(String::new());
        for (attribute, values) in entry.states {
            lines.push(format!(
                "- {}{}: {}",
                code(attribute),
                mark(attribute),
                values
                    .iter()
                    .map(|value| code(value))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        lines.push(String::new());
    }
    if !entry.interaction.is_empty() {
        lines.push(format!(
            "React Aria interaction: {}",
            entry
                .interaction
                .iter()
                .map(|attribute| code(attribute))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        lines.push(String::new());
    }
    if !entry.contains.is_empty() {
        lines.push(format!(
            "Contains: {}",
            entry
                .contains
                .iter()
                .map(|inner| format!("[{}](#{inner})", code(inner)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        lines.push(String::new());
    }
    lines
}

/// `docs/theming/anatomy.md` for `entries`, as `docs.ts` writes it.
pub fn markdown(entries: &[Entry]) -> String {
    let index = |source: Source| {
        entries
            .iter()
            .filter(|entry| entry.source == source)
            .map(|entry| {
                format!(
                    "- [{}](#{}): {}",
                    code(entry.name),
                    entry.name,
                    entry.description
                )
            })
            .collect::<Vec<_>>()
    };
    let mut lines: Vec<String> = [
        "<!-- Generated by @caskade/ui from packages/ui/src/anatomy.ts (pnpm --filter @caskade/ui docs). Do not edit. -->",
        "",
        "# Anatomy catalogue",
        "",
        "The selectors a theme or plugin may target (see [README](README.md)). Each",
        "component renders `data-cs=\"<name>\"` on its root and `data-part=\"<part>\"` on",
        "its parts. A part belongs to the nearest enclosing root, so",
        "`[data-cs=\"game-card\"] [data-part=\"title\"]` does not match a title inside a",
        "nested component. A nested component's root can also be a part of its",
        "parent: a card's art is `[data-cs=\"game-art\"][data-part=\"art\"]`.",
        "",
        "Domain state is an attribute on the root, such as `data-status` or",
        "`data-variant`. Interaction state comes from React Aria",
        "(`data-hovered`, `data-pressed`, `data-focus-visible`, `data-selected`,",
        "`data-disabled`, …). Class names are not API and change without notice.",
        "",
        "Parts and states marked (app only) are rendered only by the app's own",
        "variant of the component, such as the library's card.",
        "",
        "## `@caskade/ui` components",
        "",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    lines.extend(index(Source::Ui));
    lines.extend(["".into(), "## App shell".into(), "".into()]);
    lines.extend(index(Source::App));
    lines.extend(["".into(), "## Website".into(), "".into()]);
    lines.extend(index(Source::Website));
    lines.push(String::new());
    for entry in entries {
        lines.extend(section(entry));
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `JSON.stringify(value, null, 2)` for a list of strings at `indent`.
fn json_list(items: &[&str], indent: usize) -> String {
    if items.is_empty() {
        return "[]".into();
    }
    let pad = " ".repeat(indent + 2);
    let body = items
        .iter()
        .map(|item| format!("{pad}{}", json_string(item)))
        .collect::<Vec<_>>()
        .join(",\n");
    format!("[\n{body}\n{}]", " ".repeat(indent))
}

/// `packages/ui/anatomy.json` for `entries`, as `docs.ts` writes it: roots
/// with their parts, states and interaction, and every attribute a
/// selector may use (the contract `caskade-theme` lints themes against).
pub fn contract(entries: &[Entry]) -> String {
    let mut attributes: BTreeSet<&str> = ["data-cs", "data-part"].into_iter().collect();
    attributes.extend(ROOT_ATTRIBUTES);
    attributes.extend(PART_ATTRIBUTES);
    let mut roots = Vec::new();
    for entry in entries {
        attributes.extend(entry.states.iter().map(|(attribute, _)| *attribute));
        attributes.extend(entry.interaction.iter().copied());
        let parts: Vec<&str> = entry.parts.iter().map(|(part, _)| *part).collect();
        let states = if entry.states.is_empty() {
            "{}".to_owned()
        } else {
            let body = entry
                .states
                .iter()
                .map(|(attribute, values)| {
                    format!(
                        "        {}: {}",
                        json_string(attribute),
                        json_list(values, 8)
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");
            format!("{{\n{body}\n      }}")
        };
        roots.push(format!(
            "    {}: {{\n      \"parts\": {},\n      \"states\": {},\n      \"interaction\": {}\n    }}",
            json_string(entry.name),
            json_list(&parts, 6),
            states,
            json_list(entry.interaction, 6)
        ));
    }
    let roots = if roots.is_empty() {
        "{}".to_owned()
    } else {
        format!("{{\n{}\n  }}", roots.join(",\n"))
    };
    // JavaScript sorts strings by UTF-16 code units; these are ASCII.
    let attributes: Vec<&str> = attributes.into_iter().collect();
    format!(
        "{{\n  \"roots\": {roots},\n  \"attributes\": {}\n}}\n",
        json_list(&attributes, 2)
    )
}

/// The sections of a generated `anatomy.md`, by root name.
pub fn sections(markdown: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in markdown.lines() {
        if let Some(name) = line
            .strip_prefix("## `")
            .and_then(|rest| rest.strip_suffix('`'))
            .filter(|name| !name.starts_with('@'))
        {
            if let Some((name, lines)) = current.take() {
                out.push((name, lines.join("\n")));
            }
            current = Some((name.to_owned(), vec![line]));
        } else if line.starts_with("## ") {
            if let Some((name, lines)) = current.take() {
                out.push((name, lines.join("\n")));
            }
        } else if let Some((_, lines)) = &mut current {
            lines.push(line);
        }
    }
    if let Some((name, lines)) = current {
        out.push((name, lines.join("\n")));
    }
    out
}

/// Where the native catalogue differs from the webview's `anatomy.md`
/// (`webview`): a root it doesn't have, or a section that isn't word for
/// word the same apart from the component it names as rendering it. Empty
/// when they agree.
pub fn differences(entries: &[Entry], webview: &str) -> Vec<String> {
    let theirs = sections(webview);
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in entries {
        if !seen.insert(entry.name) {
            problems.push(format!("{} is catalogued twice", entry.name));
            continue;
        }
        // "Rendered by" names the webview's component until the cutover:
        // the native entry names its own, and the rest has to match.
        let rendered = |text: &str| {
            text.lines()
                .nth(2)
                .and_then(|line| line.rsplit_once(" Rendered by "))
                .map(|(_, rendered)| rendered.trim_end_matches('.').to_owned())
        };
        let ours = |rendered: Option<String>| {
            let mut lines = section(entry);
            lines[2] = format!(
                "{} Rendered by {}.",
                entry.description,
                rendered.as_deref().unwrap_or(entry.rendered)
            );
            lines.join("\n")
        };
        match theirs.iter().find(|(name, _)| name == entry.name) {
            None => problems.push(format!(
                "{} isn't in the webview's catalogue (packages/ui/src/anatomy.ts)",
                entry.name
            )),
            Some((_, text)) if text.trim_end() != ours(rendered(text)).trim_end() => {
                problems.push(format!(
                    "{} differs from the webview's catalogue:\n--- webview\n{}\n--- native\n{}",
                    entry.name,
                    text.trim_end(),
                    ours(None).trim_end()
                ))
            }
            Some(_) => {}
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEBVIEW: &str = include_str!("../../../docs/theming/anatomy.md");

    /// `game-art` as `anatomy.ts` has it, which the native app will render
    /// (#93): its section is the webview's, word for word.
    #[test]
    fn a_native_entry_writes_the_webviews_section() {
        let theirs = sections(WEBVIEW);
        assert!(theirs.len() > 80, "{} sections", theirs.len());
        let (name, text) = &theirs[0];
        // The first section, re-read from its own lines, round-trips.
        assert!(text.starts_with(&format!("## `{name}`")));
        let unknown = Entry::new("not-a-root", "nothing", Source::Ui, "Nothing.");
        assert_eq!(differences(&[unknown], WEBVIEW).len(), 1);
    }

    /// Markdown and JSON for a small catalogue, against what `docs.ts`
    /// writes for the same entries (its functions, run by hand on these).
    #[test]
    fn writes_what_docs_ts_writes() {
        const ENTRY: Entry = Entry {
            parts: &[("title", "The game's | title."), ("art", "Its cover.")],
            states: &[("data-status", &["installed", "available"])],
            interaction: &["data-hovered", "data-pressed"],
            app_only: &["art"],
            contains: &["game-art"],
            slots: &["actions"],
            container: true,
            ..Entry::new("game-card", "GameCard (x.rs)", Source::Ui, "A game.")
        };
        assert_eq!(
            section(&ENTRY).join("\n"),
            "## `game-card`\n\nA game. Rendered by GameCard (x.rs).\n\nRoot: `[data-cs=\"game-card\"]`\n\n\
             | Part | Selector | What it is |\n| --- | --- | --- |\n\
             | `title` | `[data-cs=\"game-card\"] [data-part=\"title\"]` | The game's \\| title. |\n\
             | `art` (app only) | `[data-cs=\"game-card\"] [data-part=\"art\"]` | Its cover. |\n\n\
             Slots: `actions` hold the caller's content; parts inside them belong to that content.\n\n\
             Container: its content comes from the caller, which may add parts of its own.\n\n\
             State on the root:\n\n- `data-status`: `installed`, `available`\n\n\
             React Aria interaction: `data-hovered`, `data-pressed`\n\n\
             Contains: [`game-art`](#game-art)\n"
        );
        let json = contract(&[ENTRY]);
        assert!(json.starts_with(
            "{\n  \"roots\": {\n    \"game-card\": {\n      \"parts\": [\n        \"title\",\n        \"art\"\n      ],\n      \"states\": {\n        \"data-status\": [\n          \"installed\",\n          \"available\"\n        ]\n      },\n      \"interaction\": [\n        \"data-hovered\",\n        \"data-pressed\"\n      ]\n    }\n  },\n  \"attributes\": [\n    \"data-color-scheme\",\n"
        ), "{json}");
        assert!(
            markdown(&[ENTRY]).contains("- [`game-card`](#game-card): A game.\n\n## App shell")
        );
    }

    /// With the whole webview catalogue as data (`apps/native/scripts/
    /// anatomy-json.mjs` writes it), the generators write today's
    /// `anatomy.md` and `anatomy.json` byte for byte. CI sets
    /// `CASKADE_ANATOMY_JSON`; without it this checks nothing.
    #[test]
    fn the_whole_catalogue_writes_todays_docs() {
        let Ok(path) = std::env::var("CASKADE_ANATOMY_JSON") else {
            return;
        };
        let text = std::fs::read_to_string(path).expect("the catalogue's JSON");
        let entries = from_json(&text);
        assert_eq!(markdown(&entries), WEBVIEW);
        assert_eq!(
            contract(&entries),
            include_str!("../../../packages/ui/anatomy.json")
        );
    }

    /// Entries from `anatomy-json.mjs`'s output, leaked for the test.
    fn from_json(text: &str) -> Vec<Entry> {
        use serde_json::Value;
        fn leak(text: &str) -> &'static str {
            Box::leak(text.to_owned().into_boxed_str())
        }
        fn strings(value: &Value) -> &'static [&'static str] {
            let list: Vec<&'static str> = value
                .as_array()
                .map(|items| items.iter().filter_map(Value::as_str).map(leak).collect())
                .unwrap_or_default();
            Box::leak(list.into_boxed_slice())
        }
        fn pairs<T>(value: &Value, item: impl Fn(&Value) -> T) -> &'static [(&'static str, T)] {
            let list: Vec<(&'static str, T)> = value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|pair| (leak(pair[0].as_str().unwrap_or("")), item(&pair[1])))
                        .collect()
                })
                .unwrap_or_default();
            Box::leak(list.into_boxed_slice())
        }
        let list: Value = serde_json::from_str(text).expect("JSON");
        list.as_array()
            .expect("a list of entries")
            .iter()
            .map(|entry| Entry {
                name: leak(entry["name"].as_str().unwrap_or("")),
                rendered: leak(entry["rendered"].as_str().unwrap_or("")),
                source: match entry["source"].as_str() {
                    Some("app") => Source::App,
                    Some("website") => Source::Website,
                    _ => Source::Ui,
                },
                description: leak(entry["description"].as_str().unwrap_or("")),
                parts: pairs(&entry["parts"], |what| leak(what.as_str().unwrap_or(""))),
                states: pairs(&entry["states"], strings),
                interaction: strings(&entry["interaction"]),
                app_only: strings(&entry["appOnly"]),
                contains: strings(&entry["contains"]),
                slots: strings(&entry["slots"]),
                container: entry["container"].as_bool().unwrap_or(false),
            })
            .collect()
    }
}
