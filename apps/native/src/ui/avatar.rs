//! An account's picture and the picker of one, with the shipped avatars
//! drawn as SVG data URLs, so they need no network and stay crisp.
//!
//! An id (`avatar:<name>`) names the same picture in every release; the core
//! keeps the same list (`caskade_core::accounts::AVATARS`), and tests keep
//! it in step and each picture as `assets/avatars/<name>.svg` has it.

use std::sync::OnceLock;

use blitz_dom::NodeId;
use caskade_ui_i18n::{t, tp};
use caskade_ui_input::Action;
use caskade_ui_theme::anatomy::{Entry, Source};
use dioxus_native::NodeHandle;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use dioxus_native::prelude::*;

use super::icons::{CHECK, Svg};
use super::interact::{Keys, flag, use_interaction_where};
use crate::input::{Bind, use_input, use_input_action};

pub const ANATOMY: Entry = Entry {
    parts: &[
        ("image", "The avatar image."),
        ("initials", "The initials shown instead."),
    ],
    states: &[
        ("data-size", &["sm", "md", "lg", "xl"]),
        ("data-source", &["image", "initials"]),
    ],
    ..Entry::new(
        "avatar",
        "Avatar (apps/native/src/ui/avatar.rs)",
        Source::Ui,
        "An account's picture: one of the shipped avatars, or the name's initials when its id is unknown or the image can't load.",
    )
};

pub const PICKER_ANATOMY: Entry = Entry {
    parts: &[
        (
            "option",
            "An avatar to choose (an `avatar` inside); carries `data-selected`, `data-hovered` and `data-focus-visible`.",
        ),
        ("check", "The tick on the chosen avatar."),
    ],
    states: &[("data-size", &["md", "lg", "xl"])],
    interaction: &["data-disabled", "data-orientation"],
    contains: &["avatar"],
    ..Entry::new(
        "avatar-picker",
        "AvatarPicker (apps/native/src/ui/avatar.rs)",
        Source::Ui,
        "A grid of the shipped avatars to choose one from (a radio group).",
    )
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AvatarSize {
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
}

impl AvatarSize {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
            Self::Xl => "xl",
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::Sm => "size-9 text-xs",
            Self::Md => "size-11 text-sm",
            Self::Lg => "size-20 text-2xl",
            Self::Xl => "size-32 text-4xl",
        }
    }
}

/// Up to two letters from a name: "Ada Lovelace" → "AL", "kid" → "K".
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let picked: Vec<&str> = match words.as_slice() {
        [] => Vec::new(),
        [one] => vec![*one],
        [first, .., last] => vec![*first, *last],
    };
    let letters: String = picked
        .iter()
        .filter_map(|word| word.chars().next())
        .collect::<String>()
        .to_uppercase();
    if letters.is_empty() {
        "?".to_owned()
    } else {
        letters
    }
}

/// A drawing: the tile's hue and its glyph, whose `"L"`, `"M"` and `"D"`
/// are the light ink, the mid tone and the tile's dark.
struct Drawing {
    name: &'static str,
    hue: u16,
    glyph: &'static str,
}

