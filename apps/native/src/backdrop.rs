//! The window's backdrop on the GPU (#149).
//!
//! A theme can describe its backdrop as data (`backdrop.json`, read by
//! `caskade_theme::backdrop`). Then the renderer draws it beneath the
//! document in one GPU pass, and moves it on its own clock: a backdrop
//! frame composites the last painted document over the layers without
//! resolving, laying out or painting anything (`WindowRenderer::
//! render_backdrop`, scheduled by Blitz's shell).
//!
//! The document keeps `[data-cs="backdrop"]` as the styling anchor. Before
//! each frame is painted ([`Sync::before_paint`]) the composition's
//! `{token}` references are read from the backdrop element's computed
//! style, so the theme's tokens, the user's accent, Appearance's override
//! sets and custom CSS that sets tokens all reach the GPU. So does the
//! element's `display`, `visibility` and `opacity`: a hidden backdrop
//! draws only the base colour and asks for no frames. This happens when
//! the look, the window's size or the mounted backdrop changed, never on
//! a backdrop frame.
//!
//! While the GPU draws it, the document's own backdrop isn't painted (the
//! [`GPU_SHEET`]). Without a usable GPU, or for a theme whose backdrop is
//! only CSS, the document draws its CSS backdrop with its animations
//! stopped ([`STILL_SHEET`]): the theme's look at the start of its motion,
//! with no ambient repaint loop.
//!
//! `CASKADE_BACKDROP=still` draws the CSS backdrop still on a GPU too, and
//! `CASKADE_BACKDROP=css` draws it animated as before #149: both for
//! comparing and measuring.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyrender::{
    Backdrop as GpuBackdrop, BackdropBlend, BackdropImage, BackdropLayer, BackdropLayerKind,
    BackdropMask, BackdropMotion, BackdropShader, BackdropStop, WindowRenderer,
};
use blitz_dom::{BaseDocument, NodeId};
use caskade_core::themes::ThemeService;
use caskade_theme::backdrop::{
    self as schema, Blend, ColorValue, Composition, ImageSize, Kind, Length, LengthValue, Motion,
    NumberValue, Param, TimeValue, Unit,
};
use dioxus_native::prelude::*;
use peniko::Color;
use peniko::kurbo::{Point, Rect, Vec2};

/// Unlayered, so it beats the theme's and the user's layers: the GPU draws
/// the backdrop and the window's own colour, so the document paints
/// neither. Only what paints is turned off; `display`, `visibility` and
/// `opacity` stay the theme's and the user's, and are read.
pub const GPU_SHEET: &str = r#"/* The GPU draws the backdrop (#149). */
html,
body {
  background: transparent;
}
[data-cs="backdrop"] {
  background: none;
  border-color: transparent;
  box-shadow: none;
  outline: none;
  filter: none;
  backdrop-filter: none;
  animation: none;
  transition: none;
}
[data-cs="backdrop"]::before,
[data-cs="backdrop"]::after {
  content: none;
}
"#;

/// Unlayered: the document draws the theme's CSS backdrop, still.
pub const STILL_SHEET: &str = r#"/* The backdrop is drawn still (#149). */
[data-cs="backdrop"],
[data-cs="backdrop"]::before,
[data-cs="backdrop"]::after {
  animation-play-state: paused;
  transition: none;
}
"#;

/// How the backdrop may be drawn (`CASKADE_BACKDROP`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Requested {
    /// On the GPU when there is one and the theme has a composition;
    /// otherwise the CSS backdrop, still.
    Auto,
    /// The CSS backdrop, still, even on a GPU.
    Still,
    /// The CSS backdrop, animated (as before #149).
    Css,
}

fn requested() -> Requested {
    static REQUESTED: std::sync::OnceLock<Requested> = std::sync::OnceLock::new();
    *REQUESTED.get_or_init(|| match std::env::var("CASKADE_BACKDROP").as_deref() {
        Ok("still") => Requested::Still,
        Ok("css") => Requested::Css,
        _ => Requested::Auto,
    })
}

/// A theme's composition with its images decoded, built with the look,
/// off the main thread.
#[derive(Debug)]
pub struct Prepared {
    /// The theme whose `backdrop.json` it is.
    pub theme: String,
    pub composition: Composition,
    images: Vec<(String, BackdropImage)>,
}

