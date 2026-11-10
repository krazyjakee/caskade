//! Paper (`themes/paper`, #143): a light theme written only
//! against the public contract. It passes the lint as it is on disk and
//! compiles on its own, without caskade.default underneath.
use caskade_theme::{DirFiles, Theme, chain, compile, servable};
use std::path::Path;

fn paper() -> Theme {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/paper");
    Theme::load(&DirFiles::new(dir)).unwrap_or_else(|problems| {
        let problems: Vec<String> = problems.iter().map(ToString::to_string).collect();
        panic!("themes/paper fails the lint:\n{}", problems.join("\n"))
    })
}

#[test]
fn paper_passes_the_lint_as_a_light_theme_that_extends_nothing() {
    let theme = paper();
    assert_eq!(theme.id(), "dev.caskade.paper");
    assert_eq!(theme.manifest.name, "Paper");
    assert_eq!(theme.manifest.modes, ["light"]);
    assert_eq!(theme.manifest.extends, None);
    assert_eq!(theme.manifest.previews, ["previews/home.svg"]);
}

#[test]
fn paper_compiles_into_the_theme_layer_with_its_tokens_and_assets() {
    let theme = paper();
    let chain = chain("dev.caskade.paper", |id| {
        (id == theme.id()).then_some(&theme)
    })
    .unwrap();
    assert_eq!(chain.len(), 1, "Paper stands alone");
    let css = compile(&chain);
    assert!(
        css.starts_with("/* dev.caskade.paper 1.0.0, compiled by caskade-theme. */\n@font-face {")
    );
    let layer = css.find("@layer cs.theme {\n:root {\n").expect("the layer");
    assert!(
        !css[layer..].contains("@font-face"),
        "the fonts come before the layer"
    );
    assert!(css.trim_end().ends_with('}'));
    for expected in [
        // Light paper and ink.
        "  --cs-color-surface-base: #f3eee3;\n",
        "  --cs-color-fg: #1f1b14;\n",
        // Square corners, no chamfer.
        "  --cs-radius-card: 0;\n",
        "  --cs-radius-control: 0;\n",
        "  --cs-radius-panel: 0;\n",
        "  --cs-chamfer: 0px;\n",
        // No blur, grid, scanlines or glow.
        "  --cs-effect-glass-blur: 0px;\n",
        "  --cs-effect-grid-opacity: 0;\n",
        "  --cs-effect-scanline-opacity: 0;\n",
        "  --cs-effect-glow-strength: 0;\n",
        "  --cs-color-glow: #00000000;\n",
        // A serif display face, bundled.
        "  --cs-font-display: \"Source Serif 4 Variable\", \"Georgia\", \"Times New Roman\", serif;\n",
        "  --cs-focus-ring-glow: var(--cs-color-accent-alt);\n",
        "  --cs-card-aspect: 3 / 2;\n",
        r#"src: url("/dev.caskade.paper/assets/fonts/source-serif-4-latin-wght-normal.woff2") format("woff2");"#,
        r#"src: url("/dev.caskade.paper/assets/fonts/source-serif-4-latin-ext-wght-normal.woff2") format("woff2");"#,
        r#"background-image: url("/dev.caskade.paper/assets/grain.svg");"#,
    ] {
        assert!(css.contains(expected), "missing {expected:?} in\n{css}");
    }
    // Nothing of Nexus comes along.
    assert!(!css.contains("/* caskade.default */"));
}

#[test]
fn paper_serves_its_fonts_images_and_preview_but_not_its_sources() {
    let theme = paper();
    for path in [
        "assets/fonts/source-serif-4-latin-wght-normal.woff2",
        "assets/fonts/source-serif-4-latin-ext-wght-normal.woff2",
        "assets/grain.svg",
        "previews/home.svg",
    ] {
        assert!(servable(&theme, path), "{path} is servable");
    }
    for path in [
        "theme.json",
        "tokens.json",
        "theme.css",
        "assets/fonts/OFL.txt",
    ] {
        assert!(!servable(&theme, path), "{path} is not servable");
    }
}
