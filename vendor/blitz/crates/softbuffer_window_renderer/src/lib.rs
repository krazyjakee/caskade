//! An AnyRender WindowRenderer for rendering pixel buffers using the softbuffer crate

#![cfg_attr(docsrs, feature(doc_cfg))]

use anyrender::{ImageRenderer, PaintScene, RenderContext, WindowHandle, WindowRenderer};
use debug_timer::debug_timer;
use kurbo::{Affine, Rect};
use softbuffer::{Context, Surface};
use std::{num::NonZero, sync::Arc};

/// Configuration options for the Softbuffer renderer.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SoftbufferRendererOptions {
    /// Background color used to clear the frame.
    pub base_color: peniko::Color,
}

impl Default for SoftbufferRendererOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl SoftbufferRendererOptions {
    pub const fn new() -> Self {
        Self {
            base_color: peniko::Color::WHITE,
        }
    }

    pub const fn base_color(self, base_color: peniko::Color) -> Self {
        Self { base_color, ..self }
    }
}

impl From<anyrender::RendererConfig> for SoftbufferRendererOptions {
    fn from(config: anyrender::RendererConfig) -> Self {
        let mut options = Self::default();
        if let Some(color) = config.base_color {
            options.base_color = color;
        }
        // `composite_alpha_mode` is intentionally ignored: softbuffer does not
        // support transparency, so any requested alpha mode degrades to opaque.
        options
    }
}

// Simple struct to hold the state of the renderer
pub struct ActiveRenderState {
    _context: Context<Arc<dyn WindowHandle>>,
    surface: Surface<Arc<dyn WindowHandle>, Arc<dyn WindowHandle>>,
}

#[allow(clippy::large_enum_variant)]
pub enum RenderState {
    Active(ActiveRenderState),
    Suspended,
}

pub struct SoftbufferWindowRenderer<Renderer: ImageRenderer> {
    // The fields MUST be in this order, so that the surface is dropped before the window
    // Window is cached even when suspended so that it can be reused when the app is resumed after being suspended
    render_state: RenderState,
    window_handle: Option<Arc<dyn WindowHandle>>,
    renderer: Renderer,
    buffer: Vec<u8>,
    config: SoftbufferRendererOptions,
    width: u32,
    height: u32,
    /// Whether `buffer` holds a complete frame at the current size, which a
    /// damaged frame repaints part of.
    has_frame: bool,
}

impl<Renderer: ImageRenderer> SoftbufferWindowRenderer<Renderer> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::with_renderer(Renderer::new(0, 0))
    }

    pub fn with_renderer<R: ImageRenderer>(renderer: R) -> SoftbufferWindowRenderer<R> {
        SoftbufferWindowRenderer {
            render_state: RenderState::Suspended,
            window_handle: None,
            renderer,
            buffer: Vec::new(),
            config: SoftbufferRendererOptions::default(),
            width: 0,
            height: 0,
            has_frame: false,
        }
    }

    pub fn with_options(config: impl Into<SoftbufferRendererOptions>) -> Self {
        Self {
            render_state: RenderState::Suspended,
            window_handle: None,
            renderer: Renderer::new(0, 0),
            buffer: Vec::new(),
            config: config.into(),
            width: 0,
            height: 0,
            has_frame: false,
        }
    }

    pub fn try_with_options<E: std::error::Error>(
        config: impl TryInto<SoftbufferRendererOptions, Error = E>,
    ) -> Result<Self, E> {
        Ok(Self {
            render_state: RenderState::Suspended,
            window_handle: None,
            renderer: Renderer::new(0, 0),
            buffer: Vec::new(),
            config: config.try_into()?,
            width: 0,
            height: 0,
            has_frame: false,
        })
    }

    pub fn with_options_and_renderer<R: ImageRenderer>(
        renderer: R,
        config: impl TryInto<SoftbufferRendererOptions, Error = impl std::error::Error>,
    ) -> SoftbufferWindowRenderer<R> {
        SoftbufferWindowRenderer {
            render_state: RenderState::Suspended,
            window_handle: None,
            renderer,
            buffer: Vec::new(),
            config: config
                .try_into()
                .expect("Invalid Softbuffer renderer configuration"),
            width: 0,
            height: 0,
            has_frame: false,
        }
    }
}

impl<Renderer: ImageRenderer> SoftbufferWindowRenderer<Renderer> {
    /// Renders and presents a whole frame.
    fn render_full<F: FnOnce(&mut Renderer::ScenePainter<'_>)>(&mut self, draw_fn: F) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };

        debug_timer!(timer, feature = "log_frame_times");

        let Ok(mut surface_buffer) = state.surface.buffer_mut() else {
            return;
        };
        timer.record_time("buffer_mut");

        // Paint
        let base_color = self.config.base_color;
        let width = self.width as f64;
        let height = self.height as f64;

        let wrapped_draw_fn = |painter: &mut Renderer::ScenePainter<'_>| {
            painter.fill(
                peniko::Fill::NonZero,
                Affine::IDENTITY,
                base_color,
                None,
                &Rect::new(0.0, 0.0, width, height),
            );
            draw_fn(painter);
        };

        self.renderer
            .render_to_vec(wrapped_draw_fn, &mut self.buffer);
        timer.record_time("render");

        let out = surface_buffer.as_mut();
        assert_eq!(self.buffer.len(), out.len() * 4);
        swizzle(&self.buffer, out);
        timer.record_time("swizel");

