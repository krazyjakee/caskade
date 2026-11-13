//! A theme's own backdrop shader (`"type": "shader"` layers, #149): one
//! WGSL file under `assets/` that defines
//!
//! ```wgsl
//! fn layer(input: Input) -> vec4<f32>
//! ```
//!
//! returning the layer's colour at a pixel (straight alpha, sRGB, 0..1).
//! [`INPUT`] declares what it gets. It is a pure function: no bindings,
//! textures, storage or entry points, so it can read nothing but its input.
//!
//! [`compile`] parses and validates it with naga, checks those rules and
//! rewrites it with every name it declares prefixed, so several themes'
//! shaders and the app's own code live in one module without clashing.
//! The renderer compiles the result into its backdrop pass.

use crate::Problem;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use std::sync::Arc;

/// Largest shader file.
pub const MAX_SOURCE: usize = 64 * 1024;

/// What a shader's `layer` function is given. Declared for it: a shader
/// file only uses it.
pub const INPUT: &str = "struct Input {
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
";

/// The name [`INPUT`] has in a compiled shader, which the renderer declares.
pub const COMPILED_INPUT: &str = "CsInput";

/// A shader ready for the renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    /// Every name it declares prefixed, without the input's declaration.
    pub wgsl: Arc<str>,
    /// Its `layer` function's name in `wgsl`.
    pub entry: String,
    /// Identifies it: the same source compiles to the same prefix.
    pub id: u64,
}

fn hash(text: &str) -> u64 {
    // FNV-1a: stable across builds, unlike std's hasher.
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Checks `source` (the file `file`) and compiles it for the renderer.
pub fn compile(file: &str, source: &str) -> Result<Compiled, Vec<Problem>> {
    if source.len() > MAX_SOURCE {
        return Err(vec![Problem::new(file, "is larger than 64 KiB")]);
    }
    let prelude_lines = INPUT.lines().count() as u32;
    let full = format!("{INPUT}{source}");
    let mut module = naga::front::wgsl::parse_str(&full).map_err(|error| {
        let message = error.message().to_owned();
        vec![match error.location(&full) {
            Some(at) if at.line_number > prelude_lines => Problem::at(
                file,
                at.line_number - prelude_lines,
                at.line_position,
                message,
            ),
            _ => Problem::new(file, message),
        }]
    })?;
    let mut problems = Vec::new();
    let mut fail = |message: &str| problems.push(Problem::new(file, message));
    if !module.entry_points.is_empty() {
        fail(
            "a backdrop shader has no entry points: it defines fn layer(input: Input) -> vec4<f32>",
        );
    }
    if !module.global_variables.is_empty() {
        fail("a backdrop shader declares no var at module scope: no bindings, textures or storage");
    }
    if !module.overrides.is_empty() {
        fail("a backdrop shader declares no override constants");
    }
    let input = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some("Input"))
        .map(|(handle, _)| handle);
    let layer = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("layer"));
    match layer {
        None => fail("defines no fn layer(input: Input) -> vec4<f32>"),
        Some((_, function)) => {
            let takes_input =
                function.arguments.len() == 1 && Some(function.arguments[0].ty) == input;
            let gives_color = function.result.as_ref().is_some_and(|result| {
                matches!(
                    module.types[result.ty].inner,
                    naga::TypeInner::Vector {
                        size: naga::VectorSize::Quad,
                        scalar: naga::Scalar::F32
                    }
                )
            });
            if !takes_input || !gives_color {
                fail("layer must be fn layer(input: Input) -> vec4<f32>");
            }
        }
    }
    if let Err(error) =
        Validator::new(ValidationFlags::all(), Capabilities::empty()).validate(&module)
    {
        fail(&format!("isn't valid WGSL: {}", error.into_inner()));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let id = hash(source);
    let prefix = format!("cs_t{id:016x}_");
    // Types are interned: each named one is replaced by its renamed copy.
    let named: Vec<(naga::Handle<naga::Type>, naga::Type)> = module
        .types
        .iter()
        .filter(|(_, ty)| ty.name.is_some())
        .map(|(handle, ty)| (handle, ty.clone()))
        .collect();
    for (handle, mut ty) in named {
        ty.name = Some(if Some(handle) == input {
            COMPILED_INPUT.to_owned()
        } else {
            format!("{prefix}{}", ty.name.unwrap_or_default())
        });
        module.types.replace(handle, ty);
    }
    for (_, constant) in module.constants.iter_mut() {
        if let Some(name) = &mut constant.name {
            *name = format!("{prefix}{name}");
        }
    }
    for (_, function) in module.functions.iter_mut() {
        if let Some(name) = &mut function.name {
            *name = format!("{prefix}{name}");
        }
    }
    let renamed = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .map_err(|error| {
            vec![Problem::new(
                file,
                format!("can't be compiled: {}", error.into_inner()),
            )]
        })?;
    let written =
        naga::back::wgsl::write_string(&module, &renamed, naga::back::wgsl::WriterFlags::empty())
            .map_err(|error| vec![Problem::new(file, format!("can't be compiled: {error}"))])?;
    let entry = format!("{prefix}layer");
    let wgsl = strip_struct(&written, COMPILED_INPUT);
    if !wgsl.contains(&format!("fn {entry}(")) {
        return Err(vec![Problem::new(
            file,
            "can't be compiled: its layer function was renamed",
        )]);
    }
    Ok(Compiled {
        wgsl: wgsl.into(),
        entry,
        id,
    })
}