/// The drawings, in the picker's order; the first is the default.
const DRAWINGS: &[Drawing] = &[
    Drawing {
        name: "default",
        hue: 200,
        glyph: "<circle cx=\"32\" cy=\"32\" r=\"14\"/><path d=\"M38.5 23.5l4 4-16 16-4-4z\" fill=\"M\" stroke=\"M\" stroke-width=\"1\"/>",
    },
    Drawing {
        name: "astronaut",
        hue: 210,
        glyph: "<circle cx=\"32\" cy=\"27\" r=\"14\"/><rect x=\"22\" y=\"20\" width=\"20\" height=\"12\" rx=\"6\" fill=\"M\" fill-opacity=\".55\"/><path d=\"M27 23.5h5\"/><path d=\"M13 56c2-9 9-14 19-14s17 5 19 14\"/><path d=\"M28 48h8\"/>",
    },
    Drawing {
        name: "droid",
        hue: 185,
        glyph: "<rect x=\"17\" y=\"21\" width=\"30\" height=\"25\" rx=\"6\"/><path d=\"M32 21v-6\"/><circle cx=\"32\" cy=\"12.5\" r=\"2.5\" fill=\"L\"/><circle cx=\"25.5\" cy=\"32\" r=\"3.2\" fill=\"L\"/><circle cx=\"38.5\" cy=\"32\" r=\"3.2\" fill=\"L\"/><path d=\"M26 40h12M12 29v9M52 29v9M24 46v6M40 46v6\"/>",
    },
    Drawing {
        name: "rocket",
        hue: 15,
        glyph: "<path d=\"M32 9c7 6 10 14 10 24l-3 9H25l-3-9c0-10 3-18 10-24z\"/><circle cx=\"32\" cy=\"26\" r=\"4\" fill=\"M\" fill-opacity=\".6\"/><path d=\"M22 34l-7 9 9-1M42 34l7 9-9-1\"/><path d=\"M28 47l4 8 4-8\" stroke=\"M\"/>",
    },
    Drawing {
        name: "planet",
        hue: 35,
        glyph: "<circle cx=\"32\" cy=\"32\" r=\"12\" fill=\"M\" fill-opacity=\".35\"/><path d=\"M22 36c-4 2-6 4-5 6 1.5 3 12 1 22-4s17-11 15.5-14c-1-2-4-2-8-1\"/><path d=\"M42 28c4-2 6-4 5-6-1.5-3-12-1-22 4\"/>",
    },
    Drawing {
        name: "moon",
        hue: 240,
        glyph: "<path d=\"M38 12a20 20 0 1 0 14 30A16 16 0 0 1 38 12z\"/><circle cx=\"27\" cy=\"38\" r=\"3\"/><circle cx=\"21\" cy=\"29\" r=\"1.8\" fill=\"L\"/><circle cx=\"33\" cy=\"46\" r=\"1.6\" fill=\"L\"/>",
    },
    Drawing {
        name: "comet",
        hue: 195,
        glyph: "<circle cx=\"42\" cy=\"22\" r=\"7\" fill=\"L\"/><path d=\"M36.5 27.5L13 51M35 20.5L17 31M43.5 29.5L33 47\" /><path d=\"M24 40l-6 6\" stroke=\"M\"/>",
    },
    Drawing {
        name: "atom",
        hue: 165,
        glyph: "<circle cx=\"32\" cy=\"32\" r=\"3.5\" fill=\"L\"/><ellipse cx=\"32\" cy=\"32\" rx=\"21\" ry=\"8\"/><ellipse cx=\"32\" cy=\"32\" rx=\"21\" ry=\"8\" transform=\"rotate(60 32 32)\"/><ellipse cx=\"32\" cy=\"32\" rx=\"21\" ry=\"8\" transform=\"rotate(-60 32 32)\"/>",
    },
    Drawing {
        name: "satellite",
        hue: 220,
        glyph: "<rect x=\"26\" y=\"26\" width=\"12\" height=\"12\" rx=\"2\" fill=\"M\" fill-opacity=\".4\"/><rect x=\"8\" y=\"27\" width=\"14\" height=\"10\" rx=\"1\"/><rect x=\"42\" y=\"27\" width=\"14\" height=\"10\" rx=\"1\"/><path d=\"M22 32h4M38 32h4M15 27v10M49 27v10M32 26v-7M27 15.5a7 7 0 0 1 10 0\"/>",
    },
    Drawing {
        name: "ufo",
        hue: 140,
        glyph: "<ellipse cx=\"32\" cy=\"34\" rx=\"22\" ry=\"7\"/><path d=\"M21 31a11 11 0 0 1 22 0\" fill=\"M\" fill-opacity=\".4\"/><circle cx=\"23\" cy=\"35\" r=\"1.6\" fill=\"L\"/><circle cx=\"32\" cy=\"36.5\" r=\"1.6\" fill=\"L\"/><circle cx=\"41\" cy=\"35\" r=\"1.6\" fill=\"L\"/><path d=\"M22 46l-4 8M32 45v9M42 46l4 8\" stroke=\"M\"/>",
    },
    Drawing {
        name: "alien",
        hue: 110,
        glyph: "<path d=\"M32 11c11 0 18 8 18 18 0 12-10 23-18 23S14 41 14 29c0-10 7-18 18-18z\"/><path d=\"M19 29c5 0 10 2 10 8-5 0-10-2-10-8zM45 29c-5 0-10 2-10 8 5 0 10-2 10-8z\" fill=\"L\"/><path d=\"M29 45h6\"/>",
    },
    Drawing {
        name: "star",
        hue: 48,
        glyph: "<path d=\"M32 11l6.2 12.6 13.8 2-10 9.8 2.4 13.8L32 42.7l-12.4 6.5 2.4-13.8-10-9.8 13.8-2z\" fill=\"M\" fill-opacity=\".35\"/><path d=\"M32 26v8M28 30h8\" stroke=\"L\"/>",
    },
    Drawing {
        name: "galaxy",
        hue: 280,
        glyph: "<circle cx=\"32\" cy=\"32\" r=\"3\" fill=\"L\"/><path d=\"M35 32c0-6-9-7-12-1s2 15 11 14 15-9 12-19-16-14-24-8\"/><path d=\"M29 32c0 6 9 7 12 1\"/><circle cx=\"16\" cy=\"44\" r=\"1.4\" fill=\"L\"/><circle cx=\"50\" cy=\"16\" r=\"1.4\" fill=\"L\"/>",
    },
    Drawing {
        name: "pulsar",
        hue: 300,
        glyph: "<circle cx=\"32\" cy=\"32\" r=\"5\" fill=\"L\"/><circle cx=\"32\" cy=\"32\" r=\"12\"/><circle cx=\"32\" cy=\"32\" r=\"20\" stroke-dasharray=\"4 6\" stroke=\"M\"/><path d=\"M32 6v10M32 48v10\"/>",
    },
    Drawing {
        name: "crystal",
        hue: 175,
        glyph: "<path d=\"M32 9l14 15-14 31-14-31z\" fill=\"M\" fill-opacity=\".3\"/><path d=\"M18 24h28M26 24l6 31 6-31M26 24l6-15 6 15\"/>",
    },
    Drawing {
        name: "radar",
        hue: 130,
        glyph: "<circle cx=\"32\" cy=\"32\" r=\"21\"/><circle cx=\"32\" cy=\"32\" r=\"12\" stroke=\"M\"/><path d=\"M32 11v42M11 32h42\" stroke=\"M\" stroke-width=\"1.5\"/><path d=\"M32 32L47 17\" /><path d=\"M32 32L47 17a21 21 0 0 1 6 15z\" fill=\"M\" fill-opacity=\".45\" stroke=\"none\"/><circle cx=\"40\" cy=\"40\" r=\"2.2\" fill=\"L\"/>",
    },
    Drawing {
        name: "chip",
        hue: 155,
        glyph: "<rect x=\"19\" y=\"19\" width=\"26\" height=\"26\" rx=\"3\"/><rect x=\"27\" y=\"27\" width=\"10\" height=\"10\" rx=\"1\" fill=\"L\"/><path d=\"M26 19v-7M32 19v-7M38 19v-7M26 45v7M32 45v7M38 45v7M19 26h-7M19 32h-7M19 38h-7M45 26h7M45 32h7M45 38h7\"/>",
    },
    Drawing {
        name: "ghost",
        hue: 260,
        glyph: "<path d=\"M18 52V30a14 14 0 0 1 28 0v22l-5-4-4.5 4-4.5-4-4.5 4-4.5-4z\"/><circle cx=\"26.5\" cy=\"31\" r=\"2.8\" fill=\"L\"/><circle cx=\"37.5\" cy=\"31\" r=\"2.8\" fill=\"L\"/>",
    },
    Drawing {
        name: "mech",
        hue: 0,
        glyph: "<path d=\"M15 41V28a17 17 0 0 1 34 0v13l-7 9H22z\"/><path d=\"M20 29h24l-3 8H23z\" fill=\"L\"/><path d=\"M32 11v7M24 50l2-6h12l2 6\"/>",
    },
    Drawing {
        name: "bolt",
        hue: 55,
        glyph: "<path d=\"M36 8L17 36h13l-4 20 21-30H34z\" fill=\"M\" fill-opacity=\".4\"/>",
    },
    Drawing {
        name: "sentinel",
        hue: 345,
        glyph: "<path d=\"M9 32c6-10 14-15 23-15s17 5 23 15c-6 10-14 15-23 15S15 42 9 32z\"/><circle cx=\"32\" cy=\"32\" r=\"8\"/><circle cx=\"32\" cy=\"32\" r=\"3\" fill=\"L\"/>",
    },
    Drawing {
        name: "starfighter",
        hue: 225,
        glyph: "<path d=\"M32 9l6 22 15 11v6l-15-4-2 8h-8l-2-8-15 4v-6l15-11z\" fill=\"M\" fill-opacity=\".3\"/><path d=\"M32 22v14\" stroke=\"L\"/>",
    },
    Drawing {
        name: "portal",
        hue: 270,
        glyph: "<circle cx=\"32\" cy=\"32\" r=\"21\"/><ellipse cx=\"33\" cy=\"33\" rx=\"14\" ry=\"16\"/><ellipse cx=\"34\" cy=\"34\" rx=\"8\" ry=\"10\" stroke=\"M\"/><ellipse cx=\"35\" cy=\"35\" rx=\"3\" ry=\"4.5\" fill=\"L\"/>",
    },
    Drawing {
        name: "cybercat",
        hue: 320,
        glyph: "<path d=\"M18 27l-2-14 11 8h10l11-8-2 14c3 4 4 8 4 12 0 9-7 14-18 14S14 48 14 39c0-4 1-8 4-12z\"/><path d=\"M21 34h9l-1 4h-7zM43 34h-9l1 4h7z\" fill=\"L\"/><path d=\"M29 44l3 2 3-2M10 40h8M46 40h8\"/>",
    },
    Drawing {
        name: "gamepad",
        hue: 190,
        glyph: "<path d=\"M20 22h24c6 0 10 6 11 14s0 14-5 14c-4 0-6-6-10-6H24c-4 0-6 6-10 6-5 0-6-6-5-14s5-14 11-14z\"/><path d=\"M22 30v9M17.5 34.5h9\"/><circle cx=\"41\" cy=\"31\" r=\"2.2\" fill=\"L\"/><circle cx=\"46\" cy=\"36.5\" r=\"2.2\" fill=\"L\"/>",
    },
];

