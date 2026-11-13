//! Draws an [`anyrender::Backdrop`] on the GPU beneath the cached scene
//! (`backdrop.wgsl`). A frame of the backdrop alone writes the layers'
//! motion offsets (a few dozen bytes) and records one or two passes: no
//! scene is built, encoded or rasterised.
//!
//! Without embedder shaders, one pass draws the layers and composes the
//! scene over them. With them (a theme's own WGSL), the layers are drawn
//! into a texture of their own only when the backdrop moves or changes, and
//! a document frame just composes it: a costly shader never runs per UI
//! frame. Their pass is timed on the GPU where the device can: a shader
//! over its budget stops moving (it stays at that frame), so an idle window
//! can't keep the GPU busy. A shader that doesn't compile isn't drawn.

use anyrender::backdrop::{
    BACKDROP_SHADER_INPUT, BACKDROP_SHADER_INPUTS, MAX_BACKDROP_IMAGES, MAX_BACKDROP_LAYERS,
    MAX_BACKDROP_SHADERS, MAX_BACKDROP_STOPS,
};
use anyrender::{
    Backdrop, BackdropBlend, BackdropImage, BackdropLayer, BackdropLayerKind, BackdropShader,
    BackdropStop,
};
use peniko::Color;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
    BufferDescriptor, BufferUsages, CommandEncoder, Device, Extent3d, Features, FilterMode,
    FragmentState, LoadOp, MapMode, MultisampleState, Operations, PipelineLayout,
    PipelineLayoutDescriptor, PrimitiveState, QuerySet, QuerySetDescriptor, QueryType, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPassTimestampWrites, RenderPipeline,
    RenderPipelineDescriptor, SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StoreOp, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
    TextureViewDimension, VertexState,
};

const HEADER_SIZE: usize = 128;
const LAYER_SIZE: usize = 160;
/// GPU time the embedder's shaders may take, per second of motion: 4% of
/// the GPU. Over it (or over [`FRAME_BUDGET`] a frame), they stop moving.
const SECOND_BUDGET: Duration = Duration::from_millis(40);
const FRAME_BUDGET: Duration = Duration::from_millis(8);
/// Frames timed before the budget is judged.
const SAMPLES: u32 = 8;
/// The share of a core the backdrop's own frames may use, presenting them
/// included: frames that cost more come further apart than the backdrop
/// asks. On a display that presents cheaply this never applies.
const CPU_SHARE: f64 = 0.01;
/// The longest the backdrop waits between frames for [`CPU_SHARE`].
const LONGEST_INTERVAL: Duration = Duration::from_secs(10);

/// [`CPU_SHARE`], or `ANYRENDER_BACKDROP_CPU_SHARE` (0–1) to test frames at
/// their own rate where presenting is slow (Xvfb).
fn cpu_share() -> f64 {
    static SHARE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *SHARE.get_or_init(|| {
        std::env::var("ANYRENDER_BACKDROP_CPU_SHARE")
            .ok()
            .and_then(|share| share.parse::<f64>().ok())
            .filter(|share| *share > 0.0 && *share <= 1.0)
            .unwrap_or(CPU_SHARE)
    })
}

/// [`SECOND_BUDGET`], or `ANYRENDER_BACKDROP_SHADER_BUDGET_MS` to test the
/// watch without a costly shader.
fn second_budget() -> Duration {
    static BUDGET: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| {
        std::env::var("ANYRENDER_BACKDROP_SHADER_BUDGET_MS")
            .ok()
            .and_then(|ms| ms.parse::<f64>().ok())
            .filter(|ms| ms.is_finite() && *ms >= 0.0)
            .map_or(SECOND_BUDGET, |ms| Duration::from_secs_f64(ms / 1000.0))
    })
}

