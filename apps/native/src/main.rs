//! Caskade's native app (#87): the main window rendered by Blitz, running
//! the same application layer as the Tauri app (`caskade-app`). It ships
//! beside the Tauri app until it reaches parity (BLITZ-33).

use dioxus_native::prelude::*;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let window = dioxus_native::WindowAttributes::default()
        .with_title("Caskade")
        .with_surface_size(dioxus_native::LogicalSize::new(1280.0, 800.0));
    dioxus_native::launch_cfg(app, vec![], vec![Box::new(window)]);
}

/// The main window's page; the views come with the UI work (#96 onwards).
fn app() -> Element {
    rsx! {
        div { style: "padding: 24px; font-family: sans-serif;", "Caskade" }
    }
}
