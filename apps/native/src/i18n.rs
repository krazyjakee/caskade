//! The native UI's language (#91): `caskade_ui_i18n`'s text in Dioxus.
//!
//! - Text goes through `caskade_ui_i18n`'s `t!`, `tn!`, `tp!` and `tnp!`
//!   (the webview's `app` and `ui` domains, so their translations), and
//!   rich text through [`trans!`](crate::trans): numbered tags in the
//!   translation (`<0>…</0>`) become the elements the code gives.
//! - Every lookup reads [`LANGUAGE`], so a component that showed text
//!   renders again when the language changes: the Language setting applies
//!   at once, in every open view, without a restart.
//! - [`use_language`] at the root follows the core's language (the
//!   setting, `system` for the OS's). In debug builds `CASKADE_LANG`
//!   overrides it, and `en-XA` (from either) is the pseudo-locale.

use dioxus_native::prelude::dioxus_core::Runtime;
use dioxus_native::prelude::*;

/// The language the UI shows, as `caskade_ui_i18n` resolved it.
pub static LANGUAGE: GlobalSignal<String> = Signal::global(String::new);

/// Subscribes the component rendering now to the language.
fn observe() {
    if Runtime::try_current().is_some() {
        let _ = LANGUAGE.read();
    }
}

/// The language to show: `CASKADE_LANG` in debug builds, then the
/// Language setting when it asks for the pseudo-locale, else the core's.
async fn requested() -> String {
    if cfg!(debug_assertions)
        && let Ok(language) = std::env::var("CASKADE_LANG")
        && !language.trim().is_empty()
    {
        return language;
    }
    let setting = caskade_app::runtime::spawn(async {
        let services = crate::shell::services()?;
        let general: caskade_core::settings::GeneralSettings =
            services.settings.read_or_default("core.general").await;
        Some(general.language)
    })
    .await
    .ok()
    .flatten();
    match setting {
        Some(language) if caskade_ui_i18n::is_pseudo(&language) => language,
        _ => caskade_core::i18n::locale(),
    }
}

fn show(language: &str) {
    let resolved = caskade_ui_i18n::set_language(language);
    if *LANGUAGE.peek() != resolved {
        *LANGUAGE.write() = resolved;
    }
}

/// Follows the core's language for the rest of the window's life: at the
/// root, before any text.
pub fn use_language() {
    use_hook(|| {
        caskade_ui_i18n::set_observer(observe);
        show(&caskade_core::i18n::locale());
        spawn(async {
            show(&requested().await);
            // The core's language: the setting, or the OS's.
            let mut core = caskade_core::i18n::subscribe();
            while core.changed().await.is_ok() {
                show(&requested().await);
            }
        });
        // The setting itself, for the pseudo-locale, which the core
        // resolves to English and so doesn't announce.
        spawn(async {
            use tokio::sync::broadcast::error::RecvError;
            // The services start after the window (and never in fixtures
            // mode or a test: then it gives up).
            let mut waited = 0;
            let services = loop {
                if let Some(services) = crate::shell::services() {
                    break services;
                }
                waited += 1;
                if waited > 1200 {
                    return;
                }
                let _ = caskade_app::runtime::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                })
                .await;
            };
            let mut settings = services.settings.subscribe();
            show(&requested().await);
            loop {
                match settings.recv().await {
                    Ok(changed) if changed.namespace == "core.general" => {
                        show(&requested().await);
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        });
    });
}

/// Rich text's tree, with each numbered tag built by its function: the
/// element around its translated words.
#[allow(
    dead_code,
    reason = "trans! calls it, and the views (#96 onwards) use trans!"
)]
pub fn rich(
    nodes: &[caskade_ui_i18n::Node],
    build: &[(usize, &dyn Fn(Element) -> Element)],
) -> Element {
    let parts = nodes.iter().map(|node| match node {
        caskade_ui_i18n::Node::Text(text) => rsx! { "{text}" },
        caskade_ui_i18n::Node::Element { index, children } => {
            let inner = rich(children, build);
            match build.iter().find(|(at, _)| at == index) {
                Some((_, make)) => make(inner),
                // A tag the code doesn't give: its words alone.
                None => inner,
            }
        }
    });
    rsx! {
        for part in parts {
            {part}
        }
    }
}

/// Rich text, as the webview's `<Trans>`: the message's numbered tags
/// become elements, in the order the translation puts them.
///
/// ```ignore
/// trans!("<0>{guide}</0> opens the quick menu", guide = glyph;
///        0 => |inner| rsx! { kbd { {inner} } })
/// ```
#[macro_export]
macro_rules! trans {
    ($id:literal $(, $name:tt = $value:expr)* ; $($index:literal => $build:expr),+ $(,)?) => {
        $crate::i18n::rich(
            &caskade_ui_i18n::rich(
                None,
                $id,
                &[$((stringify!($name), &$value as &dyn caskade_ui_i18n::Value)),*],
            ),
            &[$(($index, &$build as &dyn Fn(Element) -> Element)),+],
        )
    };
}

/// [`trans!`] in a context.
#[macro_export]
macro_rules! transp {
    ($context:literal, $id:literal $(, $name:tt = $value:expr)* ; $($index:literal => $build:expr),+ $(,)?) => {
        $crate::i18n::rich(
            &caskade_ui_i18n::rich(
                Some($context),
                $id,
                &[$((stringify!($name), &$value as &dyn caskade_ui_i18n::Value)),*],
            ),
            &[$(($index, &$build as &dyn Fn(Element) -> Element)),+],
        )
    };
}