/// The backdrop to draw and when its motion started.
pub(crate) struct BackdropState {
    pub backdrop: Backdrop,
    started: Instant,
    /// When a frame showing it was last presented, or `None` since it
    /// changed.
    pub last_frame: Option<Instant>,
    /// Where shader layers stopped after going over their budget.
    frozen_shaders: Option<Duration>,
    /// What a frame of the backdrop alone costs the process in CPU time,
    /// presenting it included (a moving average).
    frame_cost: Option<Duration>,
    /// The cost was logged as slowing the frames.
    slowed: bool,
}

impl BackdropState {
    pub fn new(backdrop: Backdrop, started: Option<Instant>) -> Self {
        Self {
            backdrop,
            started: started.unwrap_or_else(Instant::now),
            last_frame: None,
            frozen_shaders: None,
            frame_cost: None,
            slowed: false,
        }
    }

    /// Notes what a frame of the backdrop alone cost in CPU time.
    pub fn note_cost(&mut self, cost: Duration) {
        let average = match self.frame_cost {
            None => cost,
            Some(average) => average.mul_f64(0.8) + cost.mul_f64(0.2),
        };
        self.frame_cost = Some(average);
        let interval = self.interval();
        if !self.slowed
            && let Some(asked) = self.backdrop.frame_interval
            && interval > asked * 2
        {
            self.slowed = true;
            log::info!(
                "backdrop frames cost {:.1} ms of CPU each: one every {:.2} s instead of {:.2} s",
                average.as_secs_f64() * 1000.0,
                interval.as_secs_f64(),
                asked.as_secs_f64()
            );
        }
    }

    /// The time between the backdrop's frames: as asked, or longer when
    /// they'd use more than [`CPU_SHARE`] of a core.
    fn interval(&self) -> Duration {
        let asked = self.backdrop.frame_interval.unwrap_or(Duration::ZERO);
        let afforded = self
            .frame_cost
            .map_or(Duration::ZERO, |cost| cost.div_f64(cpu_share()));
        asked.max(afforded.min(LONGEST_INTERVAL))
    }

    pub fn started(&self) -> Instant {
        self.started
    }

    fn frozen(&self, layer: &BackdropLayer) -> bool {
        self.frozen_shaders.is_some() && matches!(layer.kind, BackdropLayerKind::Shader { .. })
    }

    /// Whether a layer that can be seen moves (and isn't stopped).
    pub fn moves(&self) -> bool {
        let backdrop = &self.backdrop;
        backdrop.frame_interval.is_some()
            && backdrop.opacity > 0.0
            && backdrop
                .layers
                .iter()
                .take(MAX_BACKDROP_LAYERS)
                .any(|layer| layer.motion.is_some() && layer.opacity > 0.0 && !self.frozen(layer))
    }

    /// See [`anyrender::WindowRenderer::backdrop_frame_due`].
    pub fn frame_due(&self) -> Option<Instant> {
        let Some(last) = self.last_frame else {
            return Some(Instant::now());
        };
        if !self.moves() {
            return None;
        }
        Some(last + self.interval())
    }

    /// How far into its motion `layer` is at `now`.
    fn elapsed(&self, layer: &BackdropLayer, now: Instant) -> Duration {
        if self.backdrop.frame_interval.is_none() {
            return Duration::ZERO;
        }
        match self.frozen_shaders {
            Some(at) if self.frozen(layer) => at,
            _ => now.saturating_duration_since(self.started),
        }
    }
}

/// The pipelines for one set of embedder shaders.
struct Pipelines {
    /// Their ids, in slot order.
    shaders: Vec<u64>,
    direct: RenderPipeline,
    layers: RenderPipeline,
    compose: RenderPipeline,
}

/// The backdrop's own texture while embedder shaders draw it.
struct Cache {
    size: (u32, u32),
    _texture: Texture,
    view: TextureView,
    /// It holds the backdrop as last written.
    valid: bool,
}