/// One of the shipped avatars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvatarInfo {
    index: usize,
}

impl AvatarInfo {
    /// `avatar:<name>`.
    pub fn id(self) -> String {
        format!("avatar:{}", DRAWINGS[self.index].name)
    }

    /// What the picture shows, translated, for screen readers and the picker.
    pub fn label(self) -> String {
        label(DRAWINGS[self.index].name)
    }

    /// Its image, a `data:` URL.
    pub fn url(self) -> &'static str {
        static URLS: OnceLock<Vec<String>> = OnceLock::new();
        &URLS.get_or_init(|| {
            DRAWINGS
                .iter()
                .map(|drawing| {
                    format!("data:image/svg+xml,{}", encode_uri_component(&svg(drawing)))
                })
                .collect()
        })[self.index]
    }
}

fn label(name: &str) -> String {
    match name {
        "default" => tp!(
            note = "The avatar showing the Caskade logo; usually kept as the brand name",
            "avatar",
            "Caskade"
        ),
        "astronaut" => t!("Astronaut"),
        "droid" => t!("Droid"),
        "rocket" => t!("Rocket"),
        "planet" => t!("Ringed planet"),
        "moon" => t!("Moon"),
        "comet" => t!("Comet"),
        "atom" => t!("Atom"),
        "satellite" => t!("Satellite"),
        "ufo" => t!("Flying saucer"),
        "alien" => t!("Alien"),
        "star" => t!("Star"),
        "galaxy" => t!("Spiral galaxy"),
        "pulsar" => t!("Pulsar"),
        "crystal" => t!("Crystal"),
        "radar" => t!("Radar"),
        "chip" => t!("Chip"),
        "ghost" => t!("Ghost"),
        "mech" => t!("Mech helmet"),
        "bolt" => t!("Energy bolt"),
        "sentinel" => t!("Sentinel eye"),
        "starfighter" => t!("Starfighter"),
        "portal" => t!("Portal"),
        "cybercat" => t!("Cyber cat"),
        "gamepad" => t!("Gamepad"),
        _ => name.to_owned(),
    }
}

