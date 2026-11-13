# Backdrop compositions

Generated from `crates/caskade-theme/src/backdrop.rs`: `CASKADE_WRITE_BACKDROP=1 cargo test -p caskade-theme --test backdrop`. Do not edit.

A theme's `backdrop.json` describes the backdrop the native app draws on the GPU beneath its window (#149). How it fits with `theme.css`, inheritance and custom CSS is in [the theming contract](README.md#backdrops-149).

Colours are a hex literal, `transparent` or a `{color.*}` token; numbers a literal or a `{number}` token; lengths `px`, `rem`, `em`, `%`, `vw` or `vh` (`%` of the backdrop's width or height) or a `{dimension}` token; durations `s` or `ms` or a `{duration}` token. Tokens are read from the backdrop's computed style, so Appearance, the user's accent and custom CSS reach them.

## The file

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `base` | colour |  | `{color.surface.base}` | The window's own colour, beneath every layer and shown alone when the backdrop is hidden. |
| `frameRate` | number, 1–60 |  | `30` | Frames a second while a layer moves. Slow, soft motion needs few. |
| `layers` | array of layers | yes |  | Up to 8, drawn in order, each over the ones before. `[]` is a flat backdrop. |

## Every layer

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `type` | `linear`, `radial`, `grid`, `image`, `shader` | yes |  | What the layer draws. |
| `opacity` | number, 0–1 |  | `1` | 0 hides the layer, and a hidden layer's motion needs no frames. |
| `blend` | `normal`, `multiply`, `screen` |  | `normal` | How the layer combines with the layers under it. |
| `motion` | motion |  | none | How the layer's content moves; its mask stays put. |
| `mask` | mask |  | none | Fades the layer out away from an ellipse. |

## Linear

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `angle` | number (degrees) |  | `180` | As CSS `linear-gradient`: 0 points up, 90 right. |

## Linear, radial

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `stops` | array of 2–4 stops | yes |  | The gradient's colours, in order. |

## Radial

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `at` | [length, length] |  | `["50%", "50%"]` | The ellipse's centre; `%` of the backdrop's width and height. |
| `size` | [length, length] | yes |  | The ellipse's radii, as CSS `radial-gradient(40% 35% at …)`. |

## Grid

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `color` | colour | yes |  | The lines' colour. |
| `cell` | length or [length, length] | yes |  | A cell's width and height. |
| `line` | length |  | `1px` | The lines' width, along each cell's top and left. |

## Image

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `src` | path | yes |  | A PNG, JPEG or WebP under `assets/`, at most 4096 pixels a side. Up to 4 images. |
| `size` | `cover`, `contain`, `auto` or [length, length] |  | `auto` | As CSS `background-size`; `auto` is the image's own size in CSS pixels. |
| `at` | [length, length] |  | `["0%", "0%"]` | As CSS `background-position`: `50%` centres the image. |
| `repeat` | boolean |  | `true` | Tiles the image in both directions. |

## Shader

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `src` | path | yes |  | A `.wgsl` file under `assets/` defining `fn layer(input: Input) -> vec4<f32>` (see Shaders below). Up to 2 shader layers. |
| `colors` | array of up to 4 colours |  | `[]` | `input.colors`, straight alpha: tokens keep the shader in the user's accent and colour scheme. |
| `numbers` | array of up to 4 numbers |  | `[]` | `input.numbers`: tokens such as `{effect.glow.strength}` let Reduce effects reach the shader. |
| `animate` | boolean |  | `true` | Whether `input.time` runs. A still shader is drawn once and asks for no frames. |
| `loop` | duration, 1s–1h |  | `3600s` | `input.time` runs from 0 to this, then again: make the shader seamless at it. |

## Stop

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `color` | colour | yes |  | The colour at this stop; `transparent` fades out. |
| `alpha` | number, 0–1 |  | `1` | Multiplies the colour's alpha, as `color-mix(in oklab, <color> 22%, transparent)` does. |
| `at` | percentage | yes |  | Where along the gradient line (linear) or radius (radial). |

## Motion

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `type` | `drift`, `pan` | yes |  | `drift` eases out by `by` and back every `period`; `pan` moves steadily by `by` every `period`, for grids and repeating images. |
| `by` | [length, length] | yes |  | How far. |
| `period` | duration, 1s–1h | yes |  | How long one cycle takes. |

## Mask

| Field | Type | Required | Default | |
| --- | --- | --- | --- | --- |
| `at` | [length, length] | yes |  | The ellipse's centre. |
| `size` | [length, length] | yes |  | The ellipse's radii. |
| `from` | percentage | yes |  | Fully shown inside this fraction of the radii. |
| `to` | percentage | yes |  | Gone from this fraction of the radii. |

## Shaders

A `shader` layer's file defines `fn layer(input: Input) -> vec4<f32>`, returning the layer's colour at a pixel: straight alpha, sRGB, 0 to 1. It is given:

```wgsl
struct Input {
    // The pixel's centre, in pixels from the backdrop's top left.
    pixel: vec2<f32>,
    // The backdrop's size in pixels.
    size: vec2<f32>,
    // Seconds into the layer's loop; 0 while motion is reduced.
    time: f32,
    // Pixels per CSS pixel: the display's scale and the zoom.
    scale: f32,
    // The layer's `colors`, straight alpha (unused ones transparent).
    colors: array<vec4<f32>, 4>,
    // The layer's `numbers` (unused ones 0).
    numbers: vec4<f32>,
}
```

It may declare helper functions, structs and constants, but no module-scope `var`, bindings, textures, `override`s or entry points: it can read nothing but its input. Files are at most 64 KiB; a backdrop has at most 2 shader layers. `caskade-theme lint` parses and validates it, and every name it declares is prefixed, so it can't clash with another theme's shader or the app's own code.

A shader runs only when the backdrop moves or changes: the window's frames compose the backdrop as last drawn. The GPU times it where it can, and a shader that takes more than 8 ms a frame, or 40 ms a second of motion, stops moving: it stays on its last frame. Without a usable GPU, and in the webview, the theme's CSS backdrop is drawn instead.
