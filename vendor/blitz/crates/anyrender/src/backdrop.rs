//! A backdrop: layers a [`WindowRenderer`](crate::WindowRenderer) draws
//! beneath the scene itself, on the GPU, and animates without the scene
//! being painted again.
//!
//! The embedder describes the layers once, in surface pixels, and the
//! renderer keeps the last scene it drew (a transparent texture). Every
//! frame it presents is the backdrop with that scene composited over it,
//! and a frame of the backdrop alone ([`render_backdrop`]) only advances its
//! motion: nothing is painted, laid out or rasterised on the CPU.
//!
//! [`render_backdrop`]: crate::WindowRenderer::render_backdrop

use kurbo::{Point, Rect, Vec2};
use peniko::Color;
use std::sync::Arc;
use std::time::Duration;

/// Most layers a backdrop draws; later ones are ignored.
pub const MAX_BACKDROP_LAYERS: usize = 8;
/// Most images a backdrop's layers use; layers with others are ignored.
pub const MAX_BACKDROP_IMAGES: usize = 4;
/// Most colour stops a gradient uses; later ones are ignored.
pub const MAX_BACKDROP_STOPS: usize = 4;
/// Most distinct shaders a backdrop's layers run; layers with others are
/// ignored.
pub const MAX_BACKDROP_SHADERS: usize = 2;
/// Colours and numbers a shader layer passes its shader.
pub const BACKDROP_SHADER_INPUTS: usize = 4;

/// The input a [`BackdropShader`]'s function takes, as it is declared for
/// it (`CsInput`).
pub const BACKDROP_SHADER_INPUT: &str = "struct CsInput {
    pixel: vec2<f32>,
    size: vec2<f32>,
    time: f32,
    scale: f32,
    colors: array<vec4<f32>, 4>,
    numbers: vec4<f32>,
}
";

/// What a renderer draws beneath every frame. Lengths and positions are in
/// surface pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop {
    /// Fills the whole surface first: the window's own background.
    pub base: Color,
    /// Where the layers are drawn (they're clipped to it).
    pub area: Rect,
    /// Multiplies every layer: the backdrop's own opacity. The base is
    /// drawn at full opacity.
    pub opacity: f32,
    /// Drawn in order, each over the ones before.
    pub layers: Vec<BackdropLayer>,
    /// How often the backdrop needs a frame while something in it moves:
    /// `None` keeps it still, at the start of every motion.
    pub frame_interval: Option<Duration>,
}

impl Backdrop {
    /// A backdrop of `base` alone.
    pub fn flat(base: Color) -> Self {
        Self {
            base,
            area: Rect::ZERO,
            opacity: 1.0,
            layers: Vec::new(),
            frame_interval: None,
        }
    }

    /// Whether any layer that can be seen moves, so the backdrop needs
    /// frames of its own.
    pub fn moves(&self) -> bool {
        self.frame_interval.is_some()
            && self.opacity > 0.0
            && self
                .layers
                .iter()
                .take(MAX_BACKDROP_LAYERS)
                .any(|layer| layer.motion.is_some() && layer.opacity > 0.0)
    }
}

/// One layer of a [`Backdrop`].
#[derive(Clone, Debug, PartialEq)]
pub struct BackdropLayer {
    pub kind: BackdropLayerKind,
    /// 0 hides the layer.
    pub opacity: f32,
    pub blend: BackdropBlend,
    /// How the layer's content moves; its mask stays where it is.
    pub motion: Option<BackdropMotion>,
    /// Fades the layer out away from an ellipse's centre.
    pub mask: Option<BackdropMask>,
}

