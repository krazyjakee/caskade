//! Generated schema reference (`docs/theming/backdrop.md`) for `backdrop.json`.

use super::MAX_SHADERS;

/// One field of the schema, for the generated reference.
pub struct Field {
    /// Where it goes: `(root)`, a layer type, `stop`, `motion`, `mask`.
    pub on: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub required: bool,
    pub default: &'static str,
    pub description: &'static str,
}

const fn field(
    on: &'static str,
    name: &'static str,
    kind: &'static str,
    required: bool,
    default: &'static str,
    description: &'static str,
) -> Field {
    Field {
        on,
        name,
        kind,
        required,
        default,
        description,
    }
}

/// Every field [`parse`](super::parse) accepts.
pub const FIELDS: &[Field] = &[
    field(
        "(root)",
        "base",
        "colour",
        false,
        "`{color.surface.base}`",
        "The window's own colour, beneath every layer and shown alone when the backdrop is hidden.",
    ),
    field(
        "(root)",
        "frameRate",
        "number, 1–60",
        false,
        "`30`",
        "Frames a second while a layer moves. Slow, soft motion needs few.",
    ),
    field(
        "(root)",
        "layers",
        "array of layers",
        true,
        "",
        "Up to 8, drawn in order, each over the ones before. `[]` is a flat backdrop.",
    ),
    field(
        "every layer",
        "type",
        "`linear`, `radial`, `grid`, `image`, `shader`",
        true,
        "",
        "What the layer draws.",
    ),
    field(
        "every layer",
        "opacity",
        "number, 0–1",
        false,
        "`1`",
        "0 hides the layer, and a hidden layer's motion needs no frames.",
    ),
    field(
        "every layer",
        "blend",
        "`normal`, `multiply`, `screen`",
        false,
        "`normal`",
        "How the layer combines with the layers under it.",
    ),
    field(
        "every layer",
        "motion",
        "motion",
        false,
        "none",
        "How the layer's content moves; its mask stays put.",
    ),
    field(
        "every layer",
        "mask",
        "mask",
        false,
        "none",
        "Fades the layer out away from an ellipse.",
    ),
    field(
        "linear",
        "angle",
        "number (degrees)",
        false,
        "`180`",
        "As CSS `linear-gradient`: 0 points up, 90 right.",
    ),
    field(
        "linear, radial",
        "stops",
        "array of 2–4 stops",
        true,
        "",
        "The gradient's colours, in order.",
    ),
    field(
        "radial",
        "at",
        "[length, length]",
        false,
        "`[\"50%\", \"50%\"]`",
        "The ellipse's centre; `%` of the backdrop's width and height.",
    ),
    field(
        "radial",
        "size",
        "[length, length]",
        true,
        "",
        "The ellipse's radii, as CSS `radial-gradient(40% 35% at …)`.",
    ),
    field("grid", "color", "colour", true, "", "The lines' colour."),
    field(
        "grid",
        "cell",
        "length or [length, length]",
        true,
        "",
        "A cell's width and height.",
    ),
    field(
        "grid",
        "line",
        "length",
        false,
        "`1px`",
        "The lines' width, along each cell's top and left.",
    ),
    field(
        "image",
        "src",
        "path",
        true,
        "",
        "A PNG, JPEG or WebP under `assets/`, at most 4096 pixels a side. Up to 4 images.",
    ),
    field(
        "image",
        "size",
        "`cover`, `contain`, `auto` or [length, length]",
        false,
        "`auto`",
        "As CSS `background-size`; `auto` is the image's own size in CSS pixels.",
    ),
    field(
        "image",
        "at",
        "[length, length]",
        false,
        "`[\"0%\", \"0%\"]`",
        "As CSS `background-position`: `50%` centres the image.",
    ),
    field(
        "image",
        "repeat",
        "boolean",
        false,
        "`true`",
        "Tiles the image in both directions.",
    ),
    field(
        "shader",
        "src",
        "path",
        true,
        "",
        "A `.wgsl` file under `assets/` defining `fn layer(input: Input) -> vec4<f32>` (see Shaders below). Up to 2 shader layers.",
    ),
    field(
        "shader",
        "colors",
        "array of up to 4 colours",
        false,
        "`[]`",
        "`input.colors`, straight alpha: tokens keep the shader in the user's accent and colour scheme.",
    ),
    field(
        "shader",
        "numbers",
        "array of up to 4 numbers",
        false,
        "`[]`",
        "`input.numbers`: tokens such as `{effect.glow.strength}` let Reduce effects reach the shader.",
    ),
    field(
        "shader",
        "animate",
        "boolean",
        false,
        "`true`",
        "Whether `input.time` runs. A still shader is drawn once and asks for no frames.",
    ),
    field(
        "shader",
        "loop",
        "duration, 1s–1h",
        false,
        "`3600s`",
        "`input.time` runs from 0 to this, then again: make the shader seamless at it.",
    ),
    field(
        "stop",
        "color",
        "colour",
        true,
        "",
        "The colour at this stop; `transparent` fades out.",
    ),
    field(
        "stop",
        "alpha",
        "number, 0–1",
        false,
        "`1`",
        "Multiplies the colour's alpha, as `color-mix(in oklab, <color> 22%, transparent)` does.",
    ),
    field(
        "stop",
        "at",
        "percentage",
        true,
        "",
        "Where along the gradient line (linear) or radius (radial).",
    ),
    field(
        "motion",
        "type",
        "`drift`, `pan`",
        true,
        "",
        "`drift` eases out by `by` and back every `period`; `pan` moves steadily by `by` every `period`, for grids and repeating images.",
    ),
    field("motion", "by", "[length, length]", true, "", "How far."),
    field(
        "motion",
        "period",
        "duration, 1s–1h",
        true,
        "",
        "How long one cycle takes.",
    ),
    field(
        "mask",
        "at",
        "[length, length]",
        true,
        "",
        "The ellipse's centre.",
    ),
    field(
        "mask",
        "size",
        "[length, length]",
        true,
        "",
        "The ellipse's radii.",
    ),
    field(
        "mask",
        "from",
        "percentage",
        true,
        "",
        "Fully shown inside this fraction of the radii.",
    ),
    field(
        "mask",
        "to",
        "percentage",
        true,
        "",
        "Gone from this fraction of the radii.",
    ),
];

