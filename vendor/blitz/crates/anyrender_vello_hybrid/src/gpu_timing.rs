//! Opt-in GPU command timestamps for native measurement builds only.
//! The interval covers the scene command buffer's uploads and rendering;
//! the later surface blit/present and CPU work are outside it.
use std::sync::{OnceLock, mpsc};
use wgpu::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoder, Device, Features, MapMode, QuerySet,
    QuerySetDescriptor, QueryType,
};

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("ANYRENDER_GPU_TIMINGS").as_deref() == Ok("1"))
}

pub(crate) struct GpuTimer {
    query: QuerySet,
    resolved: Buffer,
    readback: Buffer,
}
impl GpuTimer {
    pub(crate) fn begin(device: &Device, encoder: &mut CommandEncoder) -> Option<Self> {
        if !enabled() {
            return None;
        }
        let features = Features::TIMESTAMP_QUERY | Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if !device.features().contains(features) {
            eprintln!("gpu_frame: timestamps unavailable");
            return None;
        }
        let query = device.create_query_set(&QuerySetDescriptor {
            label: Some("Frame timing"),
            ty: QueryType::Timestamp,
            count: 2,
        });
        let resolved = device.create_buffer(&BufferDescriptor {
            label: Some("Frame timing resolve"),
            size: 16,
            usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&BufferDescriptor {
            label: Some("Frame timing readback"),
            size: 16,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.write_timestamp(&query, 0);
        Some(Self {
            query,
            resolved,
            readback,
        })
    }
    pub(crate) fn end(&self, encoder: &mut CommandEncoder) {
        encoder.write_timestamp(&self.query, 1);
        encoder.resolve_query_set(&self.query, 0..2, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.readback, 0, 16);
    }
    pub(crate) fn map(self) -> (Self, mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>) {
        let (send, ready) = mpsc::channel();
        self.readback
            .slice(..)
            .map_async(MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        (self, ready)
    }
    pub(crate) fn print(
        self,
        ready: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
        period: f32,
    ) {
        self.print_as("gpu_frame", ready, period)
    }

    /// [`Self::print`] under another label: `gpu_backdrop` for the
    /// backdrop's compose pass.
    pub(crate) fn print_as(
        self,
        label: &str,
        ready: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
        period: f32,
    ) {
        if !matches!(ready.try_recv(), Ok(Ok(()))) {
            eprintln!("{label}: timestamp readback unavailable");
            return;
        }
        let Ok(data) = self.readback.slice(..).get_mapped_range() else {
            eprintln!("{label}: mapped timestamp range unavailable");
            return;
        };
        let start = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let end = u64::from_le_bytes(data[8..16].try_into().unwrap());
        if let Some(ticks) = end.checked_sub(start) {
            eprintln!(
                "{label}: {:.6}ms",
                ticks as f64 * f64::from(period) / 1_000_000.0
            );
        } else {
            eprintln!("{label}: timestamp order invalid");
        }
        drop(data);
        self.readback.unmap();
    }
}
