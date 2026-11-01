// #141: "reduce effects", reduced motion, density and UI scale are token sets
// selected by root attributes, and every blur, glow and animation in the app
// and @caskade/ui reads those tokens, so turning them down really does.
import { expect, it } from "vitest";
import { readText } from "../../../../packages/create-plugin/testing.mjs";

// Read from disk: Vitest's CSS handling empties `?raw` CSS imports.
const file = (relative: string) => readText("/", new URL(relative, import.meta.url).pathname);
const tokens = file("../../../../packages/tokens/tokens.css");
const effects = file("../../../../themes/default/theme.css");
const index = file("../styles/index.css");

const sources = {
  ...import.meta.glob("../**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }),
  ...import.meta.glob("../../../../packages/ui/src/**/*.{ts,tsx}", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
} as Record<string, string>;

/** The declarations of the block opened by `selector` in `css`. */
function block(css: string, selector: string) {
  const at = css.indexOf(`${selector} {`);
  expect(at, selector).toBeGreaterThanOrEqual(0);
  return css.slice(at, css.indexOf("}", at));
}

it("reduced effects and reduced motion are token sets on root attributes", () => {
  const reducedEffects = block(tokens, '[data-effects="reduced"]');
  for (const declaration of [
    "--cs-effect-glass-blur: 0px",
    "--cs-effect-grid-opacity: 0",
    "--cs-effect-scanline-opacity: 0",
    "--cs-effect-glow-strength: 0",
  ])
    expect(reducedEffects).toContain(declaration);
  const reducedMotion = block(tokens, '[data-motion="reduced"]');
  for (const declaration of [
    "--cs-motion-duration-fast: 0ms",
    "--cs-motion-duration-base: 0ms",
    "--cs-motion-duration-slow: 0ms",
    "--cs-motion-stagger: 0ms",
    "--cs-motion-ambient: paused",
  ])
    expect(reducedMotion).toContain(declaration);
  // The OS preference applies unless Motion is set to Full.
  expect(tokens).toMatch(
    /@media \(prefers-reduced-motion: reduce\) \{\s*:root:not\(\[data-motion="full"\]\) \{/,
  );
  expect(block(tokens, '[data-density="compact"]')).toContain("--cs-density-scale: 0.85");
  expect(tokens).toContain("--cs-ui-scale: 1;");
});

it("Tailwind's blur, transition, spacing and root size follow the tokens", () => {
  for (const size of ["xs", "sm", "md", "lg", "xl", "2xl", "3xl"])
    expect(index).toMatch(
      new RegExp(`--blur-${size}: min\\(\\d+px, var\\(--cs-effect-glass-blur\\)\\);`),
    );
  expect(index).toContain("--blur: min(8px, var(--cs-effect-glass-blur));");
  expect(index).toContain("--default-transition-duration: var(--cs-motion-duration-fast);");
  expect(index).toContain("--spacing: calc(0.25rem * var(--cs-density-scale));");
  expect(index).toContain("font-size: calc(100% * var(--cs-ui-scale) * var(--cs-type-scale));");
});

it("every effect reads the tokens: loops pause, one-shots use motion durations, glows scale", () => {
  const rules = effects.split("}").map((rule) => rule.trim());
  expect(rules.filter((rule) => /\banimation:/.test(rule)).length).toBeGreaterThan(3);
  for (const rule of rules.filter((rule) => /\banimation:/.test(rule))) {
    const loops = /\binfinite\b/.test(rule);
    if (loops) expect(rule, rule).toContain("animation-play-state: var(--cs-motion-ambient)");
    else expect(rule, rule).toMatch(/animation: \S+ (?:calc\()?var\(--cs-motion-duration-/);
  }
  for (const rule of rules.filter((rule) =>
    /(?:drop-shadow|box-shadow)[^;]*--cs-color-(glow|accent-alt)\)?/.test(rule),
  ))
    expect(rule, rule).toContain("--cs-effect-glow-strength");
  expect(effects).not.toMatch(/\bblur\(\s*\d/);
  for (const rule of rules.filter((rule) => /transition(?:-duration)?:/.test(rule)))
    expect(rule, rule).toMatch(/var\(--cs-motion-duration-/);
});

it("no component hard-codes a duration, blur or animation", () => {
  const literal =
    /\b(?:duration|delay)-\d|backdrop-blur-\[\d|\bblur-\[\d|\banimate-(?:spin|ping|pulse|bounce)\b|\banimate-\[/;
  expect(Object.keys(sources).length).toBeGreaterThan(50);
  expect(Object.values(sources).every((text) => text.length > 0)).toBe(true);
  const offenders = Object.entries(sources)
    .filter(([path]) => !path.endsWith(".test.ts") && !path.endsWith(".test.tsx"))
    .filter(([, text]) => literal.test(text))
    .map(([path]) => path);
  expect(offenders).toEqual([]);
});
