//! A search field that prints what it hears (#84), for input-method and
//! clipboard checks on a private desktop: `scripts/ime-ibus.py` types into
//! it through IBus and reads stdout.
//!
//! `cargo run -p caskade-native --example text_field_demo`

use dioxus_native::prelude::*;
use std::io::Write;

fn say(line: String) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "field: {line}");
    let _ = out.flush();
}

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    let mut query = use_signal(String::new);
    rsx! {
        title { "Caskade text field demo" }
        input {
            id: "search",
            r#type: "search",
            autofocus: true,
            style: "position: absolute; left: 20px; top: 20px; width: 400px; height: 32px; font-size: 20px",
            value: "{query}",
            oninput: move |event: FormEvent| {
                say(format!("input {}", event.value()));
                query.set(event.value());
            },
            oncompositionstart: move |_| say("compositionstart".into()),
            oncompositionupdate: move |event: CompositionEvent| {
                say(format!("compositionupdate {}", event.data().data()))
            },
            oncompositionend: move |event: CompositionEvent| {
                say(format!("compositionend {}", event.data().data()))
            },
        }
    }
}
