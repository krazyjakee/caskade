//! `backdrop.json`: the layers the native app draws on the GPU beneath its
//! window (#149), in place of the theme CSS's `[data-cs="backdrop"]`.
//!
//! A composition is data, never code: a base colour and up to eight layers
//! of a few kinds (gradients, a grid, images), each with an opacity, a blend,
//! an optional elliptical mask and an optional motion. Colours, numbers,
//! lengths and durations are literals or `{token}` references to declared
//! tokens of the same type; the app reads references from the backdrop's
//! computed style, so user overrides, Appearance and custom CSS reach them.
//!
//! [`parse`] checks a file against the schema (`docs/theming/backdrop.md`);
//! [`FIELDS`] describes it for the generated reference. The CSS value
//! parsers ([`parse_color`], [`parse_length`], …) read computed values back.

use crate::Problem;
use crate::contract::contract;
use serde::Deserialize;
use serde_json::Value;

pub(crate) const FILE: &str = "backdrop.json";

/// Most layers a composition has.
pub const MAX_LAYERS: usize = 8;
/// Most distinct images its layers use.
pub const MAX_IMAGES: usize = 4;
/// Colour stops per gradient.
pub const STOPS: std::ops::RangeInclusive<usize> = 2..=4;
/// Frames a second while something moves.
pub const FRAME_RATES: std::ops::RangeInclusive<f32> = 1.0..=60.0;
/// A motion's period, in seconds.
pub const PERIODS: std::ops::RangeInclusive<f32> = 1.0..=3600.0;
/// Image types a layer may draw (decoded once, when the theme applies).
pub const IMAGE_TYPES: &[&str] = &["png", "jpg", "jpeg", "webp"];
/// Largest image side, in pixels.
pub const MAX_IMAGE_SIDE: u32 = 4096;
/// Most shader layers a composition has (`crate::shader`).
pub const MAX_SHADERS: usize = 2;
/// Colours and numbers a shader layer passes its shader.
pub const SHADER_INPUTS: usize = 4;
/// Lengths are within this many units of zero.
const MAX_LENGTH: f32 = 10_000.0;

/// A colour: straight-alpha sRGB components in 0..=1.
pub type Rgba = [f32; 4];

/// A value given in the file, or a token to read from the computed style.
#[derive(Clone, Debug, PartialEq)]
pub enum Param<T> {
    Literal(T),
    /// The token's custom property, `--cs-color-accent`.
    Token(String),
}

pub type ColorValue = Param<Rgba>;
pub type NumberValue = Param<f32>;
pub type LengthValue = Param<Length>;
/// Seconds.
pub type TimeValue = Param<f32>;

/// A CSS length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Length {
    pub value: f32,
    pub unit: Unit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Px,
    Rem,
    Em,
    /// Of the backdrop's width (x) or height (y).
    Percent,
    Vw,
    Vh,
}

