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

mod parse;
mod reference;
mod values;

pub(crate) use parse::parse;
pub use reference::{FIELDS, Field, reference};
pub use values::{parse_color, parse_length, parse_number, parse_time, token_default};

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
