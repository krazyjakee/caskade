// The backdrop and the cached scene, composed in one pass straight onto the
// surface (fs_main); or, with an embedder's shaders, the backdrop into a
// texture of its own when it moves (fs_layers) and that texture and the
// scene composed (fs_compose). Colours are premultiplied and gamma-encoded
// sRGB, as Vello Hybrid's scene and CSS gradients interpolate them. Lengths
// are surface pixels.
//
// The embedder's shaders and `cs_theme_shader`, which calls them, are added
// where CS_THEME_SHADERS stands; `CsInput` is declared with them.

struct Header {
    // The surface's size, and the area the layers are clipped to.
    area: vec4<f32>,
    base: vec4<f32>,
    // x: the layer count, y: 1 to un-premultiply the output.
    counts: vec4<u32>,
    // x: the backdrop's opacity.
    opacity: vec4<f32>,
    // Each layer's motion offset now: xy for layers 2n, zw for 2n + 1.
    offsets: array<vec4<f32>, 4>,
}

struct Layer {
    // x: kind (0 linear, 1 radial, 2 grid, 3 image, 4 shader), y: blend
    // (0 normal, 1 multiply, 2 screen), z: image or shader slot, w: stop
    // count.
    kind: vec4<u32>,
    // Linear: centre, direction. Radial: centre, radius. Grid: cell, line,
    // repeat. Image: origin, size. Shader: the area's origin and size.
    a: vec4<f32>,
    // Linear: gradient length. Grid: origin. Image: x repeat. Shader: x
    // scale.
    b: vec4<f32>,
    // Mask: centre, radius.
    mask: vec4<f32>,
    // Mask from, to, whether there's a mask; the layer's opacity.
    mask_range: vec4<f32>,
    // Stop offsets; a shader's numbers.
    offsets: vec4<f32>,
    // Premultiplied stop colours; a shader's colours, straight.
    colors: array<vec4<f32>, 4>,
}

@group(0) @binding(0) var<uniform> header: Header;
@group(0) @binding(1) var<uniform> layers: array<Layer, 8>;
@group(0) @binding(2) var scene: texture_2d<f32>;
@group(0) @binding(3) var image_sampler: sampler;
@group(0) @binding(4) var image0: texture_2d<f32>;
@group(0) @binding(5) var image1: texture_2d<f32>;
@group(0) @binding(6) var image2: texture_2d<f32>;
@group(0) @binding(7) var image3: texture_2d<f32>;
// The backdrop as fs_layers last drew it.
@group(0) @binding(8) var cached: texture_2d<f32>;

// CS_THEME_SHADERS

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole surface.
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn offset(i: u32) -> vec2<f32> {
    let pair = header.offsets[i / 2u];
    if (i % 2u) == 0u {
        return pair.xy;
    }
    return pair.zw;
}

fn gradient(i: u32, t: f32) -> vec4<f32> {
    let count = layers[i].kind.w;
    if count == 0u {
        return vec4<f32>(0.0);
    }
    if t <= layers[i].offsets[0] {
        return layers[i].colors[0];
    }
    for (var k = 1u; k < count; k++) {
        let end = layers[i].offsets[k];
        if t <= end {
            let start = layers[i].offsets[k - 1u];
            var f = 1.0;
            if end > start {
                f = (t - start) / (end - start);
            }
            return mix(layers[i].colors[k - 1u], layers[i].colors[k], f);
        }
    }
    return layers[i].colors[count - 1u];
}

// How much of the pixel starting at `edge` (in a cell `cell` long) lines
// `line` wide at the start of each cell cover.
fn line_cover(edge: f32, cell: f32, line: f32) -> f32 {
    let m = edge - floor(edge / cell) * cell;
    let first = max(0.0, min(m + 1.0, line) - max(m, 0.0));
    let next = max(0.0, min(m + 1.0, cell + line) - max(m, cell));
    return clamp(first + next, 0.0, 1.0);
}

