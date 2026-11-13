# GPU backdrop proposal

**Implemented in #149**: [the theming contract](../theming/README.md#backdrops-149)
and [backdrop.md](../theming/backdrop.md) describe what shipped, and the
tech spec's §6 and §9 how it works and what it costs. Themes may also ship
their own shaders, which this proposal left out.

Proposed direction for [#148](https://github.com/krazyjakee/caskade/issues/148),
following the [hardware measurements](native-gpu-2026-11-13.md). This records
requirements; it does not introduce a supported theme field or token.

The background must animate without CPU rasterization or vector preprocessing.
Background-only frames should update GPU parameters and composite cached UI;
they should not rerun Dioxus, CSS resolution, layout or foreground painting.
CPU scheduling and command submission remain necessary.

## Theming is part of the architecture

The existing [theming contract](../theming/README.md) continues to apply:
themes provide declarative data, tokens, CSS and local assets, never executable
code. The renderer provides GPU drawing primitives. Each theme controls the
background composition, including choosing an image, a flat background or no
decoration. The Nexus grid and nebula are one composition.

- Resolve colours, opacity, dimensions, effect strengths and motion controls
  from the effective theme and user overrides before updating GPU uniforms.
  Shader code must not embed Nexus colours or bypass the corrected user accent.
- Support a documented set of composable background layers, such as images,
  gradients, grids and glows, with bounded transforms and animation parameters.
  The exact schema and primitives need a separate design and validation pass.
  New tokens must be declared in the token contract and generated normally.
- Preserve `[data-cs="backdrop"]` as the public styling anchor. Hiding the
  backdrop must stop its drawing and scheduling. Specify how CSS/custom-CSS
  appearance overrides map to GPU parameters; a shader does not automatically
  implement arbitrary CSS backgrounds or animations.
- Define compatibility for existing theme CSS before switching the default.
  Where translation is unsupported, use an explicitly documented static
  representation of that theme, with a migration path to declarative GPU
  layers. No recurring CPU background-rendering fallback.
- Preserve theme inheritance and the existing cascade: defaults, inherited
  theme, selected theme, then user overrides. A theme's custom assets must
  remain local and follow the existing validation and asset-serving rules.
- Apply live theme, colour-scheme, accent and custom-CSS changes by invalidating
  the relevant caches and updating GPU resources. Resolve styles when their
  inputs change; do not poll computed CSS on every background frame.
- Honour reduced motion, Reduce effects, reduced transparency and UI scale.
  Freeze motion without changing the chosen theme's visual identity. Stop
  submitting ambient frames while hidden. Without a usable GPU, retain a
  static representation of the selected theme and keep animation stopped.

## Rendering direction

Use an app-owned GPU shader/pass for the background and retain the foreground
UI in a transparent GPU texture. A background-only tick redraws/composites
those GPU layers. Foreground input, data or appearance changes invalidate the
UI cache as needed. Putting a custom shader inside the current unconditional
document resolve/paint loop would leave much of the measured CPU work.

Theme data and GPU execution should be separate: supporting additional
background primitives extends the rendering vocabulary without requiring
theme authors to write WGSL or change application Rust.

## Acceptance

- Exercise Default, Cobalt, Emerald, Engage and a custom theme with a different
  background composition; include flat/no-decoration and local-image cases.
- Switch themes, accents, modes and custom CSS live; check inheritance,
  effective colours, visibility and cache invalidation.
- Verify reduced motion/effects, resizing, UI scale, hidden windows and GPU
  unavailability, with no ambient CPU-rendering loop.
- Measure background-only ticks separately: no document resolve or UI scene
  reconstruction, no CPU rasterization/preprocessing, bounded submission cost,
  and GPU time reported independently from presentation.
- Publish any new theme schema, token and anatomy contracts with their
  generators, validation, compatibility behaviour and author documentation.