/// A [`Field::on`] as a heading: `every layer` → `Every layer`.
fn heading(on: &str) -> String {
    if on == "(root)" {
        return "The file".to_owned();
    }
    let mut chars = on.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// `docs/theming/backdrop.md`: the schema's reference, from [`FIELDS`] and
/// [`crate::shader::INPUT`].
pub fn reference() -> String {
    let mut out = String::from(
        "# Backdrop compositions\n\n\
         Generated from `crates/caskade-theme/src/backdrop/`: \
         `CASKADE_WRITE_BACKDROP=1 cargo test -p caskade-theme --test backdrop`. Do not edit.\n\n\
         A theme's `backdrop.json` describes the backdrop the native app draws on the GPU \
         beneath its window (#149). How it fits with `theme.css`, inheritance and custom \
         CSS is in [the theming contract](README.md#backdrops-149).\n\n\
         Colours are a hex literal, `transparent` or a `{color.*}` token; numbers a \
         literal or a `{number}` token; lengths `px`, `rem`, `em`, `%`, `vw` or `vh` \
         (`%` of the backdrop's width or height) or a `{dimension}` token; durations \
         `s` or `ms` or a `{duration}` token. Tokens are read from the backdrop's \
         computed style, so Appearance, the user's accent and custom CSS reach them.\n",
    );
    let mut on = "";
    for field in FIELDS {
        if field.on != on {
            on = field.on;
            out.push_str(&format!(
                "\n## {}\n\n| Field | Type | Required | Default | |\n| --- | --- | --- | --- | --- |\n",
                heading(on)
            ));
        }
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            field.name,
            field.kind,
            if field.required { "yes" } else { "" },
            field.default,
            field.description
        ));
    }
    out.push_str(&format!(
        "\n## Shaders\n\n\
         A `shader` layer's file defines `fn layer(input: Input) -> vec4<f32>`, returning \
         the layer's colour at a pixel: straight alpha, sRGB, 0 to 1. It is given:\n\n\
         ```wgsl\n{}```\n\n\
         It may declare helper functions, structs and constants, but no module-scope \
         `var`, bindings, textures, `override`s or entry points: it can read nothing but \
         its input. Files are at most {} KiB; a backdrop has at most {} shader layers. \
         `caskade-theme lint` parses and validates it, and every name it declares is \
         prefixed, so it can't clash with another theme's shader or the app's own code.\n\n\
         A shader runs only when the backdrop moves or changes: the window's frames \
         compose the backdrop as last drawn. The GPU times it where it can, and a shader \
         that takes more than 8 ms a frame, or 40 ms a second of motion, stops moving: \
         it stays on its last frame. Without a usable GPU, the \
         theme's CSS backdrop is drawn instead.\n",
        crate::shader::INPUT,
        crate::shader::MAX_SOURCE / 1024,
        MAX_SHADERS
    ));
    out
}