/// Times the layers pass on the GPU.
struct Watch {
    query: QuerySet,
    resolve: Buffer,
    readback: Buffer,
    /// 0 idle, 1 recorded (to map after submit), 2 mapping, 3 mapped.
    state: Arc<AtomicU8>,
    samples: u32,
    total: Duration,
}

/// The pipeline, buffers and images for drawing backdrops on one device.
pub(crate) struct BackdropGpu {
    format: TextureFormat,
    layout: BindGroupLayout,
    pipeline_layout: PipelineLayout,
    pipelines: Pipelines,
    /// Embedder shaders that didn't compile: never tried again.
    refused: Vec<u64>,
    header: Buffer,
    layers: Buffer,
    sampler: wgpu::Sampler,
    /// The images in their slots, by id; unused slots hold a 1×1 texture.
    images: Vec<(u64, Texture, TextureView)>,
    empty: TextureView,
    cache: Option<Cache>,
    watch: Option<Watch>,
    /// For the current scene texture, cache and images; rebuilt when any
    /// changes. The layers pass, which draws into the cache, has its own
    /// without it.
    bind_group: Option<BindGroup>,
    layers_bind_group: Option<BindGroup>,
    /// The layers last written, to skip writing them again.
    written: Option<Backdrop>,
}

/// `backdrop.wgsl` with `shaders` and the function that calls them.
fn source(shaders: &[&BackdropShader]) -> String {
    let mut calls = String::new();
    let mut functions = String::new();
    for (slot, shader) in shaders.iter().enumerate() {
        calls.push_str(&format!(
            "        case {slot}u: {{ return {}(input); }}\n",
            shader.entry
        ));
        functions.push_str(&shader.wgsl);
        functions.push('\n');
    }
    let theme = format!(
        "{BACKDROP_SHADER_INPUT}\nfn cs_theme_shader(index: u32, input: CsInput) -> vec4<f32> {{\n    switch index {{\n{calls}        default: {{ return vec4<f32>(0.0); }}\n    }}\n}}\n\n{functions}"
    );
    include_str!("backdrop.wgsl").replace("// CS_THEME_SHADERS\n", &theme)
}

