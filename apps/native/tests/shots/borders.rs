//! Partial borders must not feed invalid geometry to either renderer (#144).

use anyrender::{Scene, recording::RenderCommand, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;

const SIZE: u32 = 400;

#[derive(Props, Clone, PartialEq)]
struct BorderProps {
    radius: &'static str,
    style: &'static str,
}

fn border_page(props: BorderProps) -> Element {
    rsx! {
        style { "body {{ margin: 0; background: white; }}" }
        for mask in 0..16 {
            div {
                style: border_style(&props, mask),
            }
        }
    }
}

fn border_style(props: &BorderProps, mask: u32) -> String {
    let x = 10 + (mask % 4) * 100;
    let y = 10 + (mask / 4) * 100;
    let widths = [2, 3, 4, 5].map(|width| {
        if mask & (1 << (width - 2)) != 0 {
            width
        } else {
            0
        }
    });
    format!(
        "position: absolute; left: {x}px; top: {y}px; width: 80px; height: 80px; \
         box-sizing: border-box; border-color: red; border-style: {}; \
         border-radius: {}; border-width: {}px {}px {}px {}px;",
        props.style, props.radius, widths[0], widths[1], widths[2], widths[3]
    )
}

fn document(radius: &'static str, style: &'static str) -> DioxusDocument {
    let mut document = DioxusDocument::new(
        VirtualDom::new_with_props(border_page, BorderProps { radius, style }),
        DocumentConfig {
            viewport: Some(Viewport::new(SIZE, SIZE, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    document.initial_build();
    document.inner.borrow_mut().resolve(0.0);
    document
}

#[test]
fn partial_borders_only_emit_finite_paths_for_both_renderers() {
    for radius in ["0", "12px", "18px / 10px"] {
        for style in [
            "solid", "double", "groove", "ridge", "inset", "outset", "dashed", "dotted",
        ] {
            let document = document(radius, style);
            let mut scene = Scene::new();
            blitz_paint::paint_scene(
                &mut scene,
                &mut document.inner.borrow_mut(),
                1.0,
                SIZE,
                SIZE,
                0,
                0,
            );
            // Inspect the commands before CPU/GPU backends can discard NaNs.
            for command in scene.commands {
                let shape = match command {
                    RenderCommand::Fill(command) => command.shape,
                    RenderCommand::Stroke(command) => command.shape,
                    RenderCommand::PushLayer(command) => command.clip,
                    RenderCommand::PushClipLayer(command) => command.clip,
                    _ => continue,
                };
                assert!(shape.is_finite(), "invalid {style} border, radius {radius}");
            }
        }
    }
}

#[test]
fn partial_borders_draw_every_nonzero_edge_and_leave_zero_edges_empty() {
    for radius in ["0", "12px", "18px / 10px"] {
        let document = document(radius, "solid");
        let pixels = render_to_buffer::<VelloCpuImageRenderer, _>(
            |scene| {
                blitz_paint::paint_scene(
                    scene,
                    &mut document.inner.borrow_mut(),
                    1.0,
                    SIZE,
                    SIZE,
                    0,
                    0,
                );
            },
            SIZE,
            SIZE,
        );
        for mask in 0..16 {
            let x = 10 + (mask % 4) * 100;
            let y = 10 + (mask / 4) * 100;
            for (side, (dx, dy)) in [(40, 1), (78, 40), (40, 78), (1, 40)]
                .into_iter()
                .enumerate()
            {
                let offset = (((y + dy) * SIZE + x + dx) * 4) as usize;
                let pixel = &pixels[offset..offset + 4];
                let red = pixel[0] > 180 && pixel[1] < 80 && pixel[2] < 80;
                assert_eq!(
                    red,
                    mask & (1 << side) != 0,
                    "mask {mask}, side {side}, radius {radius}: {pixel:?}"
                );
            }
        }
    }
}
