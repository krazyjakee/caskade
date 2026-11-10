//! The `gpu-cpu-fallback` renderer: vello_hybrid on a GPU, and vello_cpu
//! where there's no usable one (no adapter, no device or surface, or only a
//! software adapter such as llvmpipe, lavapipe or WARP, which rasterise in
//! software through a GPU API and present the whole window every frame).
//! vello_cpu repaints only damage, so an animation costs its own rectangle.

use std::any::Any;
use std::sync::Arc;

use anyrender::{DynScenePainter, RenderContext, RendererConfig, WindowHandle, WindowRenderer};
use anyrender_vello_cpu::VelloCpuWindowRenderer;
use anyrender_vello_hybrid::{VelloHybridRendererOptions, VelloHybridWindowRenderer};

#[allow(clippy::large_enum_variant)]
enum Active {
    Gpu(VelloHybridWindowRenderer),
    Cpu(VelloCpuWindowRenderer),
}

pub struct FallbackWindowRenderer {
    active: Active,
    /// The CPU renderer's settings (the base colour).
    cpu_config: RendererConfig,
    /// The window and size of the last resume, to start the CPU renderer on.
    window: Option<(Arc<dyn WindowHandle>, u32, u32)>,
    /// Why the GPU renderer wasn't used, once it wasn't.
    fallback_reason: Option<String>,
}

impl FallbackWindowRenderer {
    /// Tries vello_hybrid with `options` first (refusing software adapters),
    /// or goes straight to vello_cpu when `cpu_only`.
    pub fn new(options: VelloHybridRendererOptions, cpu_only: bool) -> Self {
        let mut cpu_config = RendererConfig::default();
        cpu_config.base_color = Some(options.base_color);
        let active = if cpu_only {
            Active::Cpu(VelloCpuWindowRenderer::with_options(cpu_config.clone()))
        } else {
            Active::Gpu(VelloHybridWindowRenderer::with_options(
                options.allow_software_adapter(false),
            ))
        };
        Self {
            active,
            cpu_config,
            window: None,
            fallback_reason: cpu_only.then(|| "asked for".to_string()),
        }
    }

    /// Whether the window is drawn on the CPU.
    pub fn is_cpu(&self) -> bool {
        matches!(self.active, Active::Cpu(_))
    }

    /// Why the window is drawn on the CPU, when it is.
    pub fn fallback_reason(&self) -> Option<&str> {
        self.fallback_reason.as_deref()
    }

    /// Switches to vello_cpu for good, on the window the GPU renderer had.
    fn fall_back(&mut self, why: String) {
        #[cfg(feature = "tracing")]
        tracing::warn!("no usable GPU ({why}): drawing on the CPU");
        self.fallback_reason = Some(why);
        let mut cpu = VelloCpuWindowRenderer::with_options(self.cpu_config.clone());
        if let Some((window, width, height)) = self.window.clone() {
            cpu.resume(window, width, height, || {});
        }
        self.active = Active::Cpu(cpu);
    }
}

impl RenderContext for FallbackWindowRenderer {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn Any>,
    ) -> Result<anyrender::ResourceId, anyrender::RegisterResourceError> {
        match &mut self.active {
            Active::Gpu(r) => r.try_register_custom_resource(resource),
            Active::Cpu(r) => r.try_register_custom_resource(resource),
        }
    }

    fn unregister_resource(&mut self, resource_id: anyrender::ResourceId) {
        match &mut self.active {
            Active::Gpu(r) => r.unregister_resource(resource_id),
            Active::Cpu(r) => r.unregister_resource(resource_id),
        }
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn Any>> {
        match &self.active {
            Active::Gpu(r) => r.renderer_specific_context(),
            Active::Cpu(r) => r.renderer_specific_context(),
        }
    }
}

impl WindowRenderer for FallbackWindowRenderer {
    type ScenePainter<'a>
        = DynScenePainter<'a>
    where
        Self: 'a;

    fn resume<F: FnOnce() + 'static>(
        &mut self,
        window: Arc<dyn WindowHandle>,
        width: u32,
        height: u32,
        on_ready: F,
    ) {
        self.window = Some((window.clone(), width, height));
        match &mut self.active {
            Active::Gpu(r) => r.resume(window, width, height, on_ready),
            Active::Cpu(r) => r.resume(window, width, height, on_ready),
        }
    }

    fn complete_resume(&mut self) -> bool {
        match &mut self.active {
            Active::Gpu(r) => {
                if r.complete_resume() {
                    return true;
                }
                let Some(why) = r.failure().map(str::to_string) else {
                    return false;
                };
                self.fall_back(why);
                self.complete_resume()
            }
            Active::Cpu(r) => r.complete_resume(),
        }
    }

    fn suspend(&mut self) {
        match &mut self.active {
            Active::Gpu(r) => r.suspend(),
            Active::Cpu(r) => r.suspend(),
        }
    }

    fn is_active(&self) -> bool {
        match &self.active {
            Active::Gpu(r) => r.is_active(),
            Active::Cpu(r) => r.is_active(),
        }
    }

    fn is_pending(&self) -> bool {
        match &self.active {
            Active::Gpu(r) => r.is_pending(),
            Active::Cpu(r) => r.is_pending(),
        }
    }

    fn set_size(&mut self, width: u32, height: u32) {
        if let Some((_, w, h)) = &mut self.window {
            (*w, *h) = (width, height);
        }
        match &mut self.active {
            Active::Gpu(r) => r.set_size(width, height),
            Active::Cpu(r) => r.set_size(width, height),
        }
    }

    fn render<F: FnOnce(&mut Self::ScenePainter<'_>)>(&mut self, draw_fn: F) {
        match &mut self.active {
            Active::Gpu(r) => r.render(|p| draw_fn(&mut DynScenePainter::new(p))),
            Active::Cpu(r) => r.render(|p| draw_fn(&mut DynScenePainter::new(p))),
        }
    }

    fn supports_damage(&self) -> bool {
        match &self.active {
            Active::Gpu(r) => r.supports_damage(),
            Active::Cpu(r) => r.supports_damage(),
        }
    }

    fn render_damage<F: FnOnce(&mut Self::ScenePainter<'_>)>(
        &mut self,
        damage: peniko::kurbo::Rect,
        draw_fn: F,
    ) {
        match &mut self.active {
            Active::Gpu(r) => r.render_damage(damage, |p| draw_fn(&mut DynScenePainter::new(p))),
            Active::Cpu(r) => r.render_damage(damage, |p| draw_fn(&mut DynScenePainter::new(p))),
        }
    }
}