fn image(slot: u32, uv: vec2<f32>) -> vec4<f32> {
    switch slot {
        case 0u: { return textureSampleLevel(image0, image_sampler, uv, 0.0); }
        case 1u: { return textureSampleLevel(image1, image_sampler, uv, 0.0); }
        case 2u: { return textureSampleLevel(image2, image_sampler, uv, 0.0); }
        default: { return textureSampleLevel(image3, image_sampler, uv, 0.0); }
    }
}

fn layer(i: u32, pixel: vec2<f32>) -> vec4<f32> {
    let p = pixel - offset(i);
    let a = layers[i].a;
    var color = vec4<f32>(0.0);
    switch layers[i].kind.x {
        case 0u: {
            let t = dot(p - a.xy, a.zw) / layers[i].b.x + 0.5;
            color = gradient(i, t);
        }
        case 1u: {
            let t = length((p - a.xy) / max(a.zw, vec2<f32>(1e-3)));
            color = gradient(i, t);
        }
        case 2u: {
            // The pixel's left/top edge, relative to where a cell starts.
            let edge = p - vec2<f32>(0.5) - layers[i].b.xy;
            let x = line_cover(edge.x, a.x, a.z);
            let y = line_cover(edge.y, a.y, a.z);
            color = layers[i].colors[0] * (1.0 - (1.0 - x) * (1.0 - y));
        }
        case 3u: {
            let uv = (p - a.xy) / max(a.zw, vec2<f32>(1e-3));
            let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0));
            let sampled = image(layers[i].kind.z, uv);
            if layers[i].b.x > 0.5 || inside {
                color = sampled;
            }
        }
        case 4u: {
            var input: CsInput;
            input.pixel = pixel - a.xy;
            input.size = a.zw;
            input.time = offset(i).x;
            input.scale = layers[i].b.x;
            input.colors = layers[i].colors;
            input.numbers = layers[i].offsets;
            let straight = clamp(cs_theme_shader(layers[i].kind.z, input), vec4<f32>(0.0), vec4<f32>(1.0));
            color = vec4<f32>(straight.rgb * straight.a, straight.a);
        }
        default: {}
    }
    var cover = layers[i].mask_range.w;
    if layers[i].mask_range.z > 0.5 {
        let mask = layers[i].mask;
        let d = length((pixel - mask.xy) / max(mask.zw, vec2<f32>(1e-3)));
        let inner = layers[i].mask_range.x;
        let outer = layers[i].mask_range.y;
        cover *= clamp((outer - d) / max(outer - inner, 1e-4), 0.0, 1.0);
    }
    return color * cover;
}

fn blend(source: vec4<f32>, under: vec4<f32>, mode: u32) -> vec4<f32> {
    let alpha = source.a + under.a * (1.0 - source.a);
    switch mode {
        case 1u: {
            let rgb = source.rgb * under.rgb + source.rgb * (1.0 - under.a)
                + under.rgb * (1.0 - source.a);
            return vec4<f32>(rgb, alpha);
        }
        case 2u: {
            let rgb = source.rgb + under.rgb - source.rgb * under.rgb;
            return vec4<f32>(rgb, alpha);
        }
        default: {
            return source + under * (1.0 - source.a);
        }
    }
}

fn backdrop(pixel: vec2<f32>) -> vec4<f32> {
    var color = header.base;
    let area = header.area;
    let inside = pixel.x >= area.x && pixel.y >= area.y && pixel.x < area.z && pixel.y < area.w;
    if inside && header.opacity.x > 0.0 {
        for (var i = 0u; i < header.counts.x; i++) {
            color = blend(layer(i, pixel) * header.opacity.x, color, layers[i].kind.y);
        }
    }
    return color;
}

fn compose(pixel: vec2<f32>, under: vec4<f32>) -> vec4<f32> {
    let foreground = textureLoad(scene, vec2<i32>(pixel), 0);
    var out = foreground + under * (1.0 - foreground.a);
    if header.counts.y == 1u && out.a > 0.0 {
        out = vec4<f32>(out.rgb / out.a, out.a);
    }
    return out;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return compose(position.xy, backdrop(position.xy));
}

@fragment
fn fs_layers(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return backdrop(position.xy);
}

@fragment
fn fs_compose(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return compose(position.xy, textureLoad(cached, vec2<i32>(position.xy), 0));
}