/// Every shipped avatar, in the picker's order.
pub fn avatars() -> impl Iterator<Item = AvatarInfo> {
    (0..DRAWINGS.len()).map(|index| AvatarInfo { index })
}

pub const DEFAULT_AVATAR: &str = "avatar:default";

/// The avatar an id names, or `None` for one this release doesn't ship.
pub fn avatar_info(id: Option<&str>) -> Option<AvatarInfo> {
    let name = id?.strip_prefix("avatar:")?;
    DRAWINGS
        .iter()
        .position(|drawing| drawing.name == name)
        .map(|index| AvatarInfo { index })
}

/// An HSL colour as hex, as the webview computed it.
fn hex(h: f64, s: f64, l: f64) -> String {
    let sat = s / 100.0;
    let light = l / 100.0;
    let k = |n: f64| (n + h / 30.0) % 12.0;
    let a = sat * light.min(1.0 - light);
    let channel = |n: f64| {
        let value = light - a * (-1f64).max((k(n) - 3.0).min(9.0 - k(n)).min(1.0));
        // JavaScript's Math.round: halves round up.
        format!("{:02x}", (value * 255.0 + 0.5).floor() as u8)
    };
    format!("#{}{}{}", channel(0.0), channel(8.0), channel(4.0))
}

fn svg(drawing: &Drawing) -> String {
    let hue = f64::from(drawing.hue);
    let light = hex(hue, 95.0, 86.0);
    let mid = hex(hue, 90.0, 62.0);
    let dark = hex(hue, 70.0, 9.0);
    let ink = drawing
        .glyph
        .replace("\"L\"", &format!("\"{light}\""))
        .replace("\"M\"", &format!("\"{mid}\""))
        .replace("\"D\"", &format!("\"{dark}\""));
    [
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\">".to_owned(),
        "<defs><linearGradient id=\"t\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\">".to_owned(),
        format!(
            "<stop offset=\"0\" stop-color=\"{}\"/><stop offset=\"1\" stop-color=\"{dark}\"/>",
            hex(hue, 65.0, 27.0)
        ),
        "</linearGradient><radialGradient id=\"g\" cx=\".5\" cy=\".38\" r=\".55\">".to_owned(),
        format!(
            "<stop offset=\"0\" stop-color=\"{mid}\" stop-opacity=\".5\"/><stop offset=\"1\" stop-color=\"{mid}\" stop-opacity=\"0\"/>"
        ),
        "</radialGradient></defs>".to_owned(),
        "<rect width=\"64\" height=\"64\" fill=\"url(#t)\"/><rect width=\"64\" height=\"64\" fill=\"url(#g)\"/>".to_owned(),
        format!("<path d=\"M0 16H64M0 48H64M16 0V64M48 0V64\" stroke=\"{light}\" stroke-opacity=\".07\"/>"),
        format!(
            "<g fill=\"none\" stroke=\"{light}\" stroke-width=\"2.6\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{ink}</g>"
        ),
        "</svg>".to_owned(),
    ]
    .concat()
}