fn pipelines(
    device: &Device,
    layout: &PipelineLayout,
    format: TextureFormat,
    shaders: &[&BackdropShader],
) -> Pipelines {
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("Backdrop"),
        source: ShaderSource::Wgsl(source(shaders).into()),
    });
    let pipeline = |entry: &str| {
        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Backdrop"),
            layout: Some(layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    Pipelines {
        shaders: shaders.iter().map(|shader| shader.id).collect(),
        direct: pipeline("fs_main"),
        layers: pipeline("fs_layers"),
        compose: pipeline("fs_compose"),
    }
}

impl BackdropGpu {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let uniform = |binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = |binding, filterable| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let mut entries = vec![
            uniform(0),
            uniform(1),
            texture(2, false),
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ];
        for slot in 0..MAX_BACKDROP_IMAGES as u32 {
            entries.push(texture(4 + slot, true));
        }
        entries.push(texture(8, false));
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Backdrop"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Backdrop"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = pipelines(device, &pipeline_layout, format, &[]);
        let buffer = |label, size| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Backdrop images"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let empty = image_texture(device, None, 1, 1).1;
        Self {
            format,
            layout,
            pipeline_layout,
            pipelines,
            refused: Vec::new(),
            header: buffer("Backdrop header", HEADER_SIZE),
            layers: buffer("Backdrop layers", LAYER_SIZE * MAX_BACKDROP_LAYERS),
            sampler,
            images: Vec::new(),
            empty,
            cache: None,
            watch: None,
            bind_group: None,
            layers_bind_group: None,
            written: None,
        }
    }

    pub fn format(&self) -> TextureFormat {
        self.format
    }

    /// The scene texture changed (a resize): bind the new one.
    pub fn scene_changed(&mut self) {
        self.unbind();
    }

    fn unbind(&mut self) {
        self.bind_group = None;
        self.layers_bind_group = None;
    }

    /// Compiles the embedder shaders `backdrop` uses, when they changed.
    /// One that doesn't compile is refused and the rest compiled again.
    fn compile(&mut self, device: &Device, backdrop: &Backdrop) {
        loop {
            let mut wanted: Vec<&BackdropShader> = Vec::new();
            for layer in backdrop.layers.iter().take(MAX_BACKDROP_LAYERS) {
                if let BackdropLayerKind::Shader { shader, .. } = &layer.kind
                    && !self.refused.contains(&shader.id)
                    && !wanted.iter().any(|known| known.id == shader.id)
                    && wanted.len() < MAX_BACKDROP_SHADERS
                {
                    wanted.push(shader);
                }
            }
            let ids: Vec<u64> = wanted.iter().map(|shader| shader.id).collect();
            if ids == self.pipelines.shaders {
                return;
            }
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let compiled = pipelines(device, &self.pipeline_layout, self.format, &wanted);
            #[cfg(not(target_arch = "wasm32"))]
            let error = pollster::block_on(scope.pop());
            // The web can't wait here: errors are wgpu's to report.
            #[cfg(target_arch = "wasm32")]
            let error = {
                drop(scope.pop());
                None::<wgpu::Error>
            };
            match error {
                None => {
                    self.pipelines = compiled;
                    return;
                }
                Some(error) => {
                    // Which one isn't known: refuse them all, the
                    // renderer's own layers still draw.
                    log::warn!("a backdrop shader didn't compile, so it isn't drawn: {error}");
                    self.refused.extend(ids);
                }
            }
        }
    }

    /// Reads the last timed layers pass, if it's back, and stops the
    /// embedder shaders' motion once they cost more than their budget.
    fn judge(&mut self, device: &Device, state: &mut BackdropState, now: Instant, period: f32) {
        let Some(watch) = &mut self.watch else {
            return;
        };
        if watch.state.load(Ordering::Acquire) == 2 {
            let _ = device.poll(wgpu::PollType::Poll);
        }
        if watch.state.load(Ordering::Acquire) != 3 {
            return;
        }
        let ticks = watch
            .readback
            .slice(..)
            .get_mapped_range()
            .ok()
            .and_then(|data| {
                let start = u64::from_le_bytes(data[0..8].try_into().ok()?);
                let end = u64::from_le_bytes(data[8..16].try_into().ok()?);
                end.checked_sub(start)
            });
        watch.readback.unmap();
        watch.state.store(0, Ordering::Release);
        let Some(ticks) = ticks else {
            return;
        };
        let spent = Duration::from_nanos((ticks as f64 * f64::from(period)) as u64);
        watch.samples += 1;
        watch.total += spent;
        if watch.samples < SAMPLES || state.frozen_shaders.is_some() {
            return;
        }
        let mean = watch.total / watch.samples;
        let per_second = state
            .backdrop
            .frame_interval
            .map_or(Duration::ZERO, |interval| {
                mean.mul_f64(1.0 / interval.as_secs_f64().max(1e-3))
            });
        watch.samples = 0;
        watch.total = Duration::ZERO;
        if mean > FRAME_BUDGET || per_second > second_budget() {
            let at = now.saturating_duration_since(state.started());
            log::warn!(
                "the backdrop's shaders take {:.2} ms a frame ({:.1} ms a second): they stop moving",
                mean.as_secs_f64() * 1000.0,
                per_second.as_secs_f64() * 1000.0
            );
            state.frozen_shaders = Some(at);
        }
    }

    /// After the frame `draw` recorded was submitted: reads its timing back.
    pub fn after_submit(&mut self) {
        let Some(watch) = &self.watch else {
            return;
        };
        if watch.state.load(Ordering::Acquire) != 1 {
            return;
        }
        watch.state.store(2, Ordering::Release);
        let state = Arc::clone(&watch.state);
        watch
            .readback
            .slice(..)
            .map_async(MapMode::Read, move |result| {
                state.store(if result.is_ok() { 3 } else { 0 }, Ordering::Release);
            });
    }

    /// Records the passes that draw `state`'s backdrop and `scene` over it
    /// into `target`, at `now`. `advance` moves embedder shaders' kept
    /// backdrop on to `now`; a document frame leaves it as it is.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        state: &mut BackdropState,
        now: Instant,
        scene: &TextureView,
        target: &TextureView,
        size: (u32, u32),
        unpremultiply: bool,
        advance: bool,
    ) {
        self.compile(device, &state.backdrop);
        let uses_cache = !self.pipelines.shaders.is_empty();
        if self.written.as_ref() != Some(&state.backdrop) {
            self.upload_images(device, queue, &state.backdrop);
            let slots: Vec<u64> = self.images.iter().map(|(id, ..)| *id).collect();
            queue.write_buffer(
                &self.layers,
                0,
                &layers_bytes(&state.backdrop, &slots, &self.pipelines.shaders),
            );
            self.written = Some(state.backdrop.clone());
            if let Some(cache) = &mut self.cache {
                cache.valid = false;
            }
        }
        if uses_cache {
            if self.cache.as_ref().is_none_or(|cache| cache.size != size) {
                let (texture, view) = render_texture(device, self.format, size);
                self.cache = Some(Cache {
                    size,
                    _texture: texture,
                    view,
                    valid: false,
                });
                self.unbind();
            }
            if self.watch.is_none() && device.features().contains(Features::TIMESTAMP_QUERY) {
                self.watch = Some(watch(device));
            }
        } else if self.cache.take().is_some() {
            self.unbind();
        }
        let period = queue.get_timestamp_period();
        self.judge(device, state, now, period);
        let draw_layers =
            uses_cache && (advance || self.cache.as_ref().is_some_and(|cache| !cache.valid));
        if !uses_cache || draw_layers {
            queue.write_buffer(
                &self.header,
                0,
                &header_bytes(state, now, size, unpremultiply),
            );
        }
        if self.bind_group.is_none() {
            self.bind_group = Some(self.bind(device, scene, true));
        }
        if uses_cache && self.layers_bind_group.is_none() {
            self.layers_bind_group = Some(self.bind(device, scene, false));
        }
        let (Some(bind_group), layers_bind_group) = (&self.bind_group, &self.layers_bind_group)
        else {
            return;
        };
        let pass = |encoder: &mut CommandEncoder,
                    view: &TextureView,
                    pipeline: &RenderPipeline,
                    bind_group: &BindGroup,
                    timestamps: Option<RenderPassTimestampWrites>| {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Backdrop"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: timestamps,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        };
        if !uses_cache {
            pass(encoder, target, &self.pipelines.direct, bind_group, None);
            return;
        }
        if draw_layers
            && let Some(cache) = &mut self.cache
            && let Some(layers_bind_group) = layers_bind_group
        {
            // Timed unless the last timing is still on its way back.
            let watch = self
                .watch
                .as_ref()
                .filter(|watch| watch.state.load(Ordering::Acquire) == 0);
            pass(
                encoder,
                &cache.view,
                &self.pipelines.layers,
                layers_bind_group,
                watch.map(|watch| RenderPassTimestampWrites {
                    query_set: &watch.query,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            );
            if let Some(watch) = watch {
                encoder.resolve_query_set(&watch.query, 0..2, &watch.resolve, 0);
                encoder.copy_buffer_to_buffer(&watch.resolve, 0, &watch.readback, 0, 16);
                watch.state.store(1, Ordering::Release);
            }
            cache.valid = true;
        }
        pass(encoder, target, &self.pipelines.compose, bind_group, None);
    }

    /// The bind group, with the backdrop's own texture bound when `cached`
    /// (it can't be while it is drawn into).
    fn bind(&self, device: &Device, scene: &TextureView, cached: bool) -> BindGroup {
        let mut entries = vec![
            BindGroupEntry {
                binding: 0,
                resource: self.header.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: self.layers.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(scene),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::Sampler(&self.sampler),
            },
        ];
        for slot in 0..MAX_BACKDROP_IMAGES {
            let view = self
                .images
                .get(slot)
                .map_or(&self.empty, |(_, _, view)| view);
            entries.push(BindGroupEntry {
                binding: 4 + slot as u32,
                resource: BindingResource::TextureView(view),
            });
        }
        entries.push(BindGroupEntry {
            binding: 8,
            resource: BindingResource::TextureView(
                self.cache
                    .as_ref()
                    .filter(|_| cached)
                    .map_or(&self.empty, |cache| &cache.view),
            ),
        });
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("Backdrop"),
            layout: &self.layout,
            entries: &entries,
        })
    }

    /// Uploads the images `backdrop` uses that aren't in a slot yet,
    /// dropping the ones it no longer uses.
    fn upload_images(&mut self, device: &Device, queue: &Queue, backdrop: &Backdrop) {
        let wanted = images(backdrop);
        let same = wanted.len() == self.images.len()
            && wanted
                .iter()
                .zip(&self.images)
                .all(|(image, (id, ..))| image.id == *id);
        if same {
            return;
        }
        let mut kept = std::mem::take(&mut self.images);
        for image in wanted {
            match kept.iter().position(|(id, ..)| *id == image.id) {
                Some(at) => self.images.push(kept.swap_remove(at)),
                None => {
                    let (texture, view) =
                        image_texture(device, Some((queue, image)), image.width, image.height);
                    self.images.push((image.id, texture, view));
                }
            }
        }
        self.unbind();
    }
}