/// A checked `backdrop.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Composition {
    /// The window's own colour, beneath every layer.
    pub base: ColorValue,
    pub frame_rate: f32,
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub kind: Kind,
    pub opacity: NumberValue,
    pub blend: Blend,
    pub motion: Option<Motion>,
    pub mask: Option<Mask>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// `angle` in degrees, as CSS's: 0 up, 90 right.
    Linear { angle: f32, stops: Vec<Stop> },
    /// An ellipse `size` (radii) around `at`.
    Radial {
        at: [LengthValue; 2],
        size: [LengthValue; 2],
        stops: Vec<Stop>,
    },
    /// Lines `line` wide along the top and left of each `cell`.
    Grid {
        color: ColorValue,
        cell: [LengthValue; 2],
        line: LengthValue,
    },
    /// The theme's own WGSL (`crate::shader`), given `colors` and
    /// `numbers`.
    Shader {
        /// Under `assets/`.
        src: String,
        shader: crate::shader::Compiled,
        colors: Vec<ColorValue>,
        numbers: Vec<NumberValue>,
    },
    /// A theme image, placed as a CSS background is.
    Image {
        /// Under `assets/`.
        src: String,
        size: ImageSize,
        at: [LengthValue; 2],
        repeat: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub color: ColorValue,
    /// Multiplies the colour's alpha.
    pub alpha: NumberValue,
    /// Along the gradient: `%` of its line or radius.
    pub at: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImageSize {
    Cover,
    Contain,
    /// The image's own size, in CSS pixels.
    Auto,
    Explicit([LengthValue; 2]),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Blend {
    #[default]
    Normal,
    Multiply,
    Screen,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Motion {
    /// Out by `by` and back, eased, every `period`.
    Drift {
        by: [LengthValue; 2],
        period: TimeValue,
    },
    /// Steadily by `by` every `period`; repeating layers only.
    Pan {
        by: [LengthValue; 2],
        period: TimeValue,
    },
    /// A shader's clock: its `time` runs from 0 to `period`, then again.
    Clock { period: TimeValue },
}

/// Opaque inside `from` of the ellipse `size` around `at`, fading to
/// nothing at `to` (fractions of the radii).
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub at: [LengthValue; 2],
    pub size: [LengthValue; 2],
    pub from: f32,
    pub to: f32,
}

impl Composition {
    /// The shaders the layers run.
    pub fn shaders(&self) -> Vec<&crate::shader::Compiled> {
        let mut out: Vec<&crate::shader::Compiled> = Vec::new();
        for layer in &self.layers {
            if let Kind::Shader { shader, .. } = &layer.kind
                && !out.iter().any(|known| known.id == shader.id)
            {
                out.push(shader);
            }
        }
        out
    }

    /// The images the layers draw, under `assets/`.
    pub fn images(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for layer in &self.layers {
            if let Kind::Image { src, .. } = &layer.kind
                && !out.contains(&src.as_str())
            {
                out.push(src);
            }
        }
        out
    }
}

// ── The file ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FileDoc {
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    #[serde(rename = "$description", default)]
    _description: Option<String>,
    #[serde(default)]
    base: Option<Value>,
    #[serde(default)]
    frame_rate: Option<f64>,
    layers: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LayerDoc {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    opacity: Option<Value>,
    #[serde(default)]
    blend: Option<String>,
    #[serde(default)]
    motion: Option<MotionDoc>,
    #[serde(default)]
    mask: Option<MaskDoc>,
    // linear
    #[serde(default)]
    angle: Option<f64>,
    #[serde(default)]
    stops: Option<Vec<StopDoc>>,
    // radial, image
    #[serde(default)]
    at: Option<[Value; 2]>,
    #[serde(default)]
    size: Option<Value>,
    // grid
    #[serde(default)]
    color: Option<Value>,
    #[serde(default)]
    cell: Option<Value>,
    #[serde(default)]
    line: Option<Value>,
    // image, shader
    #[serde(default)]
    src: Option<String>,
    #[serde(default)]
    repeat: Option<bool>,
    // shader
    #[serde(default)]
    colors: Option<Vec<Value>>,
    #[serde(default)]
    numbers: Option<Vec<Value>>,
    #[serde(default)]
    animate: Option<bool>,
    #[serde(rename = "loop", default)]
    looping: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopDoc {
    color: Value,
    #[serde(default)]
    alpha: Option<Value>,
    at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionDoc {
    #[serde(rename = "type")]
    kind: String,
    by: [Value; 2],
    period: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaskDoc {
    at: [Value; 2],
    size: [Value; 2],
    from: String,
    to: String,
}

/// One field of the schema, for the generated reference.
pub struct Field {
    /// Where it goes: `(root)`, a layer type, `stop`, `motion`, `mask`.
    pub on: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub required: bool,
    pub default: &'static str,
    pub description: &'static str,
}

const fn field(
    on: &'static str,
    name: &'static str,
    kind: &'static str,
    required: bool,
    default: &'static str,
    description: &'static str,
) -> Field {
    Field {
        on,
        name,
        kind,
        required,
        default,
        description,
    }
}

/// Every field [`parse`] accepts.
pub const FIELDS: &[Field] = &[
    field(
        "(root)",
        "base",
        "colour",
        false,
        "`{color.surface.base}`",
        "The window's own colour, beneath every layer and shown alone when the backdrop is hidden.",
    ),
    field(
        "(root)",
        "frameRate",
        "number, 1–60",
        false,
        "`30`",
        "Frames a second while a layer moves. Slow, soft motion needs few.",
    ),
    field(
        "(root)",
        "layers",
        "array of layers",
        true,
        "",
        "Up to 8, drawn in order, each over the ones before. `[]` is a flat backdrop.",
    ),
    field(
        "every layer",
        "type",
        "`linear`, `radial`, `grid`, `image`, `shader`",
        true,
        "",
        "What the layer draws.",
    ),
    field(
        "every layer",
        "opacity",
        "number, 0–1",
        false,
        "`1`",
        "0 hides the layer, and a hidden layer's motion needs no frames.",
    ),
    field(
        "every layer",
        "blend",
        "`normal`, `multiply`, `screen`",
        false,
        "`normal`",
        "How the layer combines with the layers under it.",
    ),
    field(
        "every layer",
        "motion",
        "motion",
        false,
        "none",
        "How the layer's content moves; its mask stays put.",
    ),
    field(
        "every layer",
        "mask",
        "mask",
        false,
        "none",
        "Fades the layer out away from an ellipse.",
    ),
    field(
        "linear",
        "angle",
        "number (degrees)",
        false,
        "`180`",
        "As CSS `linear-gradient`: 0 points up, 90 right.",
    ),
    field(
        "linear, radial",
        "stops",
        "array of 2–4 stops",
        true,
        "",
        "The gradient's colours, in order.",
    ),
    field(
        "radial",
        "at",
        "[length, length]",
        false,
        "`[\"50%\", \"50%\"]`",
        "The ellipse's centre; `%` of the backdrop's width and height.",
    ),
    field(
        "radial",
        "size",
        "[length, length]",
        true,
        "",
        "The ellipse's radii, as CSS `radial-gradient(40% 35% at …)`.",
    ),
    field("grid", "color", "colour", true, "", "The lines' colour."),
    field(
        "grid",
        "cell",
        "length or [length, length]",
        true,
        "",
        "A cell's width and height.",
    ),
    field(
        "grid",
        "line",
        "length",
        false,
        "`1px`",
        "The lines' width, along each cell's top and left.",
    ),
    field(
        "image",
        "src",
        "path",
        true,
        "",
        "A PNG, JPEG or WebP under `assets/`, at most 4096 pixels a side. Up to 4 images.",
    ),
    field(
        "image",
        "size",
        "`cover`, `contain`, `auto` or [length, length]",
        false,
        "`auto`",
        "As CSS `background-size`; `auto` is the image's own size in CSS pixels.",
    ),
    field(
        "image",
        "at",
        "[length, length]",
        false,
        "`[\"0%\", \"0%\"]`",
        "As CSS `background-position`: `50%` centres the image.",
    ),
    field(
        "image",
        "repeat",
        "boolean",
        false,
        "`true`",
        "Tiles the image in both directions.",
    ),
    field(
        "shader",
        "src",
        "path",
        true,
        "",
        "A `.wgsl` file under `assets/` defining `fn layer(input: Input) -> vec4<f32>` (see Shaders below). Up to 2 shader layers.",
    ),
    field(
        "shader",
        "colors",
        "array of up to 4 colours",
        false,
        "`[]`",
        "`input.colors`, straight alpha: tokens keep the shader in the user's accent and colour scheme.",
    ),
    field(
        "shader",
        "numbers",
        "array of up to 4 numbers",
        false,
        "`[]`",
        "`input.numbers`: tokens such as `{effect.glow.strength}` let Reduce effects reach the shader.",
    ),
    field(
        "shader",
        "animate",
        "boolean",
        false,
        "`true`",
        "Whether `input.time` runs. A still shader is drawn once and asks for no frames.",
    ),
    field(
        "shader",
        "loop",
        "duration, 1s–1h",
        false,
        "`3600s`",
        "`input.time` runs from 0 to this, then again: make the shader seamless at it.",
    ),
    field(
        "stop",
        "color",
        "colour",
        true,
        "",
        "The colour at this stop; `transparent` fades out.",
    ),
    field(
        "stop",
        "alpha",
        "number, 0–1",
        false,
        "`1`",
        "Multiplies the colour's alpha, as `color-mix(in oklab, <color> 22%, transparent)` does.",
    ),
    field(
        "stop",
        "at",
        "percentage",
        true,
        "",
        "Where along the gradient line (linear) or radius (radial).",
    ),
    field(
        "motion",
        "type",
        "`drift`, `pan`",
        true,
        "",
        "`drift` eases out by `by` and back every `period`; `pan` moves steadily by `by` every `period`, for grids and repeating images.",
    ),
    field("motion", "by", "[length, length]", true, "", "How far."),
    field(
        "motion",
        "period",
        "duration, 1s–1h",
        true,
        "",
        "How long one cycle takes.",
    ),
    field(
        "mask",
        "at",
        "[length, length]",
        true,
        "",
        "The ellipse's centre.",
    ),
    field(
        "mask",
        "size",
        "[length, length]",
        true,
        "",
        "The ellipse's radii.",
    ),
    field(
        "mask",
        "from",
        "percentage",
        true,
        "",
        "Fully shown inside this fraction of the radii.",
    ),
    field(
        "mask",
        "to",
        "percentage",
        true,
        "",
        "Gone from this fraction of the radii.",
    ),
];

/// A [`Field::on`] as a heading: `every layer` → `Every layer`.
fn heading(on: &str) -> String {
    if on == "(root)" {
        return "The file".to_owned();
    }
    let mut chars = on.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// `docs/theming/backdrop.md`: the schema's reference, from [`FIELDS`] and
/// [`crate::shader::INPUT`].
pub fn reference() -> String {
    let mut out = String::from(
        "# Backdrop compositions\n\n\
         Generated from `crates/caskade-theme/src/backdrop.rs`: \
         `CASKADE_WRITE_BACKDROP=1 cargo test -p caskade-theme --test backdrop`. Do not edit.\n\n\
         A theme's `backdrop.json` describes the backdrop the native app draws on the GPU \
         beneath its window (#149). How it fits with `theme.css`, inheritance and custom \
         CSS is in [the theming contract](README.md#backdrops-149).\n\n\
         Colours are a hex literal, `transparent` or a `{color.*}` token; numbers a \
         literal or a `{number}` token; lengths `px`, `rem`, `em`, `%`, `vw` or `vh` \
         (`%` of the backdrop's width or height) or a `{dimension}` token; durations \
         `s` or `ms` or a `{duration}` token. Tokens are read from the backdrop's \
         computed style, so Appearance, the user's accent and custom CSS reach them.\n",
    );
    let mut on = "";
    for field in FIELDS {
        if field.on != on {
            on = field.on;
            out.push_str(&format!(
                "\n## {}\n\n| Field | Type | Required | Default | |\n| --- | --- | --- | --- | --- |\n",
                heading(on)
            ));
        }
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            field.name,
            field.kind,
            if field.required { "yes" } else { "" },
            field.default,
            field.description
        ));
    }
    out.push_str(&format!(
        "\n## Shaders\n\n\
         A `shader` layer's file defines `fn layer(input: Input) -> vec4<f32>`, returning \
         the layer's colour at a pixel: straight alpha, sRGB, 0 to 1. It is given:\n\n\
         ```wgsl\n{}```\n\n\
         It may declare helper functions, structs and constants, but no module-scope \
         `var`, bindings, textures, `override`s or entry points: it can read nothing but \
         its input. Files are at most {} KiB; a backdrop has at most {} shader layers. \
         `caskade-theme lint` parses and validates it, and every name it declares is \
         prefixed, so it can't clash with another theme's shader or the app's own code.\n\n\
         A shader runs only when the backdrop moves or changes: the window's frames \
         compose the backdrop as last drawn. The GPU times it where it can, and a shader \
         that takes more than 8 ms a frame, or 40 ms a second of motion, stops moving: \
         it stays on its last frame. Without a usable GPU, and in the webview, the \
         theme's CSS backdrop is drawn instead.\n",
        crate::shader::INPUT,
        crate::shader::MAX_SOURCE / 1024,
        MAX_SHADERS
    ));
    out
}

struct Check<'a> {
    files: &'a [String],
    read: &'a dyn crate::Files,
    problems: Vec<Problem>,
}

impl Check<'_> {
    fn fail(&mut self, at: &str, message: impl std::fmt::Display) {
        self.problems
            .push(Problem::new(FILE, format!("{at}: {message}")));
    }

    /// A `{token}` reference of `kind`, as its custom property.
    fn token(&mut self, at: &str, text: &str, kind: &str) -> Option<Option<String>> {
        let Some(path) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}')) else {
            return Some(None);
        };
        let path = path.strip_suffix(".$root").unwrap_or(path);
        match contract().tokens.get(path) {
            None => {
                self.fail(
                    at,
                    format!("{{{path}}} is not a token of the theming contract"),
                );
                None
            }
            Some(token) if token.kind != kind => {
                self.fail(at, format!("{{{path}}} is a {}, not a {kind}", token.kind));
                None
            }
            Some(token) => Some(Some(token.name.clone())),
        }
    }

    fn color(&mut self, at: &str, value: &Value) -> Option<ColorValue> {
        let Some(text) = value.as_str() else {
            self.fail(at, "expected a colour");
            return None;
        };
        if let Some(token) = self.token(at, text, "color")? {
            return Some(Param::Token(token));
        }
        let literal = parse_color(text).filter(|_| text.starts_with('#') || text == "transparent");
        match literal {
            Some(rgba) => Some(Param::Literal(rgba)),
            None => {
                self.fail(
                    at,
                    format!(
                        "expected a hex colour, transparent or a {{color.*}} token, got \"{text}\""
                    ),
                );
                None
            }
        }
    }

    fn number(
        &mut self,
        at: &str,
        value: Option<&Value>,
        default: f32,
        range: std::ops::RangeInclusive<f32>,
    ) -> Option<NumberValue> {
        let Some(value) = value else {
            return Some(Param::Literal(default));
        };
        if let Some(text) = value.as_str()
            && let Some(token) = self.token(at, text, "number")?
        {
            return Some(Param::Token(token));
        }
        match value.as_f64().map(|n| n as f32) {
            Some(n) if n.is_finite() && range.contains(&n) => Some(Param::Literal(n)),
            _ => {
                self.fail(
                    at,
                    format!(
                        "expected a number from {} to {} or a {{number}} token, got {value}",
                        range.start(),
                        range.end()
                    ),
                );
                None
            }
        }
    }

    fn length(&mut self, at: &str, value: &Value) -> Option<LengthValue> {
        let text = match value {
            Value::String(text) => text.as_str(),
            Value::Number(n) if n.as_f64() == Some(0.0) => "0",
            _ => {
                self.fail(at, format!("expected a length, got {value}"));
                return None;
            }
        };
        if let Some(token) = self.token(at, text, "dimension")? {
            return Some(Param::Token(token));
        }
        match parse_length(text) {
            Some(length) if length.value.abs() <= MAX_LENGTH => Some(Param::Literal(length)),
            _ => {
                self.fail(
                    at,
                    format!(
                        "expected a length in px, rem, em, %, vw or vh (or a {{dimension}} token), got \"{text}\""
                    ),
                );
                None
            }
        }
    }

    fn pair(&mut self, at: &str, values: &[Value; 2]) -> Option<[LengthValue; 2]> {
        let x = self.length(&format!("{at}[0]"), &values[0]);
        let y = self.length(&format!("{at}[1]"), &values[1]);
        Some([x?, y?])
    }

    /// One length for both axes, or a pair.
    fn lengths(&mut self, at: &str, value: &Value) -> Option<[LengthValue; 2]> {
        match value {
            Value::Array(items) if items.len() == 2 => {
                self.pair(at, &[items[0].clone(), items[1].clone()])
            }
            Value::Array(_) => {
                self.fail(at, "expected two lengths");
                None
            }
            single => {
                let length = self.length(at, single)?;
                Some([length.clone(), length])
            }
        }
    }

    fn percent(&mut self, at: &str, text: &str) -> Option<f32> {
        match parse_length(text) {
            Some(Length {
                value,
                unit: Unit::Percent,
            }) if (0.0..=1000.0).contains(&value) => Some(value / 100.0),
            _ => {
                self.fail(at, format!("expected a percentage, got \"{text}\""));
                None
            }
        }
    }

    fn time(&mut self, at: &str, value: &Value) -> Option<TimeValue> {
        let Some(text) = value.as_str() else {
            self.fail(at, format!("expected a duration, got {value}"));
            return None;
        };
        if let Some(token) = self.token(at, text, "duration")? {
            return Some(Param::Token(token));
        }
        match parse_time(text) {
            Some(seconds) if PERIODS.contains(&seconds) => Some(Param::Literal(seconds)),
            _ => {
                self.fail(
                    at,
                    format!(
                        "expected a duration from 1s to 3600s (or a {{duration}} token), got \"{text}\""
                    ),
                );
                None
            }
        }
    }

    fn stops(&mut self, at: &str, stops: Option<Vec<StopDoc>>) -> Option<Vec<Stop>> {
        let Some(stops) = stops else {
            self.fail(at, "a gradient needs stops");
            return None;
        };
        if !STOPS.contains(&stops.len()) {
            self.fail(at, "a gradient has 2 to 4 stops");
            return None;
        }
        let mut out = Vec::new();
        let mut last = f32::NEG_INFINITY;
        let mut ok = true;
        for (index, stop) in stops.into_iter().enumerate() {
            let at = format!("{at}[{index}]");
            let color = self.color(&format!("{at}.color"), &stop.color);
            let alpha = self.number(&format!("{at}.alpha"), stop.alpha.as_ref(), 1.0, 0.0..=1.0);
            let offset = self.percent(&format!("{at}.at"), &stop.at);
            if let Some(offset) = offset
                && offset < last
            {
                self.fail(&at, "stops go in order along the gradient");
                ok = false;
            }
            match (color, alpha, offset) {
                (Some(color), Some(alpha), Some(offset)) => {
                    last = offset;
                    out.push(Stop {
                        color,
                        alpha,
                        at: offset,
                    });
                }
                _ => ok = false,
            }
        }
        ok.then_some(out)
    }

    fn image(&mut self, at: &str, src: Option<String>) -> Option<String> {
        let Some(src) = src else {
            self.fail(at, "an image layer needs a src");
            return None;
        };
        let extension = src
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_default();
        if !src.starts_with("assets/") || src.contains("..") || src.contains('\\') {
            self.fail(at, format!("\"{src}\" must be a file under assets/"));
            None
        } else if !IMAGE_TYPES.contains(&extension.as_str()) {
            self.fail(at, format!("\"{src}\" must be a PNG, JPEG or WebP"));
            None
        } else if !self.files.contains(&src) {
            self.fail(at, format!("\"{src}\" doesn't exist in the theme"));
            None
        } else {
            Some(src)
        }
    }

    fn shader(
        &mut self,
        at: &str,
        src: Option<String>,
    ) -> Option<(String, crate::shader::Compiled)> {
        let Some(src) = src else {
            self.fail(at, "a shader layer needs a src");
            return None;
        };
        if !src.starts_with("assets/") || src.contains("..") || src.contains('\\') {
            self.fail(at, format!("\"{src}\" must be a file under assets/"));
            return None;
        }
        if !src.to_ascii_lowercase().ends_with(".wgsl") {
            self.fail(at, format!("\"{src}\" must be a .wgsl file"));
            return None;
        }
        let Some(bytes) = self
            .files
            .contains(&src)
            .then(|| self.read.read(&src))
            .flatten()
        else {
            self.fail(at, format!("\"{src}\" doesn't exist in the theme"));
            return None;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            self.fail(at, format!("\"{src}\" is not UTF-8"));
            return None;
        };
        match crate::shader::compile(&src, &text) {
            Ok(compiled) => Some((src, compiled)),
            Err(mut problems) => {
                self.problems.append(&mut problems);
                None
            }
        }
    }

    fn layer(&mut self, at: &str, value: &Value) -> Option<Layer> {
        let doc: LayerDoc = match serde_json::from_value(value.clone()) {
            Ok(doc) => doc,
            Err(error) => {
                self.fail(at, error);
                return None;
            }
        };
        let opacity = self.number(
            &format!("{at}.opacity"),
            doc.opacity.as_ref(),
            1.0,
            0.0..=1.0,
        );
        let blend = match doc.blend.as_deref() {
            None | Some("normal") => Some(Blend::Normal),
            Some("multiply") => Some(Blend::Multiply),
            Some("screen") => Some(Blend::Screen),
            Some(other) => {
                self.fail(
                    &format!("{at}.blend"),
                    format!("\"{other}\" is not normal, multiply or screen"),
                );
                None
            }
        };
        let only = |check: &mut Self, name: &str, present: bool, kinds: &str| {
            if present {
                check.fail(at, format!("{name} is for {kinds} layers"));
            }
        };
        if doc.kind != "shader" {
            only(self, "colors", doc.colors.is_some(), "shader");
            only(self, "numbers", doc.numbers.is_some(), "shader");
            only(self, "animate", doc.animate.is_some(), "shader");
            only(self, "loop", doc.looping.is_some(), "shader");
        }
        let kind = match doc.kind.as_str() {
            "shader" => {
                only(self, "angle", doc.angle.is_some(), "linear");
                only(self, "stops", doc.stops.is_some(), "linear and radial");
                only(self, "at", doc.at.is_some(), "radial and image");
                only(self, "size", doc.size.is_some(), "radial and image");
                only(self, "color", doc.color.is_some(), "grid");
                only(self, "cell", doc.cell.is_some(), "grid");
                only(self, "line", doc.line.is_some(), "grid");
                only(self, "repeat", doc.repeat.is_some(), "image");
                if doc.motion.is_some() {
                    self.fail(
                        &format!("{at}.motion"),
                        "a shader moves itself: use animate and loop",
                    );
                }
                let inputs = |check: &mut Self, name: &str, list: &Option<Vec<Value>>| -> usize {
                    let count = list.as_ref().map_or(0, Vec::len);
                    if count > SHADER_INPUTS {
                        check.fail(&format!("{at}.{name}"), format!("at most {SHADER_INPUTS}"));
                    }
                    count
                };
                inputs(self, "colors", &doc.colors);
                inputs(self, "numbers", &doc.numbers);
                let colors: Vec<Option<ColorValue>> = doc
                    .colors
                    .iter()
                    .flatten()
                    .enumerate()
                    .map(|(index, value)| self.color(&format!("{at}.colors[{index}]"), value))
                    .collect();
                let numbers: Vec<Option<NumberValue>> = doc
                    .numbers
                    .iter()
                    .flatten()
                    .enumerate()
                    .map(|(index, value)| {
                        self.number(
                            &format!("{at}.numbers[{index}]"),
                            Some(value),
                            0.0,
                            -MAX_LENGTH..=MAX_LENGTH,
                        )
                    })
                    .collect();
                let shader = self.shader(&format!("{at}.src"), doc.src.clone());
                match (
                    shader,
                    colors.into_iter().collect::<Option<Vec<_>>>(),
                    numbers.into_iter().collect::<Option<Vec<_>>>(),
                ) {
                    (Some((src, shader)), Some(colors), Some(numbers)) => Some(Kind::Shader {
                        src,
                        shader,
                        colors,
                        numbers,
                    }),
                    _ => None,
                }
            }
            "linear" => {
                only(self, "at", doc.at.is_some(), "radial and image");
                only(self, "size", doc.size.is_some(), "radial and image");
                only(self, "color", doc.color.is_some(), "grid");
                only(self, "cell", doc.cell.is_some(), "grid");
                only(self, "line", doc.line.is_some(), "grid");
                only(self, "src", doc.src.is_some(), "image");
                only(self, "repeat", doc.repeat.is_some(), "image");
                let angle = doc.angle.unwrap_or(180.0) as f32;
                if !angle.is_finite() || angle.abs() > 3600.0 {
                    self.fail(&format!("{at}.angle"), "expected degrees");
                }
                let stops = self.stops(&format!("{at}.stops"), doc.stops);
                Some(Kind::Linear {
                    angle: angle.rem_euclid(360.0),
                    stops: stops?,
                })
            }
            "radial" => {
                only(self, "angle", doc.angle.is_some(), "linear");
                only(self, "color", doc.color.is_some(), "grid");
                only(self, "cell", doc.cell.is_some(), "grid");
                only(self, "line", doc.line.is_some(), "grid");
                only(self, "src", doc.src.is_some(), "image");
                only(self, "repeat", doc.repeat.is_some(), "image");
                let center = match &doc.at {
                    Some(at_pair) => self.pair(&format!("{at}.at"), at_pair),
                    None => Some([
                        Param::Literal(Length {
                            value: 50.0,
                            unit: Unit::Percent,
                        }),
                        Param::Literal(Length {
                            value: 50.0,
                            unit: Unit::Percent,
                        }),
                    ]),
                };
                let size = match &doc.size {
                    Some(size) => self.lengths(&format!("{at}.size"), size),
                    None => {
                        self.fail(at, "a radial layer needs a size");
                        None
                    }
                };
                let stops = self.stops(&format!("{at}.stops"), doc.stops);
                Some(Kind::Radial {
                    at: center?,
                    size: size?,
                    stops: stops?,
                })
            }
            "grid" => {
                only(self, "angle", doc.angle.is_some(), "linear");
                only(self, "stops", doc.stops.is_some(), "linear and radial");
                only(self, "at", doc.at.is_some(), "radial and image");
                only(self, "size", doc.size.is_some(), "radial and image");
                only(self, "src", doc.src.is_some(), "image");
                only(self, "repeat", doc.repeat.is_some(), "image");
                let color = match &doc.color {
                    Some(color) => self.color(&format!("{at}.color"), color),
                    None => {
                        self.fail(at, "a grid needs a color");
                        None
                    }
                };
                let cell = match &doc.cell {
                    Some(cell) => self.lengths(&format!("{at}.cell"), cell),
                    None => {
                        self.fail(at, "a grid needs a cell");
                        None
                    }
                };
                let line = match &doc.line {
                    Some(line) => self.length(&format!("{at}.line"), line),
                    None => Some(Param::Literal(Length {
                        value: 1.0,
                        unit: Unit::Px,
                    })),
                };
                Some(Kind::Grid {
                    color: color?,
                    cell: cell?,
                    line: line?,
                })
            }
            "image" => {
                only(self, "angle", doc.angle.is_some(), "linear");
                only(self, "stops", doc.stops.is_some(), "linear and radial");
                only(self, "color", doc.color.is_some(), "grid");
                only(self, "cell", doc.cell.is_some(), "grid");
                only(self, "line", doc.line.is_some(), "grid");
                let src = self.image(&format!("{at}.src"), doc.src);
                let size = match &doc.size {
                    None => Some(ImageSize::Auto),
                    Some(Value::String(word)) if word == "cover" => Some(ImageSize::Cover),
                    Some(Value::String(word)) if word == "contain" => Some(ImageSize::Contain),
                    Some(Value::String(word)) if word == "auto" => Some(ImageSize::Auto),
                    Some(size) => self
                        .lengths(&format!("{at}.size"), size)
                        .map(ImageSize::Explicit),
                };
                let position = match &doc.at {
                    Some(at_pair) => self.pair(&format!("{at}.at"), at_pair),
                    None => Some([
                        Param::Literal(Length {
                            value: 0.0,
                            unit: Unit::Percent,
                        }),
                        Param::Literal(Length {
                            value: 0.0,
                            unit: Unit::Percent,
                        }),
                    ]),
                };
                Some(Kind::Image {
                    src: src?,
                    size: size?,
                    at: position?,
                    repeat: doc.repeat.unwrap_or(true),
                })
            }
            other => {
                self.fail(
                    &format!("{at}.type"),
                    format!("\"{other}\" is not linear, radial, grid, image or shader"),
                );
                None
            }
        };
        let repeats = match &kind {
            Some(Kind::Grid { .. }) => true,
            Some(Kind::Image { repeat, .. }) => *repeat,
            _ => false,
        };
        let motion = match doc.motion {
            None if doc.kind == "shader" && doc.animate != Some(false) => {
                let period = match &doc.looping {
                    Some(value) => self.time(&format!("{at}.loop"), value),
                    None => Some(Param::Literal(3600.0)),
                };
                period.map(|period| Some(Motion::Clock { period }))
            }
            None => Some(None),
            Some(_) if doc.kind == "shader" => Some(None),
            Some(motion) => {
                let at = format!("{at}.motion");
                let by = self.pair(&format!("{at}.by"), &motion.by);
                let period = self.time(&format!("{at}.period"), &motion.period);
                match motion.kind.as_str() {
                    "drift" => by
                        .zip(period)
                        .map(|(by, period)| Some(Motion::Drift { by, period })),
                    "pan" if repeats => by
                        .zip(period)
                        .map(|(by, period)| Some(Motion::Pan { by, period })),
                    "pan" => {
                        self.fail(
                            &at,
                            "pan moves repeating content only (a grid or a repeating image); use drift",
                        );
                        None
                    }
                    other => {
                        self.fail(
                            &format!("{at}.type"),
                            format!("\"{other}\" is not drift or pan"),
                        );
                        None
                    }
                }
            }
        };
        let mask = match doc.mask {
            None => Some(None),
            Some(mask) => {
                let at = format!("{at}.mask");
                let center = self.pair(&format!("{at}.at"), &mask.at);
                let size = self.pair(&format!("{at}.size"), &mask.size);
                let from = self.percent(&format!("{at}.from"), &mask.from);
                let to = self.percent(&format!("{at}.to"), &mask.to);
                if let (Some(from), Some(to)) = (from, to)
                    && to <= from
                {
                    self.fail(&at, "to must be beyond from");
                }
                match (center, size, from, to) {
                    (Some(at), Some(size), Some(from), Some(to)) if to > from => {
                        Some(Some(Mask { at, size, from, to }))
                    }
                    _ => None,
                }
            }
        };
        Some(Layer {
            kind: kind?,
            opacity: opacity?,
            blend: blend?,
            motion: motion?,
            mask: mask?,
        })
    }
}

