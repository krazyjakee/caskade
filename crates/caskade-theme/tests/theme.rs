//! Theme packages against the theming contract (#139).
use caskade_theme::{DirFiles, Locales, MemFiles, Problem, Theme, chain, compile, servable};
use std::path::{Path, PathBuf};

fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/default")
}

fn default_theme() -> Theme {
    Theme::load(&DirFiles::new(default_dir())).expect("themes/default passes the lint")
}

const MANIFEST: &str = r#"{"id": "dev.example.neon", "name": "Neon", "version": "0.1.0", "extends": "caskade.default"}"#;

/// A theme from `(path, contents)` pairs; `theme.json` defaults to [`MANIFEST`].
fn theme(files: &[(&str, &str)]) -> Result<Theme, Vec<Problem>> {
    let mut all: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(path, text)| (path.to_string(), text.as_bytes().to_vec()))
        .collect();
    if !files.iter().any(|(path, _)| *path == "theme.json") {
        all.push(("theme.json".into(), MANIFEST.as_bytes().to_vec()));
    }
    Theme::load(&MemFiles::new(all))
}

fn problems(files: &[(&str, &str)]) -> Vec<String> {
    theme(files)
        .map(|theme| theme.id().to_owned())
        .expect_err("the theme is rejected")
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn the_default_theme_passes_the_lint_and_compiles_into_the_theme_layer() {
    let theme = default_theme();
    assert_eq!(theme.id(), "caskade.default");
    let css =
        compile(&chain("caskade.default", |id| (id == theme.id()).then_some(&theme)).unwrap());
    assert!(css.starts_with(
        "/* caskade.default 1.0.0, compiled by caskade-theme. */\n@layer cs.theme {\n"
    ));
    assert!(css.trim_end().ends_with('}'));
}

#[test]
fn a_theme_overrides_tokens_and_restyles_anatomy_on_top_of_what_it_extends() {
    let base = default_theme();
    let neon = Theme::load(&MemFiles::new([
        ("theme.json", MANIFEST.as_bytes().to_vec()),
        (
            "tokens.json",
            br##"{
              "color": {
                "accent": { "$root": { "$value": "#E0457B" }, "alt": { "$value": "{color.palette.cyan}" } }
              },
              "radius": { "card": { "$value": { "value": 2, "unit": "px" } } },
              "motion": { "ease": { "out": { "$value": [0.2, 0, 0, 1] } } }
            }"##
            .to_vec(),
        ),
        (
            "theme.css",
            br#"[data-cs="game-card"] [data-part="title"] { color: var(--cs-color-accent); }
[data-cs="hero"]::after { background: url(assets/scan%20lines.png) repeat; }
@font-face { font-family: Neon; src: url("./assets/neon.woff2") format("woff2"); }
@media (prefers-reduced-motion: reduce) { :root { --cs-motion-ambient: paused; } }
"#
            .to_vec(),
        ),
        ("assets/scan lines.png", b"png".to_vec()),
        ("assets/neon.woff2", b"font".to_vec()),
    ]))
    .expect("the theme passes the lint");
    let themes = [&base, &neon];
    let lookup = |id: &str| themes.iter().copied().find(|theme| theme.id() == id);
    let chain = chain("dev.example.neon", lookup).unwrap();
    assert_eq!(
        chain.iter().map(|theme| theme.id()).collect::<Vec<_>>(),
        ["caskade.default", "dev.example.neon"]
    );
    let css = compile(&chain);
    assert!(css.starts_with(
        "/* dev.example.neon 0.1.0, compiled by caskade-theme. */\n@layer cs.theme {\n:root {\n"
    ));
    for expected in [
        "  --cs-color-accent: #e0457b;\n",
        "  --cs-color-accent-alt: var(--cs-color-palette-cyan);\n",
        "  --cs-radius-card: 2px;\n",
        "  --cs-motion-ease-out: cubic-bezier(0.2, 0, 0, 1);\n",
        r#"background: url("/dev.example.neon/assets/scan%20lines.png") repeat;"#,
        r#"src: url("/dev.example.neon/assets/neon.woff2") format("woff2");"#,
    ] {
        assert!(css.contains(expected), "missing {expected:?} in\n{css}");
    }
    // The base theme's CSS comes first, so the extending theme wins ties.
    let base_at = css.find("/* caskade.default */");
    let neon_at = css.find("/* dev.example.neon */").unwrap();
    assert!(base_at.is_none_or(|base_at| base_at < neon_at));
    assert!(servable(&neon, "assets/neon.woff2"));
    assert!(!servable(&neon, "theme.css"));
    assert!(!servable(&neon, "tokens.json"));
}