fn watch(device: &Device) -> Watch {
    let buffer = |label, usage| {
        device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: 16,
            usage,
            mapped_at_creation: false,
        })
    };
    Watch {
        query: device.create_query_set(&QuerySetDescriptor {
            label: Some("Backdrop shaders' time"),
            ty: QueryType::Timestamp,
            count: 2,
        }),
        resolve: buffer(
            "Backdrop shaders' time",
            BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
        ),
        readback: buffer(
            "Backdrop shaders' time read back",
            BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        ),
        state: Arc::new(AtomicU8::new(0)),
        samples: 0,
        total: Duration::ZERO,
    }
}

/// A texture the backdrop is drawn into and read from.
fn render_texture(
    device: &Device,
    format: TextureFormat,
    (width, height): (u32, u32),
) -> (Texture, TextureView) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("Backdrop"),
        size: Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    (texture, view)
}

/// The distinct images a backdrop's layers use, in slot order.
fn images(backdrop: &Backdrop) -> Vec<&BackdropImage> {
    let mut out: Vec<&BackdropImage> = Vec::new();
    for layer in backdrop.layers.iter().take(MAX_BACKDROP_LAYERS) {
        if let BackdropLayerKind::Image { image, .. } = &layer.kind
            && !out.iter().any(|known| known.id == image.id)
            && out.len() < MAX_BACKDROP_IMAGES
            && image.rgba.len() == (image.width as usize) * (image.height as usize) * 4
            && image.width > 0
            && image.height > 0
        {
            out.push(image);
        }
    }
    out
}