/// Checks `backdrop.json` (its text) against the schema. `files` are the
/// theme's, for the images it names.
pub(crate) fn parse(
    text: &str,
    files: &[String],
    read: &dyn crate::Files,
) -> Result<Composition, Vec<Problem>> {
    let doc: FileDoc =
        serde_json::from_str(text).map_err(|error| vec![Problem::json(FILE, &error)])?;
    let mut check = Check {
        files,
        read,
        problems: Vec::new(),
    };
    let base = match &doc.base {
        Some(base) => check.color("base", base),
        None => Some(Param::Token("--cs-color-surface-base".to_owned())),
    };
    let frame_rate = doc.frame_rate.map_or(30.0, |rate| rate as f32);
    if !FRAME_RATES.contains(&frame_rate) {
        check.fail("frameRate", "expected 1 to 60 frames a second");
    }
    if doc.layers.len() > MAX_LAYERS {
        check.fail(
            "layers",
            format!("a backdrop has at most {MAX_LAYERS} layers"),
        );
    }
    let layers: Vec<Option<Layer>> = doc
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| check.layer(&format!("layers[{index}]"), layer))
        .collect();
    let composition = Composition {
        base: base.unwrap_or(Param::Literal([0.0; 4])),
        frame_rate,
        layers: layers.into_iter().flatten().collect(),
    };
    let shader_layers = composition
        .layers
        .iter()
        .filter(|layer| matches!(layer.kind, Kind::Shader { .. }))
        .count();
    if shader_layers > MAX_SHADERS {
        check.fail(
            "layers",
            format!("a backdrop has at most {MAX_SHADERS} shader layers"),
        );
    }
    if composition.images().len() > MAX_IMAGES {
        check.fail(
            "layers",
            format!("a backdrop uses at most {MAX_IMAGES} images"),
        );
    }
    if check.problems.is_empty() {
        Ok(composition)
    } else {
        Err(check.problems)
    }
}