/// JavaScript's `encodeURIComponent`: everything but A–Z a–z 0–9 and
/// `-_.!~*'()` as UTF-8 percent escapes.
fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// An account's picture: one of the shipped avatars, or the name's
/// initials when the id is unknown (an avatar a later release removed).
/// Decorative unless given a `label`.
#[component]
pub fn Avatar(
    /// An avatar id (`avatar:<name>`).
    avatar: Option<String>,
    /// The account's name, for the initials.
    name: String,
    #[props(default)] size: AvatarSize,
    /// Makes it an image with this accessible name.
    #[props(default)]
    label: Option<String>,
    /// The `data-part` it plays in a parent component.
    #[props(default)]
    part: Option<&'static str>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let url = avatar_info(avatar.as_deref()).map(AvatarInfo::url);
    let sized = size.class();
    rsx! {
        span {
            "data-cs": "avatar",
            "data-part": part,
            "data-size": size.name(),
            "data-source": if url.is_some() { "image" } else { "initials" },
            role: label.as_ref().map(|_| "img"),
            "aria-label": label.clone(),
            "aria-hidden": if label.is_none() { Some("true") } else { None },
            class: "relative inline-grid shrink-0 place-items-center overflow-hidden rounded-full bg-linear-to-br from-accent-strong to-accent-alt font-display font-semibold text-accent-fg select-none {sized} {class}",
            ..attributes,
            if let Some(url) = url {
                img {
                    "data-part": "image",
                    src: url,
                    alt: "",
                    draggable: "false",
                    class: "absolute inset-0 size-full object-cover",
                }
            } else {
                span { "data-part": "initials", {initials(&name)} }
            }
        }
    }
}