fn image_texture(
    device: &Device,
    data: Option<(&Queue, &BackdropImage)>,
    width: u32,
    height: u32,
) -> (Texture, TextureView) {
    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("Backdrop image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        // Not sRGB: colours stay gamma-encoded, as the scene's are.
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let (queue, rgba): (Option<&Queue>, &[u8]) = match data {
        Some((queue, image)) => (Some(queue), &image.rgba),
        None => (None, &[]),
    };
    if let Some(queue) = queue {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
    }
    let view = texture.create_view(&TextureViewDescriptor::default());
    (texture, view)
}

/// A colour as the shader takes it: premultiplied, gamma-encoded sRGB.
fn premultiplied(color: Color) -> [f32; 4] {
    let [r, g, b, a] = color.components;
    [r * a, g * a, b * a, a]
}

fn put_f32(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn put_u32(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn header_bytes(
    state: &BackdropState,
    now: Instant,
    (width, height): (u32, u32),
    unpremultiply: bool,
) -> Vec<u8> {
    let backdrop = &state.backdrop;
    let mut out = Vec::with_capacity(HEADER_SIZE);
    let area = backdrop.area.intersect(kurbo::Rect::new(
        0.0,
        0.0,
        f64::from(width),
        f64::from(height),
    ));
    put_f32(
        &mut out,
        &[
            area.x0 as f32,
            area.y0 as f32,
            area.x1 as f32,
            area.y1 as f32,
        ],
    );
    put_f32(&mut out, &premultiplied(backdrop.base));
    let count = backdrop.layers.len().min(MAX_BACKDROP_LAYERS) as u32;
    put_u32(&mut out, &[count, u32::from(unpremultiply), 0, 0]);
    put_f32(&mut out, &[backdrop.opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0]);
    for slot in 0..MAX_BACKDROP_LAYERS {
        let Some(layer) = backdrop.layers.get(slot) else {
            put_f32(&mut out, &[0.0, 0.0]);
            continue;
        };
        let mut offset = layer.motion.map_or(kurbo::Vec2::ZERO, |motion| {
            motion.offset(state.elapsed(layer, now))
        });
        // Repeating content looks the same a tile on: wrapping there keeps
        // the offset small enough for f32 however long the window is open.
        let tile = match &layer.kind {
            BackdropLayerKind::Grid { cell, .. } => Some(*cell),
            BackdropLayerKind::Image {
                size, repeat: true, ..
            } => Some(*size),
            _ => None,
        };
        if let Some(tile) = tile.filter(|tile| tile.x >= 1.0 && tile.y >= 1.0) {
            offset = kurbo::Vec2::new(offset.x.rem_euclid(tile.x), offset.y.rem_euclid(tile.y));
        }
        put_f32(&mut out, &[offset.x as f32, offset.y as f32]);
    }
    debug_assert_eq!(out.len(), HEADER_SIZE);
    out
}

fn stops(out: &mut Vec<u8>, stops: &[BackdropStop]) {
    let stops = &stops[..stops.len().min(MAX_BACKDROP_STOPS)];
    let mut offsets = [0.0f32; 4];
    for (at, stop) in stops.iter().enumerate() {
        offsets[at] = stop.offset;
    }
    put_f32(out, &offsets);
    for at in 0..MAX_BACKDROP_STOPS {
        let color = stops.get(at).map_or(Color::TRANSPARENT, |stop| stop.color);
        put_f32(out, &premultiplied(color));
    }
}

fn layers_bytes(backdrop: &Backdrop, slots: &[u64], shaders: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(LAYER_SIZE * MAX_BACKDROP_LAYERS);
    let area = backdrop.area;
    for layer in backdrop.layers.iter().take(MAX_BACKDROP_LAYERS) {
        let blend = match layer.blend {
            BackdropBlend::Normal => 0,
            BackdropBlend::Multiply => 1,
            BackdropBlend::Screen => 2,
        };
        let start = out.len();
        let mut opacity = layer.opacity.clamp(0.0, 1.0);
        match &layer.kind {
            BackdropLayerKind::Linear { angle, stops: list } => {
                let (sin, cos) = (angle.to_radians().sin(), angle.to_radians().cos());
                let (w, h) = (area.width() as f32, area.height() as f32);
                let length = (w * sin).abs() + (h * cos).abs();
                let center = area.center();
                let count = list.len().min(MAX_BACKDROP_STOPS) as u32;
                put_u32(&mut out, &[0, blend, 0, count]);
                put_f32(&mut out, &[center.x as f32, center.y as f32, sin, -cos]);
                put_f32(&mut out, &[length.max(1e-3), 0.0, 0.0, 0.0]);
                mask(&mut out, layer, opacity);
                stops(&mut out, list);
            }
            BackdropLayerKind::Radial {
                center,
                radius,
                stops: list,
            } => {
                let count = list.len().min(MAX_BACKDROP_STOPS) as u32;
                put_u32(&mut out, &[1, blend, 0, count]);
                put_f32(
                    &mut out,
                    &[
                        center.x as f32,
                        center.y as f32,
                        radius.x as f32,
                        radius.y as f32,
                    ],
                );
                put_f32(&mut out, &[0.0; 4]);
                mask(&mut out, layer, opacity);
                stops(&mut out, list);
            }
            BackdropLayerKind::Grid {
                color,
                cell,
                line,
                origin,
            } => {
                if cell.x < 1.0 || cell.y < 1.0 {
                    opacity = 0.0;
                }
                put_u32(&mut out, &[2, blend, 0, 0]);
                put_f32(
                    &mut out,
                    &[cell.x.max(1.0) as f32, cell.y.max(1.0) as f32, *line, 0.0],
                );
                put_f32(&mut out, &[origin.x as f32, origin.y as f32, 0.0, 0.0]);
                mask(&mut out, layer, opacity);
                stops(
                    &mut out,
                    &[BackdropStop {
                        offset: 0.0,
                        color: *color,
                    }],
                );
            }
            BackdropLayerKind::Shader {
                shader,
                colors,
                numbers,
                scale,
            } => {
                let slot = shaders.iter().position(|id| *id == shader.id);
                if slot.is_none() {
                    opacity = 0.0;
                }
                put_u32(&mut out, &[4, blend, slot.unwrap_or(0) as u32, 0]);
                put_f32(
                    &mut out,
                    &[
                        area.x0 as f32,
                        area.y0 as f32,
                        area.width() as f32,
                        area.height() as f32,
                    ],
                );
                put_f32(&mut out, &[*scale, 0.0, 0.0, 0.0]);
                mask(&mut out, layer, opacity);
                put_f32(&mut out, numbers);
                for at in 0..BACKDROP_SHADER_INPUTS {
                    // Straight: the shader's own colours, as given.
                    let color = colors.get(at).map_or(Color::TRANSPARENT, |color| *color);
                    put_f32(&mut out, &color.components);
                }
            }
            BackdropLayerKind::Image {
                image,
                origin,
                size,
                repeat,
            } => {
                let slot = slots.iter().position(|id| *id == image.id);
                if slot.is_none() {
                    opacity = 0.0;
                }
                put_u32(&mut out, &[3, blend, slot.unwrap_or(0) as u32, 0]);
                put_f32(
                    &mut out,
                    &[
                        origin.x as f32,
                        origin.y as f32,
                        size.x as f32,
                        size.y as f32,
                    ],
                );
                put_f32(&mut out, &[if *repeat { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0]);
                mask(&mut out, layer, opacity);
                stops(&mut out, &[]);
            }
        }
        debug_assert_eq!(out.len() - start, LAYER_SIZE);
    }
    out.resize(LAYER_SIZE * MAX_BACKDROP_LAYERS, 0);
    out
}

fn mask(out: &mut Vec<u8>, layer: &anyrender::BackdropLayer, opacity: f32) {
    match layer.mask {
        Some(mask) => {
            put_f32(
                out,
                &[
                    mask.center.x as f32,
                    mask.center.y as f32,
                    mask.radius.x as f32,
                    mask.radius.y as f32,
                ],
            );
            put_f32(out, &[mask.from, mask.to, 1.0, opacity]);
        }
        None => {
            put_f32(out, &[0.0; 4]);
            put_f32(out, &[0.0, 0.0, 0.0, opacity]);
        }
    }
}