// ── CSS values ──────────────────────────────────────────────────────────

/// A token's default value (its custom property, `--cs-grid-size`), aliases
/// resolved: what to use when a computed value can't be read.
pub fn token_default(name: &str) -> Option<&'static str> {
    contract()
        .tokens
        .values()
        .find(|token| token.name == name)
        .map(|token| token.resolved.as_str())
}

/// A CSS length: a number and one of px, rem, em, %, vw, vh (or `0`).
pub fn parse_length(text: &str) -> Option<Length> {
    let text = text.trim();
    if text == "0" {
        return Some(Length {
            value: 0.0,
            unit: Unit::Px,
        });
    }
    let split = text
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(text.len());
    let (amount, unit) = text.split_at(split);
    let value: f32 = amount.trim().parse().ok().filter(|n: &f32| n.is_finite())?;
    let unit = match unit.to_ascii_lowercase().as_str() {
        "px" => Unit::Px,
        "rem" => Unit::Rem,
        "em" => Unit::Em,
        "%" => Unit::Percent,
        "vw" => Unit::Vw,
        "vh" => Unit::Vh,
        _ => return None,
    };
    Some(Length { value, unit })
}

/// A CSS duration in seconds: `40s`, `500ms`.
pub fn parse_time(text: &str) -> Option<f32> {
    let text = text.trim();
    let (amount, scale) = if let Some(ms) = text.strip_suffix("ms") {
        (ms, 0.001)
    } else {
        (text.strip_suffix('s')?, 1.0)
    };
    let value: f32 = amount.trim().parse().ok()?;
    value.is_finite().then_some(value * scale)
}