/// `wgsl` without `struct <name> { … }`, which the renderer declares once.
fn strip_struct(wgsl: &str, name: &str) -> String {
    let start = format!("struct {name} {{");
    let Some(at) = wgsl.find(&start) else {
        return wgsl.to_owned();
    };
    let end = wgsl[at..]
        .find("\n}")
        .map_or(wgsl.len(), |end| at + end + 2);
    format!("{}{}", &wgsl[..at], &wgsl[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const AURORA: &str = "
const BANDS: f32 = 3.0;
struct Wave { phase: f32 }
fn noise(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453); }
fn layer(input: Input) -> vec4<f32> {
    let uv = input.pixel / input.size;
    let wave = Wave(sin(uv.x * BANDS + input.time));
    let glow = input.colors[0] * (0.5 + 0.5 * wave.phase) * input.numbers.x;
    return vec4<f32>(glow.rgb, glow.a * noise(uv));
}
";

    /// The renderer's declaration of the input, as it declares it.
    fn renderer_input() -> String {
        INPUT.replace("struct Input", &format!("struct {COMPILED_INPUT}"))
    }

    #[test]
    fn a_shader_compiles_namespaced_beside_another_that_reuses_its_names() {
        let aurora = compile("assets/aurora.wgsl", AURORA).unwrap();
        let other = compile(
            "assets/other.wgsl",
            "fn noise(p: vec2<f32>) -> f32 { return p.x; }\nfn layer(input: Input) -> vec4<f32> { return vec4<f32>(noise(input.pixel)); }\n",
        )
        .unwrap();
        assert_ne!(aurora.id, other.id);
        assert!(aurora.entry.starts_with("cs_t") && aurora.entry.ends_with("_layer"));
        assert!(!aurora.wgsl.contains("struct CsInput"));
        assert!(!aurora.wgsl.contains("struct Input"));
        // Both, and the app's own `layer`, in one module.
        let module = format!(
            "{}{}{}fn layer(i: {COMPILED_INPUT}) -> vec4<f32> {{ return {}(i) + {}(i); }}\n",
            renderer_input(),
            aurora.wgsl,
            other.wgsl,
            aurora.entry,
            other.entry
        );
        let parsed = naga::front::wgsl::parse_str(&module)
            .unwrap_or_else(|error| panic!("{}\n{module}", error.emit_to_string(&module)));
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&parsed)
            .unwrap();
        // The same source, the same names.
        assert_eq!(compile("assets/again.wgsl", AURORA).unwrap(), aurora);
    }

    #[test]
    fn a_shader_that_could_reach_beyond_its_input_is_refused() {
        let problems = |source: &str| -> String {
            compile("assets/x.wgsl", source)
                .unwrap_err()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        };
        let cases = [
            (
                "@group(0) @binding(0) var t: texture_2d<f32>;\nfn layer(input: Input) -> vec4<f32> { return vec4<f32>(0.0); }",
                "no var at module scope",
            ),
            (
                "var<private> seed: f32;\nfn layer(input: Input) -> vec4<f32> { return vec4<f32>(seed); }",
                "no var at module scope",
            ),
            (
                "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }\nfn layer(input: Input) -> vec4<f32> { return vec4<f32>(0.0); }",
                "no entry points",
            ),
            (
                "override k: f32 = 1.0;\nfn layer(input: Input) -> vec4<f32> { return vec4<f32>(k); }",
                "no override",
            ),
            (
                "fn layer(p: vec2<f32>) -> vec4<f32> { return vec4<f32>(p, 0.0, 1.0); }",
                "layer must be fn layer",
            ),
            (
                "fn layer(input: Input) -> vec3<f32> { return vec3<f32>(0.0); }",
                "layer must be fn layer",
            ),
            (
                "fn other(input: Input) -> vec4<f32> { return vec4<f32>(0.0); }",
                "defines no fn layer",
            ),
        ];
        for (source, expected) in cases {
            let found = problems(source);
            assert!(
                found.contains(expected),
                "{source}\n  expected {expected}\n  got {found}"
            );
        }
        // Syntax errors point into the file, not the input's declaration.
        let found = problems("\n\nfn layer(input: Input) -> vec4<f32> { return vec4<f32>(0.0) }\n");
        assert!(found.starts_with("assets/x.wgsl:3:"), "{found}");
        assert!(problems(&"// x\n".repeat(20_000)).contains("larger than 64 KiB"));
    }
}