/// `theme`'s backdrop composition, or `None` when it draws its backdrop
/// in CSS.
pub fn prepare(themes: &ThemeService, theme: &str) -> Option<Arc<Prepared>> {
    let backdrop = themes.backdrop(theme)?;
    let images = backdrop
        .images
        .iter()
        .filter_map(|(path, bytes)| {
            let image = decode(&backdrop.theme, path, bytes);
            if image.is_none() {
                tracing::warn!(theme = backdrop.theme, path, "backdrop image not drawn");
            }
            Some((path.clone(), image?))
        })
        .collect();
    Some(Arc::new(Prepared {
        theme: backdrop.theme,
        composition: backdrop.composition,
        images,
    }))
}

/// An image's pixels, premultiplied, if it decodes and isn't too large.
fn decode(theme: &str, path: &str, bytes: &[u8]) -> Option<BackdropImage> {
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    if width == 0
        || height == 0
        || width > schema::MAX_IMAGE_SIDE
        || height > schema::MAX_IMAGE_SIDE
    {
        return None;
    }
    let mut rgba = image.into_raw();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (theme, path, bytes.len()).hash(&mut hasher);
    bytes.hash(&mut hasher);
    Some(BackdropImage {
        id: hasher.finish(),
        width,
        height,
        rgba: rgba.into(),
    })
}

/// Counts mounts and unmounts of [`Backdrop`]: a new backdrop element is
/// read again.
static MOUNTS: AtomicU64 = AtomicU64::new(0);
/// Frames the document painted (each after a resolve), for the smoke
/// test's frame counts.
static DOCUMENT_FRAMES: AtomicU64 = AtomicU64::new(0);
/// Whether the GPU draws the backdrop now.
static ON_GPU: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Frames the document painted so far.
pub fn document_frames() -> u64 {
    DOCUMENT_FRAMES.load(Ordering::Relaxed)
}

/// Whether the GPU draws the backdrop.
pub fn on_gpu() -> bool {
    ON_GPU.load(Ordering::Relaxed)
}

/// The backdrop's styling anchor, `[data-cs="backdrop"]`. Use it rather
/// than the bare element: mounting or unmounting it reads the backdrop
/// again before the next frame.
#[component]
pub fn Backdrop() -> Element {
    use_hook(|| MOUNTS.fetch_add(1, Ordering::Relaxed));
    use_drop(|| {
        MOUNTS.fetch_add(1, Ordering::Relaxed);
    });
    rsx! {
        div { "data-cs": "backdrop", "aria-hidden": "true" }
    }
}

/// What the last frame's backdrop was made from; when none of it changed,
/// the renderer's backdrop is still right.
#[derive(Clone, Debug, PartialEq)]
struct Inputs {
    look: u64,
    mounts: u64,
    size: (u32, u32),
    scale: f32,
    gpu: bool,
}

/// Keeps the renderer's backdrop in step with the document, before each
/// frame is painted (Blitz's `BeforePaint`).
#[derive(Default)]
pub struct Sync {
    last: Option<Inputs>,
}

impl Sync {
    /// Reads the backdrop again if its inputs changed: the sheet that keeps
    /// the document from painting it (or stills it), and the composition's
    /// values from the computed style, into `renderer`.
    pub fn before_paint<R: WindowRenderer>(
        &mut self,
        doc: &mut BaseDocument,
        renderer: &mut R,
        time: f64,
    ) {
        DOCUMENT_FRAMES.fetch_add(1, Ordering::Relaxed);
        let prepared = crate::theme::last_backdrop();
        let requested = requested();
        let gpu =
            requested == Requested::Auto && prepared.is_some() && renderer.supports_backdrop();
        let viewport = doc.viewport();
        let inputs = Inputs {
            look: crate::theme::look_generation(),
            mounts: MOUNTS.load(Ordering::Relaxed),
            size: viewport.window_size,
            scale: viewport.scale(),
            gpu,
        };
        if self.last.as_ref() == Some(&inputs) {
            return;
        }
        let first = self.last.is_none();
        let was_gpu = self.last.as_ref().is_some_and(|last| last.gpu);
        self.last = Some(inputs);
        ON_GPU.store(gpu, Ordering::Relaxed);
        let sheet = match (gpu, requested) {
            (true, _) => GPU_SHEET,
            (false, Requested::Css) => "",
            (false, _) => STILL_SHEET,
        };
        if crate::theme::set_backdrop_sheet(doc, sheet) {
            doc.resolve(time);
        }
        if first || was_gpu != gpu {
            tracing::info!(
                gpu,
                theme = prepared.as_ref().map(|prepared| prepared.theme.as_str()),
                requested = ?requested,
                "backdrop"
            );
        }
        match prepared.filter(|_| gpu) {
            Some(prepared) => renderer.set_backdrop(Some(resolve(doc, &prepared))),
            None => renderer.set_backdrop(None),
        }
    }
}

