use super::*;

fn extract(sources: &[(&str, &str)]) -> Result<Po, ExtractError> {
    let mut messages = Messages::default();
    for (file, text) in sources {
        messages.scan(text, file)?;
    }
    Ok(Po {
        messages: messages.list,
        ..Po::default()
    })
}

/// The `t!` family's messages, as the native UI's domains scan them.
fn extract_native(sources: &[(&str, &str)]) -> Po {
    let mut messages = Messages::with(scan::NATIVE_KEYWORDS);
    for (file, text) in sources {
        messages.scan(text, file).unwrap();
    }
    Po {
        messages: messages.list,
        ..Po::default()
    }
}

const SV_HEADER: &str =
    "msgid \"\"\nmsgstr \"Language: sv\\nPlural-Forms: nplurals=2; plural=(n != 1);\\n\"\n\n";

#[test]
fn finds_every_kind_of_call() {
    let po = extract(&[(
        "src/a.rs",
        r##"
            // tr!("not in a comment")
            /// tr!("nor in docs")
            fn f(n: u64) {
                let _ = crate::tr!("Show Caskade");
                let _ = trn!("One game", "{n} games", n);
                let _ = trp!("folder browser place", "Home");
                let _ = caskade_core::trnp!("tray", "One window", "{n} windows", n, extra = 1);
                tracing::warn!("{}", tr!(r#"A "raw" string"#));
                let _ = tr!("Line\nbreak {a}", a = tr!("nested"));
                let _ = not_tr!("skipped");
                let _ = caskade_core::msgnp!("feed", "One update", "{n} updates", n);
                let _ = crate::msg!("{game} added", game = crate::msgp!("verb", "Quit"));
            }
            macro_rules! tr { ($msgid:literal) => {}; }
            "##,
    )])
    .unwrap();
    let keys: Vec<_> = po
        .messages
        .iter()
        .map(|message| {
            (
                message.context.as_deref(),
                message.id.as_str(),
                message.plural.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            (None, "Show Caskade", None),
            (None, "One game", Some("{n} games")),
            (Some("folder browser place"), "Home", None),
            (Some("tray"), "One window", Some("{n} windows")),
            (None, "A \"raw\" string", None),
            (None, "Line\nbreak {a}", None),
            (None, "nested", None),
            (Some("feed"), "One update", Some("{n} updates")),
            (None, "{game} added", None),
            (Some("verb"), "Quit", None),
        ]
    );
    assert_eq!(po.messages[1].translations, ["", ""]);
}

#[test]
fn a_message_used_twice_lists_both_files_once() {
    let po = extract(&[
        ("src/a.rs", r#"fn a() { tr!("Quit"); tr!("Quit"); }"#),
        (
            "src/b.rs",
            r#"fn b() { tr!("Quit"); trp!("verb", "Quit"); }"#,
        ),
    ])
    .unwrap();
    assert_eq!(po.messages.len(), 2);
    assert_eq!(po.messages[0].references, ["src/a.rs", "src/b.rs"]);
    assert_eq!(po.messages[1].references, ["src/b.rs"]);
}

#[test]
fn rejects_calls_it_cannot_read() {
    for source in [
        r#"fn a() { tr!(MESSAGE); }"#,
        r#"fn a() { trn!("One"); }"#,
        r#"fn a() { tr!(""); }"#,
        r#"fn a() { tr!("x" "y"); }"#,
        r#"fn a() { tr!("Games"); trn!("Games", "{n} games", 2); }"#,
    ] {
        assert!(extract(&[("src/a.rs", source)]).is_err(), "{source}");
    }
}

#[test]
fn merging_keeps_translations_and_follows_the_template() {
    let template = extract(&[(
        "src/a.rs",
        r#"fn a(n: u64) { tr!("Kept"); tr!("New"); trn!("One game", "{n} games", n); }"#,
    )])
    .unwrap();
    let catalog = Po::parse(
        r#"msgid ""
msgstr "Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\n"

# Translator's note.
#: src/old.rs
msgid "Kept"
msgstr "Behållen"

msgid "One game"
msgid_plural "{n} game(s)"
msgstr[0] "Ett spel"
msgstr[1] "Två spel"
msgstr[2] "{n} spel"

msgid "Gone"
msgstr "Borta"
"#,
    )
    .unwrap();
    let merged = merge(&template, &catalog).unwrap();
    assert_eq!(merged.header, catalog.header);
    let ids: Vec<_> = merged
        .messages
        .iter()
        .map(|message| message.id.as_str())
        .collect();
    assert_eq!(ids, ["Kept", "New", "One game"]);
    // The message the code dropped keeps its translation, obsolete.
    assert_eq!(merged.obsolete.len(), 1);
    assert_eq!(merged.obsolete[0].id, "Gone");
    assert_eq!(merged.obsolete[0].translations, ["Borta"]);
    let kept = &merged.messages[0];
    assert_eq!(kept.translations, ["Behållen"]);
    assert_eq!(kept.translator_comments, ["Translator's note."]);
    assert_eq!(kept.references, ["src/a.rs"]);
    assert_eq!(merged.messages[1].translations, [""]);
    // The English plural changed: the translation stays, as a guess.
    let games = &merged.messages[2];
    assert!(games.is_fuzzy());
    assert_eq!(games.plural.as_deref(), Some("{n} games"));
    assert_eq!(games.translations.len(), 3);
    // Merging again changes nothing.
    let again = merge(&template, &merged).unwrap();
    assert_eq!(again.write(), merged.write());
}

#[test]
fn new_plural_messages_get_the_languages_forms() {
    let template = extract(&[("src/a.rs", r#"fn a() { trn!("One", "{n}", 2); }"#)]).unwrap();
    let catalog = Po::parse(
        "msgid \"\"\nmsgstr \"Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\\n\"\n",
    )
    .unwrap();
    let merged = merge(&template, &catalog).unwrap();
    assert_eq!(merged.messages[0].translations, ["", "", ""]);
}

#[test]
fn merging_keeps_contexts_plurals_notes_and_tags() {
    let template = extract_native(&[(
        "apps/native/src/a.rs",
        r##"fn a(n: u64) {
                t!(note = "A button", "Open");
                tp!("verb", "Open");
                tn!("# game", "# games", n);
                trans!("<0>{guide}</0> opens the quick menu", guide = g; 0 => |x| x);
            }"##,
    )]);
    let catalog = Po::parse(&format!(
        "{SV_HEADER}{}",
        r##"# Checked by a native speaker.
#. An old note
#: apps/desktop/src/Old.tsx
msgid "Open"
msgstr "Öppna"

msgctxt "verb"
msgid "Open"
msgstr "Öppna (verb)"

msgid "# game"
msgid_plural "# games"
msgstr[0] "# spel"
msgstr[1] "# spel"

msgid "<0>{guide}</0> opens the quick menu"
msgstr "<0>{guide}</0> öppnar snabbmenyn"
"##
    ))
    .unwrap();
    let merged = merge(&template, &catalog).unwrap();
    let find = |context: Option<&str>, id: &str| {
        merged
            .messages
            .iter()
            .find(|message| message.key() == (context, id))
            .unwrap()
    };
    let open = find(None, "Open");
    assert_eq!(open.translations, ["Öppna"]);
    assert_eq!(open.translator_comments, ["Checked by a native speaker."]);
    // Notes and references follow the code.
    assert_eq!(open.extracted_comments, ["A button"]);
    assert_eq!(open.references, ["apps/native/src/a.rs"]);
    assert_eq!(find(Some("verb"), "Open").translations, ["Öppna (verb)"]);
    let games = find(None, "# game");
    assert_eq!(games.plural.as_deref(), Some("# games"));
    assert_eq!(games.translations, ["# spel", "# spel"]);
    assert!(!games.is_fuzzy());
    assert_eq!(
        find(None, "<0>{guide}</0> opens the quick menu").translations,
        ["<0>{guide}</0> öppnar snabbmenyn"]
    );
    assert!(merged.obsolete.is_empty());
    assert!(lost_translations(&catalog, &merged).is_empty());
}

#[test]
fn messages_the_code_drops_stay_obsolete_and_come_back() {
    let catalog = Po::parse(&format!(
        "{SV_HEADER}{}",
        r##"# Checked.
#. A note
#: apps/desktop/src/Old.tsx
#, fuzzy, ts-format
msgctxt "navigation"
msgid "Library"
msgstr "Bibliotek"

msgid "One file"
msgid_plural "# files"
msgstr[0] "En fil"
msgstr[1] "# filer"

msgid "Untranslated"
msgstr ""

#~ msgid "Older"
#~ msgstr "Äldre"
"##
    ))
    .unwrap();
    let empty = extract_native(&[]);
    let gone = merge(&empty, &catalog).unwrap();
    assert!(gone.messages.is_empty());
    let ids: Vec<_> = gone
        .obsolete
        .iter()
        .map(|entry| (entry.context.as_deref(), entry.id.as_str()))
        .collect();
    // Untranslated entries go; the rest keep their context.
    assert_eq!(
        ids,
        [
            (Some("navigation"), "Library"),
            (None, "One file"),
            (None, "Older")
        ]
    );
    let library = &gone.obsolete[0];
    assert_eq!(library.translator_comments, ["Checked."]);
    assert!(library.references.is_empty() && library.extracted_comments.is_empty());
    assert_eq!(library.flags, ["fuzzy"]);
    assert_eq!(gone.obsolete[1].translations, ["En fil", "# filer"]);
    assert!(lost_translations(&catalog, &gone).is_empty());
    // Written and read back, it merges the same.
    let reread = Po::parse(&gone.write()).unwrap();
    assert_eq!(merge(&empty, &reread).unwrap().write(), gone.write());
    // The code marks them again: their translations are back.
    let back = extract_native(&[(
        "apps/native/src/a.rs",
        r##"fn a(n: u64) { tp!("navigation", "Library"); tn!("One file", "# files", n); }"##,
    )]);
    let revived = merge(&back, &reread).unwrap();
    assert_eq!(revived.messages.len(), 2);
    assert_eq!(revived.messages[0].translations, ["Bibliotek"]);
    assert!(revived.messages[0].is_fuzzy());
    assert_eq!(revived.messages[1].translations, ["En fil", "# filer"]);
    assert_eq!(revived.messages[1].references, ["apps/native/src/a.rs"]);
    let ids: Vec<_> = revived.obsolete.iter().map(|entry| &entry.id).collect();
    assert_eq!(ids, ["Older"]);
}

#[test]
fn a_changed_context_is_a_lost_translation() {
    let catalog = Po::parse(&format!(
        "{SV_HEADER}msgctxt \"size\"\nmsgid \"Unknown\"\nmsgstr \"Okänd\"\n"
    ))
    .unwrap();
    let mut renamed = catalog.clone();
    renamed.messages[0].context = Some("game size".to_owned());
    let lost = lost_translations(&catalog, &renamed);
    assert_eq!(lost.len(), 1);
    assert!(
        lost[0].contains("size") && lost[0].contains("Unknown"),
        "{lost:?}"
    );
    // Through a merge, the old context's translation stays obsolete.
    let template = extract_native(&[(
        "apps/native/src/a.rs",
        r##"fn a() { tp!("game size", "Unknown"); }"##,
    )]);
    let merged = merge(&template, &catalog).unwrap();
    assert_eq!(merged.messages[0].translations, [""]);
    assert_eq!(merged.obsolete[0].context.as_deref(), Some("size"));
    assert!(lost_translations(&catalog, &merged).is_empty());
}

#[test]
fn a_message_moving_between_domains_keeps_its_translation() {
    let ui = Po::parse(&format!(
        "{SV_HEADER}{}",
        r##"msgid "Cancel"
msgstr "Avbryt"

msgid "# store"
msgid_plural "# stores"
msgstr[0] "# butik"
msgstr[1] "# butiker"

#~ msgid "More"
#~ msgstr "Mer"

#, fuzzy
msgid "Guess"
msgstr "Gissning"
"##
    ))
    .unwrap();
    let app = Po::parse(&format!("{SV_HEADER}msgid \"Cancel\"\nmsgstr \"\"\n")).unwrap();
    let template = extract_native(&[(
        "apps/native/src/a.rs",
        r##"fn a(n: u64) { t!("Cancel"); t!("More"); t!("Guess"); tn!("# store", "# stores", n); }"##,
    )]);
    let merged = merge_with(&template, &app, std::slice::from_ref(&ui)).unwrap();
    let translations: Vec<_> = merged
        .messages
        .iter()
        .map(|message| (message.id.as_str(), message.translations.clone()))
        .collect();
    assert_eq!(
        translations,
        [
            ("Cancel", vec!["Avbryt".to_owned()]),
            ("More", vec!["Mer".to_owned()]),
            // A guess isn't carried over.
            ("Guess", vec![String::new()]),
            (
                "# store",
                vec!["# butik".to_owned(), "# butiker".to_owned()]
            ),
        ]
    );
    // Merging again changes nothing.
    let again = merge_with(&template, &merged, std::slice::from_ref(&ui)).unwrap();
    assert_eq!(again.write(), merged.write());
}

#[test]
fn a_message_gaining_a_plural_keeps_its_translation_as_a_guess() {
    let catalog = Po::parse(&format!("{SV_HEADER}msgid \"# game\"\nmsgstr \"# spel\"\n")).unwrap();
    let template = extract_native(&[(
        "apps/native/src/a.rs",
        r##"fn a(n: u64) { tn!("# game", "# games", n); }"##,
    )]);
    let merged = merge(&template, &catalog).unwrap();
    let games = &merged.messages[0];
    assert!(games.is_fuzzy());
    assert_eq!(games.translations, ["# spel", ""]);
    assert!(lost_translations(&catalog, &merged).is_empty());
}

#[test]
fn the_native_ui_splits_into_app_and_ui_by_folder() {
    let messages = extract_native(&[
        (
            "apps/native/src/library/view.rs",
            r##"fn a() { t!("Library"); t!("Cancel"); }"##,
        ),
        (
            "apps/native/src/ui/dialog.rs",
            r##"fn a() { t!("Cancel"); t!("Close"); }"##,
        ),
        (
            "crates/caskade-ui-i18n/src/format.rs",
            r##"fn a() { t!("Unknown"); }"##,
        ),
    ])
    .messages;
    let ids = |part: Part| {
        messages
            .iter()
            .filter(|message| part.has(message))
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(APP.part), ["Library", "Unknown"]);
    assert_eq!(ids(UI.part), ["Cancel", "Close"]);
}

#[test]
fn a_started_catalog_has_the_languages_header() {
    let root = std::env::temp_dir().join(format!("caskade-i18n-start-{}", std::process::id()));
    let sv = root.join("locales/sv");
    std::fs::create_dir_all(&sv).unwrap();
    std::fs::write(
        sv.join("core.po"),
        "msgid \"\"\nmsgstr \"Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n==2 ? 1 : 2);\\n\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("locales/en-XA")).unwrap();
    std::fs::write(root.join("locales/en-XA/app.po"), "").unwrap();
    assert_eq!(languages(&root).unwrap(), ["sv"]);
    let started = start_catalog(&root, "sv").unwrap();
    assert_eq!(started.header_field("Language"), Some("sv"));
    assert_eq!(started.plural_forms().unwrap().count, 3);
    let new = start_catalog(&root, "de").unwrap();
    assert_eq!(new.header_field("Plural-Forms"), Some(DEFAULT_PLURAL_FORMS));
    let _ = std::fs::remove_dir_all(&root);
}
