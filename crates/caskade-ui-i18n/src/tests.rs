//! The native UI's text in Swedish and the pseudo-locale, and the proof
//! that the move keeps translations: every Swedish translation of a
//! message the native UI uses resolves through its macros.

use super::*;
use caskade_i18n::po::Po;
use std::path::Path;
use std::sync::Mutex;

/// One language at a time: it's process-wide.
static LANGUAGE: Mutex<()> = Mutex::new(());

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn swedish_text_comes_from_the_webviews_catalogs() {
    let _one = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(set_language("sv"), "sv");
    assert_eq!(t!("Quick menu"), "Snabbmeny");
    assert_eq!(tp!("on-screen keyboard", "Delete"), "Radera");
    assert_eq!(t!("Stop {title}", title = "Celeste"), "Stoppa Celeste");
    assert_eq!(
        tn!("# already has it", "# already have it", 1),
        "1 har det redan"
    );
    assert_eq!(
        tn!("# already has it", "# already have it", 1234),
        "1\u{a0}234 har det redan"
    );
    // English where there's no translation.
    assert_eq!(t!("Not a message anywhere"), "Not a message anywhere");
    set_language("en");
    assert_eq!(t!("Quick menu"), "Quick menu");
    assert_eq!(
        tn!("# already has it", "# already have it", 2),
        "2 already have it"
    );
}

#[test]
fn every_swedish_translation_of_a_native_message_resolves() {
    let _one = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    let native = caskade_i18n::extract::native_messages(root()).unwrap();
    assert!(!native.is_empty(), "the native UI marks its text");
    let catalogs: Vec<Po> = ["app", "ui"]
        .iter()
        .map(|domain| {
            let text =
                std::fs::read_to_string(root().join(format!("locales/sv/{domain}.po"))).unwrap();
            Po::parse(&text).unwrap()
        })
        .collect();
    set_language("sv");
    let mut checked = 0;
    for message in &native {
        let found = catalogs.iter().find_map(|po| {
            po.messages.iter().find(|entry| {
                entry.key() == message.key()
                    && !entry.is_fuzzy()
                    && entry.translations.iter().any(|text| !text.is_empty())
            })
        });
        let Some(entry) = found else {
            continue;
        };
        checked += 1;
        let context = message.context.as_deref();
        match &message.plural {
            None => {
                let expected = message::render(
                    &message::parse(&entry.translations[0]),
                    &[],
                    None,
                    &Numbers("sv".into()),
                    &|text| text.to_owned(),
                );
                assert_eq!(
                    text(context, &message.id, &[]),
                    expected,
                    "{:?}",
                    message.id
                );
            }
            Some(english) => {
                let expected = message::render(
                    &message::parse_counted(&entry.translations[1]),
                    &[],
                    Some(5.0),
                    &Numbers("sv".into()),
                    &|text| text.to_owned(),
                );
                assert_eq!(plural(context, &message.id, english, 5.0, &[]), expected);
            }
        }
    }
    assert!(checked > 0, "some native text is translated already");
    set_language("en");
}

#[test]
fn the_pseudo_locale_accents_and_lengthens_text_in_debug_builds() {
    let _one = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    if cfg!(debug_assertions) {
        assert_eq!(set_language("en-XA"), "en-XA");
        assert_eq!(t!("Quick menu"), "[~Ǫũĩćķ ḿēńũ~]");
        assert_eq!(t!("Stop {title}", title = "Celeste"), "[Śţōƥ ]Celeste");
    }
    set_language("en");
}

#[test]
fn numbers_sizes_and_durations_follow_the_language() {
    let _one = LANGUAGE.lock().unwrap_or_else(|e| e.into_inner());
    set_language("sv");
    assert_eq!(format::size(Some(1_610_612_736)), "1,5 GB");
    assert_eq!(format::percent(Some(42.0)), "42\u{a0}%");
    assert_eq!(format::duration(3600 * 3 + 1800), "3,5h");
    assert_eq!(format::size(None), t!("Size unknown"));
    set_language("en-US");
    assert_eq!(format::size(Some(1_610_612_736)), "1.5 GB");
    assert_eq!(format::duration(30), "<1m");
    assert_eq!(format::playtime(0), "Not played");
    assert_eq!(format::playtime(3600 * 3 + 1800), "3.5h played");
    assert_eq!(format::bytes(Some(1_932_735_283)), "1.8 GiB");
    set_language("en");
}

/// Literals the UI shows as they are in every language: the app's name.
const NOT_TRANSLATED: &[(&str, &str)] = &[
    ("apps/native/src/main.rs", "Caskade"),
    // The sidebar's brand, as the webview's (`Sidebar.tsx`).
    ("apps/native/src/frame/sidebar.rs", "CASKADE"),
];

/// Folders whose text no user sees: `shots/` holds #90's fixture screens
/// for the screenshot comparison (the `shots` feature only), in fixed
/// English so Blitz and WebKit render the same text; `gallery/` is the
/// component gallery's fixtures (#93, for developers and theme authors,
/// and screens of the same comparison).
const DEV_ONLY: &[&str] = &["apps/native/src/shots/", "apps/native/src/gallery/"];

#[test]
fn every_rsx_text_people_see_goes_through_a_macro() {
    let found =
        caskade_i18n::extract::untranslated_rsx(root(), caskade_i18n::extract::NATIVE_SOURCES)
            .unwrap();
    let untranslated: Vec<_> = found
        .iter()
        .filter(|(file, text)| {
            !DEV_ONLY.iter().any(|folder| file.starts_with(folder))
                && !NOT_TRANSLATED
                    .iter()
                    .any(|(allowed, literal)| allowed == file && literal == text)
        })
        .collect();
    assert!(
        untranslated.is_empty(),
        "mark these with t! (or list them in NOT_TRANSLATED if no language translates them): {untranslated:#?}"
    );
}