        surface_buffer.present().unwrap();
        timer.record_time("present");
        timer.print_times("softbuffer: ");
        self.has_frame = true;

        // Reset the renderer ready for the next render
        self.renderer.reset();
    }
}

impl<Renderer: ImageRenderer> RenderContext for SoftbufferWindowRenderer<Renderer> {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn std::any::Any>,
    ) -> Result<anyrender::ResourceId, anyrender::RegisterResourceError> {
        self.renderer.try_register_custom_resource(resource)
    }

    fn unregister_resource(&mut self, resource_id: anyrender::ResourceId) {
        self.renderer.unregister_resource(resource_id);
    }
}
impl<Renderer: ImageRenderer> WindowRenderer for SoftbufferWindowRenderer<Renderer> {
    type ScenePainter<'a>
        = Renderer::ScenePainter<'a>
    where
        Self: 'a;

    fn is_active(&self) -> bool {
        matches!(self.render_state, RenderState::Active(_))
    }

    fn resume<F: FnOnce() + 'static>(
        &mut self,
        window_handle: Arc<dyn WindowHandle>,
        width: u32,
        height: u32,
        on_ready: F,
    ) {
        let context = Context::new(window_handle.clone()).unwrap();
        let surface = Surface::new(&context, window_handle.clone()).unwrap();
        self.render_state = RenderState::Active(ActiveRenderState {
            _context: context,
            surface,
        });
        self.window_handle = Some(window_handle);
        self.has_frame = false;

        self.set_size(width, height);
        on_ready();
    }

    fn complete_resume(&mut self) -> bool {
        true
    }

    fn suspend(&mut self) {
        self.render_state = RenderState::Suspended;
        self.has_frame = false;
    }

    fn set_size(&mut self, physical_width: u32, physical_height: u32) {
        self.width = physical_width;
        self.height = physical_height;
        self.has_frame = false;
        if let RenderState::Active(state) = &mut self.render_state {
            state
                .surface
                .resize(
                    NonZero::new(physical_width.max(1)).unwrap(),
                    NonZero::new(physical_height.max(1)).unwrap(),
                )
                .unwrap();
            self.renderer.resize(physical_width, physical_height);
        };
    }

    fn render<F: FnOnce(&mut Renderer::ScenePainter<'_>)>(&mut self, draw_fn: F) {
        self.render_full(draw_fn);
    }

    fn supports_damage(&self) -> bool {
        self.has_frame
            && self.is_active()
            && self.renderer.supports_region()
            && self.buffer.len() == self.width as usize * self.height as usize * 4
    }

    /// Renders only `damage` into the kept frame, and copies and presents
    /// only that rectangle when the surface's buffer holds the last frame.
    fn render_damage<F: FnOnce(&mut Renderer::ScenePainter<'_>)>(
        &mut self,
        damage: Rect,
        draw_fn: F,
    ) {
        if !self.supports_damage() {
            return self.render_full(draw_fn);
        }
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };
        let x0 = (damage.x0.floor().max(0.0) as u32).min(self.width);
        let y0 = (damage.y0.floor().max(0.0) as u32).min(self.height);
        let x1 = (damage.x1.ceil().max(0.0) as u32).min(self.width);
        let y1 = (damage.y1.ceil().max(0.0) as u32).min(self.height);
        let (Some(region_width), Some(region_height)) = (
            NonZero::new(x1.saturating_sub(x0)),
            NonZero::new(y1.saturating_sub(y0)),
        ) else {
            return;
        };

        debug_timer!(timer, feature = "log_frame_times");

        let Ok(mut surface_buffer) = state.surface.buffer_mut() else {
            return;
        };
        timer.record_time("buffer_mut");

        let base_color = self.config.base_color;
        let region = Rect::new(x0 as f64, y0 as f64, x1 as f64, y1 as f64);
        let wrapped_draw_fn = |painter: &mut Renderer::ScenePainter<'_>| {
            painter.fill(
                peniko::Fill::NonZero,
                Affine::IDENTITY,
                base_color,
                None,
                &region,
            );
            draw_fn(painter);
        };
        self.renderer
            .render_region(region, wrapped_draw_fn, &mut self.buffer);
        timer.record_time("render");

        let age = surface_buffer.age();
        let out = surface_buffer.as_mut();
        if age == 1 {
            // The buffer holds the last frame: only the region changed.
            let width = self.width as usize;
            let (x0, x1) = (x0 as usize, x1 as usize);
            for y in y0 as usize..y1 as usize {
                let row = y * width;
                swizzle(
                    &self.buffer[(row + x0) * 4..(row + x1) * 4],
                    &mut out[row + x0..row + x1],
                );
            }
        } else {
            swizzle(&self.buffer, out);
        }
        timer.record_time("swizel");

        surface_buffer
            .present_with_damage(&[softbuffer::Rect {
                x: x0,
                y: y0,
                width: region_width,
                height: region_height,
            }])
            .unwrap();
        timer.record_time("present");
        timer.print_times("softbuffer (damage): ");

        self.renderer.reset();
    }
}

/// RGBA8 pixels to softbuffer's 0RGB.
fn swizzle(src: &[u8], out: &mut [u32]) {
    let (chunks, remainder) = src.as_chunks::<4>();
    debug_assert_eq!(chunks.len(), out.len());
    debug_assert_eq!(remainder.len(), 0);
    for (&[r, g, b, _a], dest) in chunks.iter().zip(out.iter_mut()) {
        *dest = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    }
}