/// The renderer's backdrop for `prepared`, its values read from the
/// document's computed styles.
pub fn resolve(doc: &BaseDocument, prepared: &Prepared) -> GpuBackdrop {
    let element = backdrop_element(doc);
    let read_on = element.unwrap_or_else(|| doc.root_element().id);
    let values = Values {
        doc,
        node: read_on,
        context: Context::new(doc, element),
    };
    let composition = &prepared.composition;
    let base = values.color(&composition.base);
    let mut backdrop = GpuBackdrop::flat(base);
    backdrop.area = values.context.area;
    // A hidden backdrop shows the window's colour alone, and needs no frames.
    let Some(element) = element else {
        backdrop.opacity = 0.0;
        return backdrop;
    };
    let style = |property: &str| doc.resolved_style_value(element, property);
    let hidden = doc
        .get_node(element)
        .is_none_or(|node| node.primary_styles().is_none())
        || style("display").trim() == "none"
        || matches!(style("visibility").trim(), "hidden" | "collapse");
    backdrop.opacity = if hidden {
        0.0
    } else {
        schema::parse_number(&style("opacity")).map_or(1.0, |n| n.clamp(0.0, 1.0))
    };
    let paused = values.raw("--cs-motion-ambient").trim() == "paused";
    backdrop.frame_interval =
        (!paused).then(|| Duration::from_secs_f32(1.0 / composition.frame_rate));
    backdrop.layers = composition
        .layers
        .iter()
        .filter_map(|layer| values.layer(layer, prepared))
        .collect();
    backdrop
}

/// The first `[data-cs="backdrop"]` that is displayed, or the first.
fn backdrop_element(doc: &BaseDocument) -> Option<NodeId> {
    let found = doc.query_selector_all("[data-cs=\"backdrop\"]").ok()?;
    found
        .iter()
        .copied()
        .find(|&id| {
            doc.get_node(id)
                .is_some_and(|node| node.primary_styles().is_some())
        })
        .or_else(|| found.first().copied())
}

/// Lengths' units in surface pixels, and the backdrop's box.
struct Context {
    area: Rect,
    /// Surface pixels per CSS pixel.
    scale: f64,
    rem: f64,
    em: f64,
    vw: f64,
    vh: f64,
}

impl Context {
    fn new(doc: &BaseDocument, element: Option<NodeId>) -> Self {
        let viewport = doc.viewport();
        let scale = viewport.scale_f64();
        let (width, height) = viewport.window_size;
        let window = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
        let font_size = |node: NodeId| {
            schema::parse_length(&doc.resolved_style_value(node, "font-size"))
                .filter(|length| length.unit == Unit::Px)
                .map_or(16.0, |length| f64::from(length.value))
        };
        let rem = font_size(doc.root_element().id) * scale;
        let area = element
            .and_then(|id| doc.get_node(id))
            .map(|node| {
                let at = node.absolute_position(0.0, 0.0);
                let size = node.final_layout().size;
                Rect::from_origin_size(
                    (f64::from(at.x) * scale, f64::from(at.y) * scale),
                    (
                        f64::from(size.width) * scale,
                        f64::from(size.height) * scale,
                    ),
                )
            })
            .filter(|area| area.area() > 0.0)
            .unwrap_or(window);
        Self {
            area,
            scale,
            rem,
            em: element.map_or(rem, |id| font_size(id) * scale),
            vw: f64::from(width) / 100.0,
            vh: f64::from(height) / 100.0,
        }
    }