#[test]
fn invalid_theme_json_is_rejected_with_its_location() {
    assert_eq!(
        problems(&[("theme.json", "{\n  \"id\": \"dev.x\",\n  \"name\": \n}")]),
        ["theme.json:4:1: expected value"]
    );
    assert_eq!(
        problems(&[("theme.json", r#"{"id": "dev.x", "version": "1.0.0"}"#)]),
        ["theme.json:1:35: missing field `name`"]
    );
    let found = problems(&[(
        "theme.json",
        r#"{"id": "Neon", "name": "N", "version": "1.0", "modes": ["sepia"], "previews": ["shot.png"]}"#,
    )]);
    assert_eq!(
        found,
        [
            "theme.json: id \"Neon\" must be reverse-DNS: lowercase dot-separated names, like dev.example.neon",
            "theme.json: version \"1.0\" must be major.minor.patch",
            "theme.json: mode \"sepia\" is not dark or light",
            "theme.json: preview \"shot.png\" doesn't exist in the theme",
        ]
    );
    assert!(
        problems(&[(
            "theme.json",
            r#"{"id": "dev.x", "name": "N", "version": "1.0.0", "script": "x.js"}"#
        )])[0]
            .contains("unknown field `script`")
    );
}

#[test]
fn unknown_tokens_and_wrong_value_types_are_rejected() {
    let found = problems(&[(
        "tokens.json",
        r##"{
          "color": {
            "nope": { "$value": "#fff" },
            "accent": { "$root": { "$value": "red" }, "strong": { "$value": "{motion.duration.fast}" } },
            "border": { "$root": { "$type": "dimension", "$value": "1px" } }
          },
          "radius": { "card": { "$value": "4pt" } }
        }"##,
    )]);
    assert_eq!(
        found,
        [
            "tokens.json: color.accent: expected a hex colour, got \"red\"",
            "tokens.json: color.accent.strong: alias {motion.duration.fast} is a duration, not a color",
            "tokens.json: color.border: is a color, not a dimension",
            "tokens.json: color.nope: is not a token of the theming contract",
            "tokens.json: radius.card: expected a dimension in px, rem, em, %, vh, vw, got \"4pt\"",
        ]
    );
}

#[test]
fn class_id_and_element_selectors_are_rejected_at_their_position() {
    let found = problems(&[(
        "theme.css",
        ".card:hover { color: red; }\n[data-cs=\"hero\"] #title,\n  h1 { color: red; }\n[data-cs=\"cardz\"] [data-part=\"title\"] { }\n[data-cs=\"hero\"] [data-part=\"stores\"], [data-onclick] { }\n",
    )]);
    assert_eq!(
        found,
        [
            "theme.css:1:1: class selector .card: class names aren't API; target data-cs and data-part",
            "theme.css:2:18: id selector #title: target data-cs and data-part",
            "theme.css:3:3: element selector h1: target data-cs and data-part (use :root for the document)",
            "theme.css:4:1: data-cs \"cardz\" is not a catalogued component",
            "theme.css:5:18: data-part \"stores\" is not a part of hero",
            "theme.css:5:40: [data-onclick] is not a contract attribute (see docs/theming/anatomy.md)",
        ]
    );
}

#[test]
fn consent_dialogs_cannot_be_restyled() {
    // CSK-08: hiding the permission list, adding text, swapping buttons.
    let found = problems(&[(
        "theme.css",
        "[data-cs=\"plugin-review\"] * { color: transparent; }\n\
         :is([data-cs=\"INSTALL-CONFIRMATION\" i])::after { content: \"Verified\"; }\n\
         [data-cs=\"view\"]:has([data-cs=\"plugin-review\"]) [data-cs=\"button\"] { order: 2; }\n",
    )]);
    assert_eq!(
        found,
        [
            "theme.css:1:1: data-cs \"plugin-review\" is a consent dialog: themes can't restyle it",
            "theme.css:2:5: data-cs \"INSTALL-CONFIRMATION\" is a consent dialog: themes can't restyle it",
            "theme.css:3:22: data-cs \"plugin-review\" is a consent dialog: themes can't restyle it",
        ]
    );
}

#[test]
fn scripts_html_and_binaries_are_refused() {
    let found = problems(&[
        ("assets/boot.js", "alert(1)"),
        ("preview.html", "<script>"),
        ("assets/neon.woff2", "font"),
    ]);
    assert_eq!(
        found,
        [
            "assets/boot.js: themes contain no code: scripts, HTML and binaries are refused",
            "preview.html: themes contain no code: scripts, HTML and binaries are refused",
        ]
    );
}

#[test]
fn remote_imports_and_urls_that_leave_the_theme_are_refused() {
    let found = problems(&[(
        "theme.css",
        "@import url(https://fonts.example/neon.css);\n\
         :root { --cs-color-accent: #fff; }\n\
         [data-cs=\"hero\"] { background: url(https://evil.example/x.png); }\n\
         [data-cs=\"panel\"] { background: url('/etc/passwd'), url(../outside.png); }\n\
         [data-cs=\"shelf\"] { background: url(assets/missing.png), url(data:text/html,hi); }\n\
         [data-cs=\"view\"] { background: url(theme.json); }\n",
    )]);
    assert_eq!(
        found,
        [
            "theme.css:1:1: @import isn't allowed: a theme is one theme.css with local assets",
            "theme.css:3:32: url(https://evil.example/x.png) leaves the theme: remote and absolute URLs are refused; ship the file under assets/",
            "theme.css:4:33: url(/etc/passwd) leaves the theme: remote and absolute URLs are refused; ship the file under assets/",
            "theme.css:4:53: url(../outside.png) escapes the theme folder",
            "theme.css:5:33: url(assets/missing.png): assets/missing.png doesn't exist in the theme",
            "theme.css:5:58: only image and font data: URLs are allowed",
            "theme.css:6:32: url(theme.json) must point under assets/",
        ]
    );
}

#[test]
fn important_undeclared_tokens_and_other_at_rules_are_rejected() {
    let found = problems(&[(
        "theme.css",
        "[data-cs=\"hero\"] { color: red !important; --cs-nope: 1; margin: var(--cs-nope, 0); }\n\
         @layer mine { :root { color: red; } }\n\
         [data-cs=\"hero\"] { &:hover { color: red; } }\n",
    )]);
    assert_eq!(
        found,
        [
            "theme.css:1:31: !important isn't allowed: the cs.theme layer already beats component styles",
            "theme.css:1:43: --cs-nope is not a token of the theming contract",
            "theme.css:1:69: --cs-nope is not a token of the theming contract",
            "theme.css:2:1: @layer isn't allowed in a theme",
            "theme.css:3:20: expected a declaration, found \"&\" (nested rules aren't supported)",
        ]
    );
}

#[test]
fn extends_chains_need_installed_themes_and_no_cycles() {
    let one = theme(&[(
        "theme.json",
        r#"{"id": "dev.one", "name": "One", "version": "1.0.0", "extends": "dev.two"}"#,
    )])
    .unwrap();
    let two = theme(&[(
        "theme.json",
        r#"{"id": "dev.two", "name": "Two", "version": "1.0.0", "extends": "dev.one"}"#,
    )])
    .unwrap();
    let themes = [&one, &two];
    let lookup = |id: &str| themes.iter().copied().find(|theme| theme.id() == id);
    assert_eq!(
        chain("dev.one", lookup).unwrap_err(),
        "extends cycle: dev.one → dev.two → dev.one"
    );
    let orphan = theme(&[]).unwrap();
    assert_eq!(
        chain("dev.example.neon", |id| (id == orphan.id())
            .then_some(&orphan))
        .unwrap_err(),
        "dev.example.neon extends caskade.default, which isn't installed or isn't valid"
    );
}

const SV: &str = r#"msgid ""
msgstr ""
"Language: sv\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgid "Neon"
msgstr "Neonljus"

msgid "Glowing edges on a dark grid."
msgstr "Glödande kanter på ett mörkt rutnät."
"#;

/// The catalogs of a theme from `(path, contents)` pairs.
fn locales(files: &[(&str, &str)]) -> (Locales, Vec<String>) {
    let (locales, problems) = Locales::load(&MemFiles::new(
        files
            .iter()
            .map(|(path, text)| (path.to_string(), text.as_bytes().to_vec())),
    ));
    (locales, problems.iter().map(ToString::to_string).collect())
}

#[test]
fn a_themes_name_and_description_come_from_its_catalogs() {
    let files = [("locales/sv.po", SV), ("locales/dev.example.neon.pot", "")];
    // The catalogs leave the theme valid.
    theme(&files).expect("a theme with catalogs is valid");
    let (locales, problems) = locales(&files);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(locales.available(), ["sv"]);
    assert_eq!(locales.text("sv", "Neon"), "Neonljus");
    assert_eq!(locales.text("sv-FI", "Neon"), "Neonljus");
    assert_eq!(
        locales.text("sv", "Glowing edges on a dark grid."),
        "Glödande kanter på ett mörkt rutnät."
    );
    assert_eq!(locales.text("de", "Neon"), "Neon");
}

#[test]
fn a_theme_without_catalogs_is_english_with_no_problems() {
    let (locales, problems) = locales(&[("theme.json", MANIFEST)]);
    assert!(problems.is_empty());
    assert_eq!(locales.text("sv", "Neon"), "Neon");
    // The themes in the repository lint clean, translations included.
    for dir in [default_dir(), default_dir().join("../paper")] {
        let (_, problems) = Locales::load(&DirFiles::new(dir));
        assert!(problems.is_empty(), "{problems:?}");
    }
}

#[test]
fn broken_catalogs_are_lint_problems_but_the_theme_still_applies_in_english() {
    let files = [
        ("locales/sv.po", "msgid \"Neon\"\nmsgstr \"Neonljus\n"),
        ("locales/neon.po.txt", "not a catalog"),
        ("locales/Not a language.po", SV),
        (
            "locales/de.po",
            "msgid \"Neon {mode}\"\nmsgstr \"Neon {modus}\"\n",
        ),
    ];
    theme(&files).expect("catalog problems never stop a theme applying");
    let (locales, problems) = locales(&files);
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        problems[0].starts_with("locales/Not a language.po: "),
        "{problems:?}"
    );
    assert!(problems[1].starts_with("locales/de.po: "), "{problems:?}");
    assert!(problems[1].contains("modus"), "{problems:?}");
    assert!(
        problems[2].starts_with("locales/sv.po: can't be read"),
        "{problems:?}"
    );
    assert_eq!(locales.text("sv", "Neon"), "Neon");
}
