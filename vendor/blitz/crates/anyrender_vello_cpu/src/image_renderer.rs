use crate::{ImageCacheConfig, VelloCpuScenePainter};
use anyrender::{ImageRenderer, RenderContext as AnyRenderContext};
use debug_timer::debug_timer;
use kurbo::{Affine, Rect};
use vello_cpu::{PixmapMut, RenderContext};

pub struct VelloCpuImageRenderer {
    scene: VelloCpuScenePainter,
    /// The context a region renders into, kept while regions keep its size
    /// (an animation damages the same rectangle each frame).
    region_ctx: Option<RenderContext>,
    region_pixels: Vec<u8>,
}

impl VelloCpuImageRenderer {
    /// Create a renderer with a custom image cache configuration.
    pub fn with_image_cache_config(width: u32, height: u32, config: ImageCacheConfig) -> Self {
        Self {
            scene: VelloCpuScenePainter::with_image_cache_config(
                width as u16,
                height as u16,
                config,
            ),
            region_ctx: None,
            region_pixels: Vec::new(),
        }
    }

    /// Enable or disable caching of rasterized glyphs in Vello's glyph atlas.
    ///
    /// Defaults to `false`.
    ///
    /// Note: Vello considers atlas-backed glyph caching experimental.
    pub fn set_glyph_caching(&mut self, enabled: bool) {
        self.scene.set_glyph_caching(enabled);
    }

    /// Drop all cached image conversions.
    pub fn clear_image_cache(&mut self) {
        self.scene.clear_image_cache();
    }
}

impl AnyRenderContext for VelloCpuImageRenderer {}
impl ImageRenderer for VelloCpuImageRenderer {
    type ScenePainter<'a> = VelloCpuScenePainter;

    fn new(width: u32, height: u32) -> Self {
        Self {
            scene: VelloCpuScenePainter::new(width as u16, height as u16),
            region_ctx: None,
            region_pixels: Vec::new(),
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.scene.render_ctx = RenderContext::new(width as u16, height as u16);
    }

    fn reset(&mut self) {
        self.scene.render_ctx.reset();
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F, buffer: &mut [u8]) {
        debug_timer!(timer, feature = "log_frame_times");

        draw_fn(&mut self.scene);
        timer.record_time("cmds");

        self.scene.render_ctx.flush();
        timer.record_time("flush");

        self.scene.render_ctx.render(
            PixmapMut::new(
                self.scene.render_ctx.width(),
                self.scene.render_ctx.height(),
                buffer,
            )
            .unwrap(),
            &mut self.scene.resources,
        );
        timer.record_time("render");

        self.scene.maintain();
        timer.record_time("maintain");

        timer.print_times("vello_cpu: ");
    }

    fn supports_region(&self) -> bool {
        true
    }

    fn render_region<F: FnOnce(&mut Self::ScenePainter<'_>)>(
        &mut self,
        region: Rect,
        draw_fn: F,
        buffer: &mut [u8],
    ) {
        debug_timer!(timer, feature = "log_frame_times");

        let full_width = self.scene.render_ctx.width() as usize;
        let full_height = self.scene.render_ctx.height() as usize;
        let x0 = region.x0.floor().max(0.0) as usize;
        let y0 = region.y0.floor().max(0.0) as usize;
        let x1 = (region.x1.ceil().max(0.0) as usize).min(full_width);
        let y1 = (region.y1.ceil().max(0.0) as usize).min(full_height);
        if x1 <= x0 || y1 <= y0 || buffer.len() < full_width * full_height * 4 {
            return;
        }
        let (width, height) = (x1 - x0, y1 - y0);

        // Render the scene shifted so the region's origin is the context's.
        let region_ctx = match self.region_ctx.take() {
            Some(ctx) if ctx.width() as usize == width && ctx.height() as usize == height => ctx,
            _ => RenderContext::new(width as u16, height as u16),
        };
        let full_ctx = std::mem::replace(&mut self.scene.render_ctx, region_ctx);
        self.scene.offset = Affine::translate((-(x0 as f64), -(y0 as f64)));
        draw_fn(&mut self.scene);
        timer.record_time("cmds");

        self.scene.render_ctx.flush();
        self.region_pixels.clear();
        self.region_pixels.resize(width * height * 4, 0);
        self.scene.render_ctx.render(
            PixmapMut::new(width as u16, height as u16, &mut self.region_pixels).unwrap(),
            &mut self.scene.resources,
        );
        timer.record_time("render");

        let row = width * 4;
        for (y, pixels) in self.region_pixels.chunks_exact(row).enumerate() {
            let start = ((y0 + y) * full_width + x0) * 4;
            buffer[start..start + row].copy_from_slice(pixels);
        }
        timer.record_time("copy");

        self.scene.offset = Affine::IDENTITY;
        let mut region_ctx = std::mem::replace(&mut self.scene.render_ctx, full_ctx);
        region_ctx.reset();
        self.region_ctx = Some(region_ctx);
        self.scene.maintain();
        timer.print_times("vello_cpu (region): ");
    }

    fn render_to_vec<F: FnOnce(&mut Self::ScenePainter<'_>)>(
        &mut self,
        draw_fn: F,
        buffer: &mut Vec<u8>,
    ) {
        let width = self.scene.render_ctx.width();
        let height = self.scene.render_ctx.height();
        buffer.resize(width as usize * height as usize * 4, 0);
        self.render(draw_fn, buffer);
    }
}

#[cfg(test)]
mod region_tests {
    use anyrender::{ImageRenderer, PaintScene};
    use kurbo::{Affine, Circle, Rect};
    use peniko::{
        Color, Fill,
        color::palette::css::{BLUE, RED, WHITE},
    };