    /// A length along x (`horizontal`) or y, in surface pixels; `%` is of
    /// the backdrop's width or height.
    fn length(&self, length: Length, horizontal: bool) -> f64 {
        let value = f64::from(length.value);
        match length.unit {
            Unit::Px => value * self.scale,
            Unit::Rem => value * self.rem,
            Unit::Em => value * self.em,
            Unit::Percent => {
                value / 100.0
                    * if horizontal {
                        self.area.width()
                    } else {
                        self.area.height()
                    }
            }
            Unit::Vw => value * self.vw,
            Unit::Vh => value * self.vh,
        }
    }
}

/// Reads a composition's values on the backdrop element.
struct Values<'a> {
    doc: &'a BaseDocument,
    node: NodeId,
    context: Context,
}

impl Values<'_> {
    fn raw(&self, name: &str) -> String {
        self.doc.resolved_style_value(self.node, name)
    }

    /// A token's computed value, or the contract's default when it can't
    /// be read as `parse` reads it.
    fn token<T>(&self, name: &str, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
        let value = parse(&self.raw(name));
        if value.is_none() {
            tracing::debug!(token = name, "backdrop token unreadable: its default");
        }
        value.or_else(|| schema::token_default(name).and_then(parse))
    }

    fn color(&self, value: &ColorValue) -> Color {
        let rgba = match value {
            Param::Literal(rgba) => Some(*rgba),
            Param::Token(name) => self.token(name, schema::parse_color),
        };
        Color::new(rgba.unwrap_or([0.0; 4]))
    }

    fn number(&self, value: &NumberValue) -> f32 {
        match value {
            Param::Literal(n) => *n,
            Param::Token(name) => self.token(name, schema::parse_number).unwrap_or(0.0),
        }
    }

    fn time(&self, value: &TimeValue) -> Duration {
        let seconds = match value {
            Param::Literal(seconds) => *seconds,
            Param::Token(name) => self.token(name, schema::parse_time).unwrap_or(0.0),
        };
        Duration::from_secs_f32(seconds.clamp(0.0, 3600.0))
    }

    fn length(&self, value: &LengthValue, horizontal: bool) -> f64 {
        let length = match value {
            Param::Literal(length) => Some(*length),
            Param::Token(name) => self.token(name, schema::parse_length),
        };
        length.map_or(0.0, |length| self.context.length(length, horizontal))
    }

    fn vec(&self, [x, y]: &[LengthValue; 2]) -> Vec2 {
        Vec2::new(self.length(x, true), self.length(y, false))
    }

    /// A point `at` from the backdrop's top left.
    fn point(&self, at: &[LengthValue; 2]) -> Point {
        self.context.area.origin() + self.vec(at)
    }

    fn stops(&self, stops: &[schema::Stop]) -> Vec<BackdropStop> {
        stops
            .iter()
            .map(|stop| {
                let color = self.color(&stop.color);
                let alpha = self.number(&stop.alpha).clamp(0.0, 1.0);
                BackdropStop {
                    offset: stop.at,
                    color: color.multiply_alpha(alpha),
                }
            })
            .collect()
    }

    fn layer(&self, layer: &schema::Layer, prepared: &Prepared) -> Option<BackdropLayer> {
        let area = self.context.area;
        let kind = match &layer.kind {
            Kind::Linear { angle, stops } => BackdropLayerKind::Linear {
                angle: *angle,
                stops: self.stops(stops),
            },
            Kind::Radial { at, size, stops } => BackdropLayerKind::Radial {
                center: self.point(at),
                radius: self.vec(size),
                stops: self.stops(stops),
            },
            Kind::Grid { color, cell, line } => BackdropLayerKind::Grid {
                color: self.color(color),
                cell: self.vec(cell),
                line: self.length(line, true) as f32,
                origin: area.origin(),
            },
            Kind::Shader {
                shader,
                colors,
                numbers,
                ..
            } => {
                let mut inputs = [0.0; anyrender::backdrop::BACKDROP_SHADER_INPUTS];
                for (input, number) in inputs.iter_mut().zip(numbers) {
                    *input = self.number(number);
                }
                BackdropLayerKind::Shader {
                    shader: BackdropShader {
                        id: shader.id,
                        wgsl: Arc::clone(&shader.wgsl),
                        entry: shader.entry.as_str().into(),
                    },
                    colors: colors.iter().map(|color| self.color(color)).collect(),
                    numbers: inputs,
                    scale: self.context.scale as f32,
                }
            }
            Kind::Image {
                src,
                size,
                at,
                repeat,
            } => {
                let (_, image) = prepared.images.iter().find(|(path, _)| path == src)?;
                let natural = Vec2::new(
                    f64::from(image.width) * self.context.scale,
                    f64::from(image.height) * self.context.scale,
                );
                let size = match size {
                    ImageSize::Auto => natural,
                    ImageSize::Cover | ImageSize::Contain => {
                        let x = area.width() / natural.x;
                        let y = area.height() / natural.y;
                        let fit = if matches!(size, ImageSize::Cover) {
                            x.max(y)
                        } else {
                            x.min(y)
                        };
                        natural * fit
                    }
                    ImageSize::Explicit(lengths) => self.vec(lengths),
                };
                // As `background-position`: a percentage aligns that point
                // of the image with the same point of the box.
                let place = |value: &LengthValue, free: f64, horizontal: bool| match value {
                    Param::Literal(Length {
                        value,
                        unit: Unit::Percent,
                    }) => free * f64::from(*value) / 100.0,
                    other => self.length(other, horizontal),
                };
                let origin = area.origin()
                    + Vec2::new(
                        place(&at[0], area.width() - size.x, true),
                        place(&at[1], area.height() - size.y, false),
                    );
                BackdropLayerKind::Image {
                    image: image.clone(),
                    origin,
                    size,
                    repeat: *repeat,
                }
            }
        };
        let motion = layer.motion.as_ref().map(|motion| match motion {
            Motion::Drift { by, period } => BackdropMotion::Drift {
                by: self.vec(by),
                period: self.time(period),
            },
            Motion::Pan { by, period } => BackdropMotion::Pan {
                by: self.vec(by),
                period: self.time(period),
            },
            Motion::Clock { period } => BackdropMotion::Clock {
                period: self.time(period),
            },
        });
        let mask = layer.mask.as_ref().map(|mask| BackdropMask {
            center: self.point(&mask.at),
            radius: self.vec(&mask.size),
            from: mask.from,
            to: mask.to,
        });
        Some(BackdropLayer {
            kind,
            opacity: self.number(&layer.opacity).clamp(0.0, 1.0),
            blend: match layer.blend {
                Blend::Normal => BackdropBlend::Normal,
                Blend::Multiply => BackdropBlend::Multiply,
                Blend::Screen => BackdropBlend::Screen,
            },
            motion,
            mask,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{apply, look};
    use blitz_dom::DocumentConfig;
    use blitz_traits::shell::{ColorScheme, Viewport};
    use caskade_core::settings::AppearanceSettings;
    use dioxus_native::DioxusDocument;

    fn root() -> Element {
        rsx! {
            Backdrop {}
        }
    }

    struct Window {
        document: DioxusDocument,
        prepared: Arc<Prepared>,
    }

    impl Window {
        /// A window with `theme` and `prefs` applied, resolved.
        fn open(theme: &str, prefs: AppearanceSettings) -> Self {
            let folder = tempfile::tempdir().unwrap();
            let themes = ThemeService::new(folder.path());
            let active = themes.resolve(theme);
            assert_eq!(active.id, theme);
            let look = look(&themes, theme, active.modes, prefs, false, false);
            let mut document = DioxusDocument::new(
                VirtualDom::new(root),
                DocumentConfig {
                    viewport: Some(Viewport::new(1280, 800, 1.0, ColorScheme::Dark)),
                    base_url: Some(crate::theme::BASE_URL.to_owned()),
                    ..Default::default()
                },
            );
            apply(&mut document.inner.borrow_mut(), &look);
            document.initial_build();
            document.inner.borrow_mut().resolve(0.0);
            let prepared = look.backdrop.clone().expect("the theme has a composition");
            Self { document, prepared }
        }

        fn backdrop(&self) -> GpuBackdrop {
            resolve(&self.document.inner.borrow(), &self.prepared)
        }

        fn computed(&self, selector: &str, property: &str) -> String {
            let doc = self.document.inner.borrow();
            let node = doc.query_selector(selector).unwrap().unwrap();
            doc.resolved_style_value(node, property).trim().to_owned()
        }

        /// Gives the backdrop's sheet `css` and resolves.
        fn sheet(&mut self, css: &str) {
            let mut doc = self.document.inner.borrow_mut();
            crate::theme::set_backdrop_sheet(&mut doc, css);
            doc.resolve(0.0);
        }
    }

    fn rgba(color: Color) -> [f32; 4] {
        color.components
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-3)
    }

    fn hex(text: &str) -> [f32; 4] {
        schema::parse_color(text).unwrap()
    }

    fn grid(backdrop: &GpuBackdrop) -> (&BackdropLayer, Vec2) {
        backdrop
            .layers
            .iter()
            .find_map(|layer| match &layer.kind {
                BackdropLayerKind::Grid { cell, .. } => Some((layer, *cell)),
                _ => None,
            })
            .expect("a grid")
    }

    /// The first glow's first stop.
    fn glow(backdrop: &GpuBackdrop) -> [f32; 4] {
        backdrop
            .layers
            .iter()
            .find_map(|layer| match &layer.kind {
                BackdropLayerKind::Radial { stops, .. } if layer.motion.is_some() => {
                    Some(rgba(stops[0].color))
                }
                _ => None,
            })
            .expect("a drifting glow")
    }

    #[test]
    fn nexus_reads_its_tokens_from_the_computed_style() {
        let window = Window::open("caskade.default", AppearanceSettings::default());
        assert_eq!(window.prepared.theme, "caskade.default");
        let backdrop = window.backdrop();
        assert!(close(rgba(backdrop.base), hex("#060914")));
        assert_eq!(backdrop.area, Rect::new(0.0, 0.0, 1280.0, 800.0));
        assert_eq!(backdrop.opacity, 1.0);
        assert_eq!(backdrop.layers.len(), 3);
        let (layer, cell) = grid(&backdrop);
        assert_eq!(cell, Vec2::new(48.0, 48.0));
        assert!((layer.opacity - 0.07).abs() < 1e-6);
        let mask = layer.mask.unwrap();
        assert_eq!(mask.center, Point::new(1280.0 * 0.6, 800.0 * 0.3));
        let mut accent = hex("#5b5ff7");
        accent[3] = 0.22;
        assert!(close(glow(&backdrop), accent));
        assert_eq!(
            backdrop.frame_interval,
            Some(Duration::from_secs_f32(1.0 / 6.0))
        );
        assert!(backdrop.moves());
    }

    #[test]
    fn appearance_reaches_the_gpu_through_the_tokens() {
        // The user's accent, as checked against the theme's surfaces.
        let prefs = AppearanceSettings {
            accent: Some("#ffe14d".into()),
            ..Default::default()
        };
        let window = Window::open("caskade.default", prefs);
        let mut accent = hex(&window.computed(":root", "--cs-color-accent"));
        assert!(!close(accent, hex("#5b5ff7")));
        accent[3] = 0.22;
        assert!(close(glow(&window.backdrop()), accent));
        // Reduce effects hides the grid; the glows stay, as in theme.css.
        let prefs = AppearanceSettings {
            reduce_effects: true,
            ..Default::default()
        };
        let backdrop = Window::open("caskade.default", prefs).backdrop();
        assert_eq!(grid(&backdrop).0.opacity, 0.0);
        assert!(backdrop.moves());
        // Reduced motion: still, so no frames of its own.
        let prefs = AppearanceSettings {
            motion: "reduced".into(),
            ..Default::default()
        };
        let backdrop = Window::open("caskade.default", prefs).backdrop();
        assert_eq!(backdrop.frame_interval, None);
        assert!(!backdrop.moves());
    }

    #[test]
    fn custom_css_restyles_the_gpu_backdrop_through_the_contract() {
        let with_css = |css: &str| {
            let prefs = AppearanceSettings {
                custom_css: css.into(),
                custom_css_enabled: true,
                ..Default::default()
            };
            Window::open("caskade.default", prefs).backdrop()
        };
        // Tokens, globally or on the backdrop alone.
        let backdrop = with_css(
            ":root { --cs-grid-size: 24px; } [data-cs=\"backdrop\"] { --cs-color-accent: #ff0000; }",
        );
        assert_eq!(grid(&backdrop).1, Vec2::new(24.0, 24.0));
        assert!(close(glow(&backdrop), [1.0, 0.0, 0.0, 0.22]));
        // Hiding the anchor hides the layers and stops their frames.
        for css in [
            "[data-cs=\"backdrop\"] { display: none; }",
            "[data-cs=\"backdrop\"] { visibility: hidden; }",
        ] {
            let backdrop = with_css(css);
            assert_eq!(backdrop.opacity, 0.0, "{css}");
            assert!(!backdrop.moves(), "{css}");
        }
        assert_eq!(
            with_css("[data-cs=\"backdrop\"] { opacity: 0.5; }").opacity,
            0.5
        );
    }

    #[test]
    fn every_built_in_theme_has_a_composition_the_window_resolves() {
        for (theme, moves) in [
            ("caskade.cobalt", true),
            ("caskade.emerald", true),
            ("caskade.engage", false),
        ] {
            let window = Window::open(theme, AppearanceSettings::default());
            assert_eq!(window.prepared.theme, theme);
            let backdrop = window.backdrop();
            let base = hex(&window.computed(":root", "--cs-color-surface-base"));
            assert!(close(rgba(backdrop.base), base), "{theme}");
            assert_eq!(backdrop.moves(), moves, "{theme}");
            if theme != "caskade.engage" {
                // Cobalt and Emerald hide Nexus's grid in their tokens.
                assert_eq!(grid(&backdrop).0.opacity, 0.0, "{theme}");
            }
        }
        // Engage's wash ends at its base surface, read from the tokens.
        let window = Window::open("caskade.engage", AppearanceSettings::default());
        let backdrop = window.backdrop();
        let BackdropLayerKind::Linear { stops, .. } = &backdrop.layers[0].kind else {
            panic!("a linear wash");
        };
        assert!(close(rgba(stops[1].color), hex("#eef0f3")));
    }

    #[test]
    fn rem_lengths_follow_the_ui_scale() {
        let prefs = AppearanceSettings {
            ui_scale: 150,
            ..Default::default()
        };
        let window = Window::open("caskade.default", prefs);
        let font = schema::parse_length(&window.computed(":root", "font-size")).unwrap();
        let mut prepared = Prepared {
            theme: "test".into(),
            composition: window.prepared.composition.clone(),
            images: Vec::new(),
        };
        let Kind::Grid { cell, .. } = &mut prepared.composition.layers[0].kind else {
            panic!("Nexus's grid comes first");
        };
        let two_rem = Param::Literal(Length {
            value: 2.0,
            unit: Unit::Rem,
        });
        *cell = [two_rem.clone(), two_rem];
        let backdrop = resolve(&window.document.inner.borrow(), &prepared);
        assert!(font.value > 16.0);
        assert_eq!(
            grid(&backdrop).1,
            Vec2::new(2.0, 2.0) * f64::from(font.value)
        );
    }

    /// The unlayered sheets beat the theme's and the user's layers.
    #[test]
    fn the_sheets_stop_the_document_painting_or_moving_its_backdrop() {
        let mut window = Window::open("caskade.default", AppearanceSettings::default());
        assert_ne!(
            window.computed("body", "background-color"),
            "rgba(0, 0, 0, 0)"
        );
        window.sheet(GPU_SHEET);
        assert_eq!(
            window.computed("body", "background-color"),
            "rgba(0, 0, 0, 0)"
        );
        assert_eq!(
            window.computed("[data-cs=\"backdrop\"]", "animation-name"),
            "none"
        );
        // What the user may still set is read, not overridden.
        assert_eq!(
            window.computed("[data-cs=\"backdrop\"]", "display"),
            "block"
        );
        window.sheet(STILL_SHEET);
        assert_eq!(
            window.computed("[data-cs=\"backdrop\"]", "animation-play-state"),
            "paused"
        );
        assert_ne!(
            window.computed("body", "background-color"),
            "rgba(0, 0, 0, 0)"
        );
    }

    #[test]
    fn a_theme_image_decodes_premultiplied() {
        let mut png = Vec::new();
        let pixels =
            image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 255, 255, 255, 128]).unwrap();
        pixels
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let image = decode("dev.example.sky", "assets/sky.png", &png).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(&image.rgba[..], &[255, 0, 0, 255, 128, 128, 128, 128]);
        assert!(decode("dev.example.sky", "assets/sky.png", b"not an image").is_none());
    }
}
