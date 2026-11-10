//! A push button, as `@caskade/ui`'s `Button`: four variants, two sizes, an
//! optional leading icon. Click, Enter, Space and a pad's A press it.

use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::prelude::*;

use super::icons::{Icon, Svg};
use super::interact::{Keys, flag, use_interaction};

pub const ANATOMY: Entry = Entry {
    parts: &[("icon", "The leading icon.")],
    states: &[
        ("data-variant", &["primary", "secondary", "ghost", "danger"]),
        ("data-size", &["sm", "md"]),
    ],
    interaction: &[
        "data-hovered",
        "data-pressed",
        "data-focus-visible",
        "data-disabled",
        "data-focused",
        "data-pending",
    ],
    ..Entry::new(
        "button",
        "Button (apps/native/src/ui/button.rs)",
        Source::Ui,
        "A push button.",
    )
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    #[default]
    Secondary,
    Ghost,
    Danger,
}

impl ButtonVariant {
    fn name(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Ghost => "ghost",
            Self::Danger => "danger",
        }
    }

    fn classes(self) -> &'static str {
        match self {
            Self::Primary => {
                "border-transparent bg-linear-to-r from-accent to-accent-strong text-accent-fg data-hovered:brightness-115 data-hovered:shadow-[0_0_18px_-4px] data-hovered:shadow-glow"
            }
            Self::Secondary => {
                "border-border-strong bg-surface-overlay/70 text-fg backdrop-blur data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover"
            }
            Self::Ghost => {
                "border-transparent text-fg-muted data-hovered:bg-surface-hover data-hovered:text-fg"
            }
            Self::Danger => {
                "border-danger/40 bg-danger/10 text-danger data-hovered:border-danger/70 data-hovered:bg-danger/20"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonSize {
    Sm,
    #[default]
    Md,
}

impl ButtonSize {
    fn name(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
        }
    }
}

#[component]
pub fn Button(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] size: ButtonSize,
    #[props(default)] icon: Option<Icon>,
    #[props(default)] disabled: bool,
    /// Busy with what it started: shown as `data-pending`, and not pressable.
    #[props(default)]
    pending: bool,
    /// Pressed: a click, Enter, Space or a pad's A.
    #[props(default)]
    on_press: Option<EventHandler<()>>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let interaction = use_interaction();
    let press = move || {
        if !disabled
            && !pending
            && let Some(on_press) = on_press
        {
            on_press.call(());
        }
    };
    let sizes = match size {
        ButtonSize::Sm => "h-8 gap-1.5 px-3 text-xs",
        ButtonSize::Md => "h-9 gap-2 px-4 text-sm",
    };
    let icon_size = match size {
        ButtonSize::Sm => "size-3.5",
        ButtonSize::Md => "size-4",
    };
    rsx! {
        button {
            r#type: "button",
            "data-cs": "button",
            "data-variant": variant.name(),
            "data-size": size.name(),
            "data-hovered": interaction.hovered(),
            "data-pressed": interaction.pressed(),
            "data-focused": interaction.focused(),
            "data-pending": flag(pending),
            "data-disabled": flag(disabled),
            "aria-disabled": flag(disabled),
            class: "inline-flex shrink-0 items-center justify-center rounded-control border font-medium whitespace-nowrap transition-[background-color,border-color,box-shadow,filter,scale] duration-(--cs-motion-duration-fast) data-disabled:opacity-40 data-focus-visible:focus-ring data-pressed:scale-[0.97] {variant.classes()} {sizes} {class}",
            onfocus: move |_| interaction.focus(),
            onblur: move |_| interaction.blur(),
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onclick: move |_| press(),
            onkeydown: move |event| {
                interaction.key_down(&event, Keys::EnterAndSpace, press);
            },
            onkeyup: move |event| interaction.key_up(&event, press),
            ..attributes,
            if let Some(icon) = icon {
                Svg { icon, part: "icon", class: icon_size }
            }
            {children}
        }
    }
}