    use super::VelloCpuImageRenderer;
    use crate::VelloCpuScenePainter;

    const W: u32 = 64;
    const H: u32 = 48;

    fn scene(painter: &mut VelloCpuScenePainter, dot: Color) {
        painter.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            WHITE,
            None,
            &Rect::new(0.0, 0.0, W as f64, H as f64),
        );
        // Fractional edges, so anti-aliasing is part of what's compared.
        painter.fill(
            Fill::NonZero,
            Affine::translate((0.3, 0.6)),
            BLUE,
            None,
            &Rect::new(5.0, 5.0, 40.5, 30.25),
        );
        painter.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            dot,
            None,
            &Circle::new((30.5, 20.5), 9.3),
        );
    }

    fn full(dot: Color) -> Vec<u8> {
        let mut renderer = VelloCpuImageRenderer::new(W, H);
        let mut buffer = Vec::new();
        renderer.render_to_vec(|p| scene(p, dot), &mut buffer);
        buffer
    }

    #[test]
    fn a_region_matches_a_full_render_inside_and_keeps_the_rest() {
        let before = full(RED);
        let after = full(BLUE);
        let mut renderer = VelloCpuImageRenderer::new(W, H);
        let mut buffer = before.clone();
        let region = Rect::new(17.0, 9.0, 45.0, 33.0);
        assert!(renderer.supports_region());
        renderer.render_region(region, |p| scene(p, BLUE), &mut buffer);
        for y in 0..H as usize {
            for x in 0..W as usize {
                let i = (y * W as usize + x) * 4;
                let inside = (17..45).contains(&x) && (9..33).contains(&y);
                let expected = if inside { &after } else { &before };
                assert_eq!(
                    &buffer[i..i + 4],
                    &expected[i..i + 4],
                    "pixel ({x}, {y}), inside: {inside}"
                );
            }
        }
    }

    #[test]
    fn regions_of_another_size_and_full_frames_render_after_a_region() {
        let mut renderer = VelloCpuImageRenderer::new(W, H);
        let mut buffer = full(RED);
        renderer.render_region(
            Rect::new(0.0, 0.0, 8.0, 8.0),
            |p| scene(p, BLUE),
            &mut buffer,
        );
        renderer.reset();
        // Fractional bounds cover the pixels they touch; outside the image is cut.
        renderer.render_region(
            Rect::new(10.5, 10.5, 200.0, 200.0),
            |p| scene(p, BLUE),
            &mut buffer,
        );
        renderer.reset();
        let after = full(BLUE);
        let ok = |x: usize, y: usize| {
            let i = (y * W as usize + x) * 4;
            buffer[i..i + 4] == after[i..i + 4]
        };
        assert!(ok(30, 20) && ok(63, 47) && ok(10, 10) && ok(3, 3));
        let mut whole = Vec::new();
        renderer.render_to_vec(|p| scene(p, BLUE), &mut whole);
        assert_eq!(whole, after, "a full frame after regions is unchanged");
    }
}