/// What a layer draws.
#[derive(Clone, Debug, PartialEq)]
pub enum BackdropLayerKind {
    /// A CSS `linear-gradient` over the area: `angle` in degrees, 0 pointing
    /// up and 90 right, the gradient line as long as CSS makes it.
    Linear {
        angle: f32,
        stops: Vec<BackdropStop>,
    },
    /// A CSS `radial-gradient` ellipse: stops are fractions of `radius`.
    Radial {
        center: Point,
        radius: Vec2,
        stops: Vec<BackdropStop>,
    },
    /// Lines `line` wide along the top and left of every `cell`, as the
    /// two-gradient CSS grid draws them; `origin` is where a cell starts.
    Grid {
        color: Color,
        cell: Vec2,
        line: f32,
        origin: Point,
    },
    /// An embedder's WGSL function over the area: the colour (straight
    /// alpha) at each pixel, given a [`BACKDROP_SHADER_INPUT`] with `colors`
    /// and `numbers`, `scale` and, as `time`, the layer's
    /// [`BackdropMotion::Clock`].
    Shader {
        shader: BackdropShader,
        colors: Vec<Color>,
        numbers: [f32; BACKDROP_SHADER_INPUTS],
        /// Surface pixels per CSS pixel.
        scale: f32,
    },
    /// An image `size` large with its top left at `origin`, tiled in both
    /// directions when `repeat`.
    Image {
        image: BackdropImage,
        origin: Point,
        size: Vec2,
        repeat: bool,
    },
}

/// A gradient's colour at `offset` (0 at its start, 1 at its end).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackdropStop {
    pub offset: f32,
    pub color: Color,
}

/// How a layer combines with the layers under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackdropBlend {
    #[default]
    Normal,
    Multiply,
    Screen,
}

/// How a layer's content moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackdropMotion {
    /// Out by `by` and back again, eased, every `period`.
    Drift { by: Vec2, period: Duration },
    /// Steadily by `by` every `period`: for repeating content (a grid, a
    /// tiled image), which the renderer wraps at its tile so it never jumps.
    Pan { by: Vec2, period: Duration },
    /// A shader's clock: its `time` (the offset's `x`, in seconds) runs
    /// from 0 to `period`, then again.
    Clock { period: Duration },
}

impl BackdropMotion {
    /// The offset `elapsed` into the motion.
    pub fn offset(&self, elapsed: Duration) -> Vec2 {
        match *self {
            BackdropMotion::Drift { by, period } => {
                let phase = phase(elapsed, period);
                by * ((1.0 - (std::f64::consts::TAU * phase).cos()) / 2.0)
            }
            BackdropMotion::Pan { by, period } if !period.is_zero() => {
                by * (elapsed.as_secs_f64() / period.as_secs_f64())
            }
            BackdropMotion::Pan { .. } => Vec2::ZERO,
            BackdropMotion::Clock { period } if !period.is_zero() => {
                Vec2::new(elapsed.as_secs_f64() % period.as_secs_f64(), 0.0)
            }
            BackdropMotion::Clock { .. } => Vec2::ZERO,
        }
    }
}

fn phase(elapsed: Duration, period: Duration) -> f64 {
    if period.is_zero() {
        return 0.0;
    }
    (elapsed.as_secs_f64() / period.as_secs_f64()).fract()
}

/// An elliptical mask: opaque inside `from` (a fraction of `radius`),
/// fading linearly to nothing at `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackdropMask {
    pub center: Point,
    pub radius: Vec2,
    pub from: f32,
    pub to: f32,
}

/// A WGSL function from the embedder: `wgsl` declares `entry`, a
/// `fn(CsInput) -> vec4<f32>`, and whatever it calls, with names that clash
/// with nothing else (`CsInput` is declared by the renderer). It may declare
/// no bindings or entry points. Shaders with the same `id` are the same.
#[derive(Clone, Debug)]
pub struct BackdropShader {
    pub id: u64,
    pub wgsl: Arc<str>,
    pub entry: Arc<str>,
}

impl PartialEq for BackdropShader {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

/// Decoded image pixels: RGBA8, premultiplied. Images with the same `id`
/// are the same pixels, uploaded once.
#[derive(Clone, Debug)]
pub struct BackdropImage {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl PartialEq for BackdropImage {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.width == other.width && self.height == other.height
    }
}
