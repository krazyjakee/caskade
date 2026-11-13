//! `backdrop.json` against its schema (#149), and which composition a
//! chain of themes shows.
use caskade_theme::backdrop::{
    self, Blend, Composition, ImageSize, Kind, Length, Motion, Param, Unit, parse_color,
    parse_length, parse_time,
};
use caskade_theme::{DirFiles, MemFiles, Theme, backdrop_of, chain};
use std::path::Path;

const MANIFEST: &str = r##"{"id": "dev.example.sky", "name": "Sky", "version": "0.1.0", "extends": "caskade.default"}"##;

fn load(files: &[(&str, &str)]) -> Result<Theme, Vec<String>> {
    let mut all: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(path, text)| (path.to_string(), text.as_bytes().to_vec()))
        .collect();
    if !files.iter().any(|(path, _)| *path == "theme.json") {
        all.push(("theme.json".into(), MANIFEST.as_bytes().to_vec()));
    }
    Theme::load(&MemFiles::new(all))
        .map_err(|problems| problems.iter().map(ToString::to_string).collect())
}

fn composition(json: &str) -> Composition {
    load(&[("backdrop.json", json), ("assets/sky.png", "png")])
        .expect("the backdrop passes")
        .backdrop()
        .expect("the theme has a backdrop")
        .clone()
}

/// The problems a `backdrop.json` has, joined.
fn problems(json: &str) -> String {
    load(&[("backdrop.json", json), ("assets/sky.png", "png")])
        .expect_err("the backdrop is refused")
        .join("\n")
}

fn built_in(folder: &str) -> Theme {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../themes")
        .join(folder);
    Theme::load(&DirFiles::new(dir)).expect("the built-in theme passes")
}

#[test]
fn the_built_in_themes_compositions_pass_and_use_tokens() {
    for folder in ["default", "cobalt", "emerald", "engage"] {
        let theme = built_in(folder);
        let composition = theme
            .backdrop()
            .expect("a built-in theme has a composition");
        assert_eq!(
            composition.base,
            Param::Token("--cs-color-surface-base".into()),
            "{folder}"
        );
        assert!(!composition.layers.is_empty(), "{folder}");
    }
    let nexus = built_in("default");
    let layers = &nexus.backdrop().unwrap().layers;
    // The grid takes Reduce effects' opacity token and pans a cell a period.
    let grid = &layers[0];
    assert_eq!(
        grid.opacity,
        Param::Token("--cs-effect-grid-opacity".into())
    );
    assert!(matches!(
        &grid.kind,
        Kind::Grid { cell, .. } if cell[0] == Param::Token("--cs-grid-size".into())
    ));
    assert!(matches!(grid.motion, Some(Motion::Pan { .. })));
    assert!(grid.mask.is_some());
    // The glows drift and follow the accent.
    assert!(layers[1..].iter().all(|layer| matches!(
        &layer.kind,
        Kind::Radial { stops, .. } if matches!(stops[0].color, Param::Token(_))
    ) && matches!(
        layer.motion,
        Some(Motion::Drift { .. })
    )));
    // Engage is still.
    let engage = built_in("engage");
    assert!(
        engage
            .backdrop()
            .unwrap()
            .layers
            .iter()
            .all(|layer| layer.motion.is_none())
    );
}

