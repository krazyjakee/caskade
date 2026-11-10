//! Drawing an SVG image: as vectors, or rasterised (once per size, cached)
//! when it uses filters, which vector rendering can't draw.

use std::sync::{Arc, Mutex};

use anyrender::PaintScene;
use blitz_dom::node::SvgImageData;
use kurbo::{Affine, Rect};
use peniko::{
    Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat, ImageQuality, ImageSampler,
};

use crate::render::svg_viewport_affine;

/// Rasterised filtered SVGs by (source hash, width, height): a tiled
/// background asks for the same size every frame.
type RasterCache = Vec<((u64, u32, u32), ImageData)>;
static RASTERS: Mutex<RasterCache> = Mutex::new(Vec::new());
/// How many sizes are kept, the most recent first.
const CACHED: usize = 16;

/// Draws `svg` into the `width`×`height` device-pixel box that `place`
/// positions (its origin at the box's top left).
pub(crate) fn draw_svg(
    scene: &mut impl PaintScene,
    svg: &SvgImageData,
    place: Affine,
    width: f32,
    height: f32,
) {
    let viewport = svg_viewport_affine(svg, width, height);
    if !svg.has_filters {
        anyrender_svg::render_svg_tree(scene, &svg.tree, place * viewport);
        return;
    }
    let (w, h) = (width.ceil() as u32, height.ceil() as u32);
    let Some(image) = rasterise(svg, viewport, w, h) else {
        return;
    };
    let brush = ImageBrush {
        image,
        sampler: ImageSampler {
            x_extend: peniko::Extend::Pad,
            y_extend: peniko::Extend::Pad,
            quality: ImageQuality::Medium,
            alpha: 1.0,
        },
    };
    scene.fill(
        peniko::Fill::NonZero,
        place,
        brush.as_ref(),
        None,
        &Rect::new(0.0, 0.0, w as f64, h as f64),
    );
}

fn rasterise(svg: &SvgImageData, viewport: Affine, width: u32, height: u32) -> Option<ImageData> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return None;
    }
    let key = (svg.source_hash, width, height);
    let mut cache = RASTERS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = cache.iter().position(|(known, _)| *known == key) {
        let entry = cache.remove(at);
        let image = entry.1.clone();
        cache.insert(0, entry);
        return Some(image);
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    let [a, b, c, d, e, f] = viewport.as_coeffs().map(|v| v as f32);
    resvg::render(
        &svg.tree,
        resvg::tiny_skia::Transform::from_row(a, b, c, d, e, f),
        &mut pixmap.as_mut(),
    );
    let image = ImageData {
        data: Blob::new(Arc::new(pixmap.take())),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::AlphaPremultiplied,
        width,
        height,
    };
    cache.insert(0, (key, image.clone()));
    cache.truncate(CACHED);
    Some(image)
}
