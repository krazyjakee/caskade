#![cfg(target_os = "macos")]
//! Offscreen Metal probe (#156): no app, window, display, tray or session bus.
//! Run explicitly with `cargo test -p caskade-native --test metal_memory -- --ignored --nocapture`.
use vello_gpu::{
    ClearSettings, RenderSettings, RenderSize, RenderTargetConfig, Renderer, Scene, TargetInit,
    TextureBindings,
};

fn footprint_mib() -> f64 {
    let output = std::process::Command::new("vmmap")
        .arg("-summary")
        .arg(std::process::id().to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix("Physical footprint:"))
        .unwrap()
        .trim();
    let (number, unit) = value.split_at(value.len() - 1);
    let scale = match unit {
        "K" => 1.0 / 1024.0,
        "M" => 1.0,
        "G" => 1024.0,
        _ => panic!("unknown footprint unit {value}"),
    };
    number.trim().parse::<f64>().unwrap() * scale
}

#[test]
#[ignore = "manual macOS GPU allocation probe, without a window"]
fn repeated_offscreen_metal_frames_keep_memory_bounded() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        flags: wgpu::InstanceFlags::from_build_config().with_env(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        display: None,
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .unwrap();
    println!("adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features()
            & (wgpu::Features::CLEAR_TEXTURE | wgpu::Features::PIPELINE_CACHE),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }))
    .unwrap();
    let size = RenderSize {
        width: 512,
        height: 512,
    };
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen memory probe"),
        size: wgpu::Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let depth = Renderer::create_depth_texture_view(&device, &size);
    let (mut renderer, mut resources) = Renderer::new_with(
        &device,
        &RenderTargetConfig {
            format,
            width: 512,
            height: 512,
        },
        RenderSettings::default(),
    );
    let mut scene = Scene::new(512, 512);
    let mut baseline = 0.0;
    // Match the long-lived outer pool around winit's AppKit run loop.
    objc2::rc::autoreleasepool(|_| {
        for frame in 0..512 {
            scene.reset();
            scene.set_paint(peniko::Color::from_rgb8(40, 50, 60));
            for i in 0..32 {
                let offset = f64::from((frame + i) % 200);
                scene.fill_rect(&peniko::kurbo::Rect::new(
                    offset,
                    f64::from(i * 10),
                    offset + 100.0,
                    f64::from(i * 10 + 8),
                ));
            }
            {
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer
                    .render(
                        &scene,
                        &mut resources,
                        &device,
                        &queue,
                        &mut encoder,
                        &size,
                        &view,
                        Some(&depth),
                        &TextureBindings::new(),
                        TargetInit::Clear(ClearSettings::default()),
                    )
                    .unwrap();
                queue.submit([encoder.finish()]);
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            }
            if matches!(frame, 31 | 127 | 511) {
                let footprint = footprint_mib();
                println!("frame {}: {footprint:.1} MiB", frame + 1);
                if frame == 127 {
                    baseline = footprint;
                }
                if frame == 511 {
                    assert!(
                        footprint <= baseline + 16.0,
                        "Metal allocations grew by {:.1} MiB after warm-up",
                        footprint - baseline
                    );
                }
            }
        }
    });
}
