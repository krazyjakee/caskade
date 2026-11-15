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
    // Exercise the same adapter policy as the app. WGPU_POWER_PREF=high
    // reproduces AMD driver growth on the affected dual-GPU Mac (#156).
    let handle = pollster::block_on(wgpu_context::DeviceHandle::new_from_compatible_surface(
        instance,
        None,
        Some(wgpu::Features::CLEAR_TEXTURE | wgpu::Features::PIPELINE_CACHE),
        None,
    ))
    .unwrap();
    println!("adapter: {:?}", handle.adapter.get_info());
    let device = &handle.device;
    let queue = &handle.queue;
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
    let depth = Renderer::create_depth_texture_view(device, &size);
    let (mut renderer, mut resources) = Renderer::new_with(
        device,
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
        for frame in 0..4096 {
            scene.reset();
            scene.set_paint(peniko::Color::from_rgb8(40, 50, 60));
            for i in 0..32 {
                let offset = f64::from((frame + i) % 200);
                // Layer composition exercises multiple Metal render passes;
                // solid rectangles alone did not reproduce the growth.
                scene.push_opacity_layer(0.9);
                scene.fill_rect(&peniko::kurbo::Rect::new(
                    offset,
                    f64::from(i * 10),
                    offset + 100.0,
                    f64::from(i * 10 + 8),
                ));
                scene.pop_layer();
            }
            {
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer
                    .render(
                        &scene,
                        &mut resources,
                        device,
                        queue,
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
            if matches!(frame, 31 | 127 | 1023 | 4095) {
                let footprint = footprint_mib();
                println!("frame {}: {footprint:.1} MiB", frame + 1);
                if frame == 127 {
                    baseline = footprint;
                }
                if frame == 4095 {
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