/// A grid of avatars to choose from, a radio group: a click, Space or a
/// pad's A chooses; arrows move and choose, as in any radio group; a pad
/// moves spatially. Escape and a pad's B leave without a change.
#[component]
pub fn AvatarPicker(
    /// The chosen avatar's id.
    value: Option<String>,
    on_change: EventHandler<String>,
    #[props(default)] on_cancel: Option<EventHandler<()>>,
    /// The avatars to offer; the shipped set by default.
    #[props(default)]
    options: Option<Vec<AvatarInfo>>,
    /// `Md`, `Lg` or `Xl`.
    #[props(default = AvatarSize::Lg)]
    size: AvatarSize,
    #[props(default)] disabled: bool,
    /// The group's name; "Avatar" by default.
    #[props(default)]
    label: Option<String>,
    #[props(default)] class: String,
    #[props(extends = GlobalAttributes)] attributes: Vec<Attribute>,
) -> Element {
    let options: Vec<AvatarInfo> = options.unwrap_or_else(|| avatars().collect());
    let nodes = use_signal(Vec::<(String, NodeId)>::new);
    let mut me = use_signal(|| None::<NodeId>);
    use_input_action(
        Action::Back,
        Bind {
            owner: me(),
            ..Bind::default()
        },
        move || match on_cancel {
            Some(cancel) => {
                cancel.call(());
                true
            }
            None => false,
        },
    );
    let ids: Vec<String> = options.iter().map(|option| option.id()).collect();
    let stop = value
        .clone()
        .filter(|value| ids.contains(value))
        .or_else(|| ids.first().cloned());
    let neighbour = |at: usize, forward: bool| -> String {
        let next = if forward {
            (at + 1) % ids.len()
        } else {
            (at + ids.len() - 1) % ids.len()
        };
        ids[next].clone()
    };
    let label = label.unwrap_or_else(|| t!(note = "The picker of an account's picture", "Avatar"));
    rsx! {
        div {
            role: "radiogroup",
            "aria-label": "{label}",
            "aria-orientation": "horizontal",
            "aria-disabled": flag(disabled),
            "data-cs": "avatar-picker",
            "data-orientation": "horizontal",
            "data-size": size.name(),
            "data-disabled": flag(disabled),
            class: "grid grid-cols-[repeat(auto-fill,minmax(var(--avatar-cell,5.5rem),1fr))] gap-3 data-disabled:opacity-40 data-[size=xl]:[--avatar-cell:9rem] data-[size=md]:[--avatar-cell:3.5rem] {class}",
            onmounted: move |event| me.set(event.data().downcast::<NodeHandle>().map(NodeHandle::node_id)),
            onkeydown: move |event| {
                if event.key() == Key::Named(NamedKey::Escape)
                    && let Some(cancel) = on_cancel
                {
                    event.stop_propagation();
                    cancel.call(());
                }
            },
            ..attributes,
            for (at, option) in options.iter().copied().enumerate() {
                AvatarOption {
                    key: "{option.id()}",
                    option,
                    size,
                    checked: value.as_deref() == Some(option.id().as_str()),
                    stop: stop.as_deref() == Some(option.id().as_str()),
                    disabled,
                    nodes,
                    on_check: on_change,
                    previous: neighbour(at, false),
                    next: neighbour(at, true),
                }
            }
        }
    }
}