/// A CSS number.
pub fn parse_number(text: &str) -> Option<f32> {
    text.trim().parse().ok().filter(|n: &f32| n.is_finite())
}

/// A CSS colour as a computed custom property holds it: `#rgb[a]`,
/// `#rrggbb[aa]`, `rgb()`/`rgba()`, `hsl()`/`hsla()` (comma or space
/// separated, alpha as a number or percentage), `transparent`, `black`
/// and `white`. Other syntaxes give `None`.
pub fn parse_color(text: &str) -> Option<Rgba> {
    let text = text.trim().to_ascii_lowercase();
    match text.as_str() {
        "transparent" => return Some([0.0; 4]),
        "black" => return Some([0.0, 0.0, 0.0, 1.0]),
        "white" => return Some([1.0; 4]),
        _ => {}
    }
    if let Some(digits) = text.strip_prefix('#') {
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let nibble = |at: usize| u8::from_str_radix(&digits[at..=at], 16).ok();
        let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        let channels: Vec<u8> = match digits.len() {
            3 | 4 => (0..digits.len())
                .map(|at| nibble(at).map(|n| n * 17))
                .collect::<Option<_>>()?,
            6 | 8 => (0..digits.len() / 2)
                .map(|at| byte(at * 2))
                .collect::<Option<_>>()?,
            _ => return None,
        };
        let alpha = channels.get(3).map_or(1.0, |a| f32::from(*a) / 255.0);
        return Some([
            f32::from(channels[0]) / 255.0,
            f32::from(channels[1]) / 255.0,
            f32::from(channels[2]) / 255.0,
            alpha,
        ]);
    }
    let (name, rest) = text.split_once('(')?;
    let inner = rest.strip_suffix(')')?;
    let (channels, alpha) = match inner.split_once('/') {
        Some((channels, alpha)) => (channels, Some(alpha)),
        None => (inner, None),
    };
    let mut parts: Vec<&str> = channels
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    let alpha = match (alpha, parts.len()) {
        (Some(alpha), 3) => Some(alpha.trim()),
        (None, 4) => parts.pop(),
        (None, 3) => None,
        _ => return None,
    };
    let alpha = match alpha {
        None => 1.0,
        Some(alpha) => match alpha.strip_suffix('%') {
            Some(percent) => percent.trim().parse::<f32>().ok()? / 100.0,
            None => alpha.parse::<f32>().ok()?,
        },
    }
    .clamp(0.0, 1.0);
    let channel = |part: &str| -> Option<f32> {
        match part.strip_suffix('%') {
            Some(percent) => Some(percent.parse::<f32>().ok()? / 100.0),
            None => Some(part.parse::<f32>().ok()? / 255.0),
        }
    };
    let [r, g, b] = match name.trim() {
        "rgb" | "rgba" => [channel(parts[0])?, channel(parts[1])?, channel(parts[2])?],
        "hsl" | "hsla" => {
            let hue: f32 = parts[0].trim_end_matches("deg").parse().ok()?;
            let percent = |part: &str| -> Option<f32> {
                Some(part.strip_suffix('%').unwrap_or(part).parse::<f32>().ok()? / 100.0)
            };
            hsl(hue, percent(parts[1])?, percent(parts[2])?)
        }
        _ => return None,
    };
    let clamp = |v: f32| v.clamp(0.0, 1.0);
    Some([clamp(r), clamp(g), clamp(b), alpha])
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let hue = hue.rem_euclid(360.0) / 30.0;
    let a = saturation.clamp(0.0, 1.0) * lightness.min(1.0 - lightness);
    let f = |n: f32| {
        let k = (n + hue) % 12.0;
        lightness - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [f(0.0), f(8.0), f(4.0)]
}