#[test]
fn every_kind_of_layer_and_its_defaults_parse() {
    let parsed = composition(
        r##"{
          "$description": "every kind",
          "base": "#102030",
          "frameRate": 12,
          "layers": [
            {"type": "linear", "stops": [{"color": "#000", "at": "0%"}, {"color": "transparent", "at": "100%"}]},
            {"type": "radial", "size": "40%", "blend": "screen",
             "stops": [{"color": "{color.accent}", "alpha": 0.5, "at": "0%"}, {"color": "transparent", "at": "70%"}],
             "motion": {"type": "drift", "by": ["-3vw", "2vh"], "period": "{motion.duration.slow}"}},
            {"type": "grid", "color": "{color.accent.alt}", "cell": ["24px", "2rem"], "opacity": "{effect.grid.opacity}",
             "mask": {"at": ["50%", "50%"], "size": ["80%", "70%"], "from": "20%", "to": "75%"},
             "motion": {"type": "pan", "by": ["24px", 0], "period": "30s"}},
            {"type": "image", "src": "assets/sky.png", "size": "cover", "at": ["50%", "100%"], "repeat": false,
             "blend": "multiply", "opacity": 0.35}
          ]
        }"##,
    );
    assert_eq!(
        parsed.base,
        Param::Literal([16.0 / 255.0, 32.0 / 255.0, 48.0 / 255.0, 1.0])
    );
    assert_eq!(parsed.frame_rate, 12.0);
    let [linear, radial, grid, image] = &parsed.layers[..] else {
        panic!("four layers");
    };
    assert!(
        matches!(&linear.kind, Kind::Linear { angle, stops } if *angle == 180.0 && stops.len() == 2)
    );
    assert_eq!(linear.opacity, Param::Literal(1.0));
    assert_eq!(linear.blend, Blend::Normal);
    let Kind::Radial { at, size, stops } = &radial.kind else {
        panic!("radial");
    };
    let half = Param::Literal(Length {
        value: 50.0,
        unit: Unit::Percent,
    });
    assert_eq!(at, &[half.clone(), half]);
    assert_eq!(size[0], size[1]);
    assert_eq!(stops[0].alpha, Param::Literal(0.5));
    assert_eq!(stops[1].at, 0.7);
    assert_eq!(radial.blend, Blend::Screen);
    assert!(matches!(
        &radial.motion,
        Some(Motion::Drift { period: Param::Token(name), .. }) if name == "--cs-motion-duration-slow"
    ));
    let Kind::Grid { line, .. } = &grid.kind else {
        panic!("grid");
    };
    assert_eq!(
        line,
        &Param::Literal(Length {
            value: 1.0,
            unit: Unit::Px
        })
    );
    let mask = grid.mask.as_ref().unwrap();
    assert_eq!((mask.from, mask.to), (0.2, 0.75));
    let Kind::Image {
        src, size, repeat, ..
    } = &image.kind
    else {
        panic!("image");
    };
    assert_eq!(
        (src.as_str(), size, *repeat),
        ("assets/sky.png", &ImageSize::Cover, false)
    );
    assert_eq!(parsed.images(), vec!["assets/sky.png"]);
    // A flat backdrop is no layers.
    assert!(composition(r##"{"layers": []}"##).layers.is_empty());
}

#[test]
fn a_backdrop_that_breaks_the_schema_is_refused_with_where_and_why() {
    let cases = [
        (
            r##"{"layers": [], "shader": "x"}"##,
            "unknown field `shader`",
        ),
        (
            r##"{"base": "red", "layers": []}"##,
            "base: expected a hex colour",
        ),
        (
            r##"{"base": "{color.nope}", "layers": []}"##,
            "{color.nope} is not a token",
        ),
        (
            r##"{"base": "{grid.size}", "layers": []}"##,
            "{grid.size} is a dimension, not a color",
        ),
        (
            r##"{"frameRate": 240, "layers": []}"##,
            "frameRate: expected 1 to 60",
        ),
        (
            r##"{"layers": [{"type": "blob"}]}"##,
            "layers[0].type: \"blob\" is not linear, radial, grid, image or shader",
        ),
        (
            r##"{"layers": [{"type": "linear", "stops": [{"color": "#000", "at": "0%"}]}]}"##,
            "layers[0].stops: a gradient has 2 to 4 stops",
        ),
        (
            r##"{"layers": [{"type": "linear", "stops": [{"color": "#000", "at": "60%"}, {"color": "#fff", "at": "10%"}]}]}"##,
            "stops go in order",
        ),
        (
            r##"{"layers": [{"type": "linear", "opacity": 2, "stops": [{"color": "#000", "at": "0%"}, {"color": "#fff", "at": "9%"}]}]}"##,
            "layers[0].opacity: expected a number from 0 to 1",
        ),
        (
            r##"{"layers": [{"type": "radial", "stops": [{"color": "#000", "at": "0%"}, {"color": "#fff", "at": "9%"}]}]}"##,
            "a radial layer needs a size",
        ),
        (
            r##"{"layers": [{"type": "radial", "size": "40%", "motion": {"type": "pan", "by": ["1px", "1px"], "period": "9s"}, "stops": [{"color": "#000", "at": "0%"}, {"color": "#fff", "at": "9%"}]}]}"##,
            "pan moves repeating content only",
        ),
        (
            r##"{"layers": [{"type": "grid", "color": "#fff", "cell": "48pt"}]}"##,
            "layers[0].cell: expected a length",
        ),
        (
            r##"{"layers": [{"type": "grid", "color": "#fff", "cell": "48px", "src": "assets/sky.png"}]}"##,
            "src is for image layers",
        ),
        (
            r##"{"layers": [{"type": "grid", "color": "#fff", "cell": "48px", "motion": {"type": "pan", "by": ["1px", "1px"], "period": "0.1s"}}]}"##,
            "expected a duration from 1s to 3600s",
        ),
        (
            r##"{"layers": [{"type": "grid", "color": "#fff", "cell": "48px", "mask": {"at": ["0%", "0%"], "size": ["9%", "9%"], "from": "50%", "to": "20%"}}]}"##,
            "to must be beyond from",
        ),
        (
            r##"{"layers": [{"type": "image", "src": "../sky.png"}]}"##,
            "must be a file under assets/",
        ),
        (
            r##"{"layers": [{"type": "image", "src": "assets/sky.svg"}]}"##,
            "must be a PNG, JPEG or WebP",
        ),
        (
            r##"{"layers": [{"type": "image", "src": "assets/moon.png"}]}"##,
            "\"assets/moon.png\" doesn't exist",
        ),
    ];
    for (json, expected) in cases {
        let found = problems(json);
        assert!(
            found.contains(expected) && found.contains("backdrop.json"),
            "{json}\n  expected {expected:?}\n  got {found}"
        );
    }
    let many = format!(
        r##"{{"layers": [{}]}}"##,
        [r##"{"type": "grid", "color": "#fff", "cell": "8px"}"##; 9].join(",")
    );
    assert!(problems(&many).contains("at most 8 layers"));
    let images: Vec<(String, String)> = (0..5)
        .map(|n| (format!("assets/{n}.png"), "png".to_owned()))
        .collect();
    let layers: Vec<String> = (0..5)
        .map(|n| format!(r##"{{"type": "image", "src": "assets/{n}.png"}}"##))
        .collect();
    let json = format!(r##"{{"layers": [{}]}}"##, layers.join(","));
    let mut files: Vec<(&str, &str)> = images
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    files.push(("backdrop.json", &json));
    assert!(
        load(&files)
            .unwrap_err()
            .join("\n")
            .contains("at most 4 images")
    );
    // Syntax errors keep their position.
    assert!(problems("{\"layers\": [}").starts_with("backdrop.json:1:"));
}

#[test]
fn a_chain_shows_its_nearest_composition_unless_its_css_restyles_the_backdrop() {
    let nexus = built_in("default");
    let tokens_only = load(&[(
        "tokens.json",
        r##"{"color": {"accent": {"$root": {"$value": "#e0457b"}}}}"##,
    )])
    .unwrap();
    let own_css = load(&[
        ("theme.json", r##"{"id": "dev.example.css", "name": "Css", "version": "0.1.0", "extends": "caskade.default"}"##),
        ("theme.css", r##"[data-cs="backdrop"]::after { content: none; }"##),
    ])
    .unwrap();
    let own_json = load(&[
        ("theme.json", r##"{"id": "dev.example.json", "name": "Json", "version": "0.1.0", "extends": "dev.example.css"}"##),
        ("backdrop.json", r##"{"layers": []}"##),
    ])
    .unwrap();
    assert!(own_css.restyles_backdrop());
    assert!(!tokens_only.restyles_backdrop());
    let themes = [&nexus, &tokens_only, &own_css, &own_json];
    let lookup = |id: &str| themes.iter().copied().find(|theme| theme.id() == id);
    let shown = |id: &str| {
        let chain = chain(id, lookup).unwrap();
        backdrop_of(&chain).map(|(theme, _)| theme.id().to_owned())
    };
    assert_eq!(shown("caskade.default").as_deref(), Some("caskade.default"));
    assert_eq!(shown("dev.example.sky").as_deref(), Some("caskade.default"));
    assert_eq!(shown("dev.example.css"), None);
    assert_eq!(
        shown("dev.example.json").as_deref(),
        Some("dev.example.json")
    );
}

#[test]
fn computed_css_values_parse_back() {
    assert_eq!(
        parse_color("#5b5ff7"),
        Some([91.0 / 255.0, 95.0 / 255.0, 247.0 / 255.0, 1.0])
    );
    assert_eq!(parse_color("#fff8"), Some([1.0, 1.0, 1.0, 136.0 / 255.0]));
    assert_eq!(parse_color(" rgb(255, 0, 0) "), Some([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(
        parse_color("rgba(0, 0, 255, 0.5)"),
        Some([0.0, 0.0, 1.0, 0.5])
    );
    assert_eq!(
        parse_color("rgb(0 255 0 / 25%)"),
        Some([0.0, 1.0, 0.0, 0.25])
    );
    let [r, g, b, a] = parse_color("hsl(120deg 100% 50%)").unwrap();
    assert!(r.abs() < 1e-6 && (g - 1.0).abs() < 1e-6 && b.abs() < 1e-6 && a == 1.0);
    assert_eq!(parse_color("transparent"), Some([0.0; 4]));
    assert_eq!(parse_color("color-mix(in oklab, red, blue)"), None);
    assert_eq!(parse_color("#12345"), None);
    assert_eq!(
        parse_length("48px"),
        Some(Length {
            value: 48.0,
            unit: Unit::Px
        })
    );
    assert_eq!(
        parse_length("-3vw"),
        Some(Length {
            value: -3.0,
            unit: Unit::Vw
        })
    );
    assert_eq!(
        parse_length("1.5rem"),
        Some(Length {
            value: 1.5,
            unit: Unit::Rem
        })
    );
    assert_eq!(
        parse_length("0"),
        Some(Length {
            value: 0.0,
            unit: Unit::Px
        })
    );
    assert_eq!(parse_length("12pt"), None);
    assert_eq!(parse_time("40s"), Some(40.0));
    assert_eq!(parse_time("250ms"), Some(0.25));
    assert_eq!(parse_time("4"), None);
    assert_eq!(backdrop::token_default("--cs-grid-size"), Some("48px"));
    assert_eq!(backdrop::token_default("--cs-nope"), None);
}

const SHADER: &str = "fn layer(input: Input) -> vec4<f32> {
    let t = 0.5 + 0.5 * sin(input.time + input.pixel.x / input.size.x * 6.28);
    return vec4<f32>(input.colors[0].rgb, input.colors[0].a * t * input.numbers.x);
}
";

fn with_shader(json: &str, shader: &str) -> Result<Theme, Vec<String>> {
    load(&[("backdrop.json", json), ("assets/waves.wgsl", shader)])
}

#[test]
fn a_shader_layer_takes_the_themes_wgsl_and_its_inputs_from_tokens() {
    let theme = with_shader(
        r##"{"frameRate": 20, "layers": [
          {"type": "shader", "src": "assets/waves.wgsl", "colors": ["{color.accent}", "#ffffff80"],
           "numbers": ["{effect.glow.strength}", 0.5], "loop": "60s", "opacity": 0.8, "blend": "screen",
           "mask": {"at": ["50%", "50%"], "size": ["60%", "60%"], "from": "0%", "to": "100%"}},
          {"type": "shader", "src": "assets/waves.wgsl", "animate": false}
        ]}"##,
        SHADER,
    )
    .expect("the shader layers pass");
    let composition = theme.backdrop().unwrap();
    let Kind::Shader {
        src,
        shader,
        colors,
        numbers,
    } = &composition.layers[0].kind
    else {
        panic!("a shader layer");
    };
    assert_eq!(src, "assets/waves.wgsl");
    assert!(shader.wgsl.contains(&format!("fn {}(", shader.entry)));
    assert_eq!(colors[0], Param::Token("--cs-color-accent".into()));
    assert_eq!(numbers[0], Param::Token("--cs-effect-glow-strength".into()));
    assert_eq!(numbers[1], Param::Literal(0.5));
    assert_eq!(
        composition.layers[0].motion,
        Some(Motion::Clock {
            period: Param::Literal(60.0)
        })
    );
    // Still: no clock. One shader, used twice.
    assert_eq!(composition.layers[1].motion, None);
    assert_eq!(composition.shaders().len(), 1);
}

#[test]
fn a_shader_layer_that_breaks_the_rules_is_refused() {
    let layer = |extra: &str| {
        format!(r##"{{"layers": [{{"type": "shader", "src": "assets/waves.wgsl"{extra}}}]}}"##)
    };
    let cases = [
        (
            layer(r#", "motion": {"type": "drift", "by": ["1px", "1px"], "period": "9s"}"#),
            SHADER,
            "a shader moves itself",
        ),
        (
            layer(r##", "colors": ["#000", "#000", "#000", "#000", "#000"]"##),
            SHADER,
            "layers[0].colors: at most 4",
        ),
        (
            layer(r#", "loop": "0.5s""#),
            SHADER,
            "layers[0].loop: expected a duration",
        ),
        (
            layer(r#", "stops": []"#),
            SHADER,
            "stops is for linear and radial layers",
        ),
        (
            r##"{"layers": [{"type": "grid", "color": "#fff", "cell": "8px", "animate": true}]}"##
                .to_owned(),
            SHADER,
            "animate is for shader layers",
        ),
        (
            r#"{"layers": [{"type": "shader", "src": "assets/waves.txt"}]}"#.to_owned(),
            SHADER,
            "must be a .wgsl file",
        ),
        (
            r#"{"layers": [{"type": "shader", "src": "assets/none.wgsl"}]}"#.to_owned(),
            SHADER,
            "doesn't exist",
        ),
        (
            layer(""),
            "fn layer(input: Input) -> vec4<f32> { return 1; }",
            "assets/waves.wgsl",
        ),
        (
            format!(
                r#"{{"layers": [{0}, {0}, {0}]}}"#,
                r#"{"type": "shader", "src": "assets/waves.wgsl"}"#
            ),
            SHADER,
            "at most 2 shader layers",
        ),
    ];
    for (json, shader, expected) in cases {
        let found = with_shader(&json, shader)
            .err()
            .unwrap_or_else(|| panic!("{json} passed"))
            .join("\n");
        assert!(
            found.contains(expected),
            "{json}\n  expected {expected:?}\n  got {found}"
        );
    }
}

/// `docs/theming/backdrop.md` is generated from the schema;
/// `CASKADE_WRITE_BACKDROP=1` writes it.
#[test]
fn the_backdrop_reference_is_current() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/theming/backdrop.md");
    let reference = backdrop::reference();
    if std::env::var_os("CASKADE_WRITE_BACKDROP").is_some() {
        std::fs::write(&file, &reference).unwrap();
    }
    let written = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        written == reference,
        "docs/theming/backdrop.md is stale: CASKADE_WRITE_BACKDROP=1 cargo test -p caskade-theme --test backdrop"
    );
}
