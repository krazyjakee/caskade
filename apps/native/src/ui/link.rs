//! A link inside the app: `<a href>` for its role and focus, going nowhere
//! itself. `on_press` takes the click, Enter (not Space, per the APG) or a
//! pad's A (the router's
//! navigation, an external URL through the shell). It renders no `data-cs`:
//! a link is part of the component or view around it, which styles it and
//! names it with `data-part`.

use dioxus_native::prelude::*;

use super::interact::{Keys, flag, use_interaction};

#[component]
pub fn Link(
    /// Where it goes, for its role and for screen readers; the app routes.
    href: String,
    #[props(default)] on_press: Option<EventHandler<()>>,
    /// The page it points at is the one shown (`aria-current="page"`).
    #[props(default)]
    current: bool,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let interaction = use_interaction();
    let press = move || {
        if let Some(on_press) = on_press {
            on_press.call(());
        }
    };
    rsx! {
        a {
            href: "{href}",
            "aria-current": current.then_some("page"),
            "data-hovered": interaction.hovered(),
            "data-pressed": interaction.pressed(),
            "data-current": flag(current),
            class: "{class}",
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onclick: move |event| {
                event.prevent_default();
                press();
            },
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::Enter, press);
            },
            ..attributes,
            {children}
        }
    }
}