#[component]
fn AvatarOption(
    option: AvatarInfo,
    size: AvatarSize,
    checked: bool,
    stop: bool,
    disabled: bool,
    nodes: Signal<Vec<(String, NodeId)>>,
    on_check: EventHandler<String>,
    /// Where the arrows go: the avatars before and after.
    previous: String,
    next: String,
) -> Element {
    let interaction = use_interaction_where(stop);
    let input = use_input();
    let id = option.id();
    let label = option.label();
    let arrow = use_callback(move |forward: bool| {
        if disabled {
            return;
        }
        let target = if forward {
            next.clone()
        } else {
            previous.clone()
        };
        on_check.call(target.clone());
        if let Some((_, node)) = nodes.peek().iter().find(|(known, _)| *known == target) {
            input.focus(*node);
        }
    });
    let chosen = id.clone();
    let check = use_callback(move |()| {
        if !disabled && !checked {
            on_check.call(chosen.clone());
        }
    });
    let mount_id = id.clone();
    rsx! {
        div {
            role: "radio",
            tabindex: if stop && !disabled { "0" } else { "-1" },
            "aria-checked": if checked { "true" } else { "false" },
            "aria-disabled": flag(disabled),
            "aria-label": "{label}",
            "data-part": "option",
            "data-selected": flag(checked),
            "data-disabled": flag(disabled),
            "data-hovered": interaction.hovered(),
            "data-focus-visible": interaction.ring(),
            "data-pressed": interaction.pressed(),
            class: "group relative grid cursor-default place-items-center rounded-full p-1 outline-none transition-[background-color,box-shadow] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:bg-surface-hover data-selected:bg-accent/25 data-selected:shadow-[inset_0_0_0_2px] data-selected:shadow-accent-alt",
            onmounted: move |event| {
                if let Some(node) = event.data().downcast::<NodeHandle>().map(NodeHandle::node_id) {
                    let mut nodes = nodes;
                    nodes.write().retain(|(known, _)| *known != mount_id);
                    nodes.write().push((mount_id.clone(), node));
                }
            },
            onmouseenter: move |_| interaction.enter(),
            onmouseleave: move |_| interaction.leave(),
            onmousedown: move |_| interaction.down(),
            onmouseup: move |_| interaction.up(),
            onblur: move |_| interaction.blur(),
            onclick: move |_| check(()),
            onkeydown: move |event| {
                let forward = match event.key() {
                    Key::Named(NamedKey::ArrowDown | NamedKey::ArrowRight) => Some(true),
                    Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft) => Some(false),
                    _ => None,
                };
                if let Some(forward) = forward {
                    event.prevent_default();
                    event.stop_propagation();
                    arrow(forward);
                } else {
                    interaction.key_down(&event, Keys::Space, || {});
                }
            },
            onkeyup: move |event| interaction.key_up(&event, || check(())),
            Avatar { avatar: Some(id.clone()), name: label.clone(), size }
            span {
                "data-part": "check",
                "aria-hidden": "true",
                class: "absolute right-1 bottom-1 hidden size-6 place-items-center rounded-full bg-accent-alt text-accent-fg group-data-selected:grid",
                Svg { icon: CHECK, class: "size-4", stroke_width: 3.0 }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_the_first_and_last_words() {
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("kid"), "K");
        assert_eq!(initials("  ada  byron lovelace "), "AL");
        assert_eq!(initials("élodie"), "É");
        assert_eq!(initials("   "), "?");
    }

    #[test]
    fn the_avatars_are_the_cores() {
        let ids: Vec<String> = avatars().map(AvatarInfo::id).collect();
        assert_eq!(ids, caskade_core::accounts::AVATARS);
        assert_eq!(ids[0], DEFAULT_AVATAR);
        assert!(avatar_info(Some("avatar:gone")).is_none());
        assert!(avatar_info(None).is_none());
    }

    #[test]
    fn colours_and_escapes_match_the_webviews() {
        // The webview's hex() for the default avatar's tones (hue 200).
        assert_eq!(hex(200.0, 95.0, 86.0), "#b9e7fd");
        assert_eq!(hex(200.0, 90.0, 62.0), "#47bbf5");
        assert_eq!(hex(200.0, 70.0, 9.0), "#071c27");
        assert_eq!(
            encode_uri_component("<a b=\"#\"/>"),
            "%3Ca%20b%3D%22%23%22%2F%3E"
        );
    }

    /// Every avatar draws as `assets/avatars/<name>.svg`, byte for byte:
    /// the pictures the webview shipped, so an id keeps its image. After
    /// changing a drawing on purpose, `CASKADE_WRITE_AVATARS=1 cargo test
    /// -p caskade-native avatar` rewrites them.
    #[test]
    fn every_avatar_is_drawn_as_shipped() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/avatars");
        let write = std::env::var_os("CASKADE_WRITE_AVATARS").is_some();
        for drawing in DRAWINGS {
            let path = folder.join(format!("{}.svg", drawing.name));
            if write {
                std::fs::write(&path, svg(drawing)).unwrap();
            }
            let shipped = std::fs::read_to_string(&path).unwrap_or_default();
            assert_eq!(svg(drawing), shipped, "{}", path.display());
        }
        let files = std::fs::read_dir(&folder)
            .unwrap()
            .filter(|entry| {
                let path = entry.as_ref().unwrap().path();
                path.extension().is_some_and(|extension| extension == "svg")
            })
            .count();
        assert_eq!(files, DRAWINGS.len(), "an avatar's file without a drawing");
        let default = avatar_info(Some(DEFAULT_AVATAR)).unwrap();
        assert_eq!(
            default.url(),
            format!(
                "data:image/svg+xml,{}",
                encode_uri_component(&std::fs::read_to_string(folder.join("default.svg")).unwrap())
            )
        );
    }
}
