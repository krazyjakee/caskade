import { expect, it } from "vitest";
import { contrast, deriveAccent, MIN_CONTRAST, parseHex, parseSurface } from "./accent";

// The default theme's surfaces: base, and raised composited over base.
const base = parseSurface("#060914");
const raised = base && parseSurface("#0d1224d9", base);
const dark = [base, raised].filter((s): s is NonNullable<typeof s> => s !== null);

it("keeps a readable accent and derives its tones", () => {
  const result = deriveAccent("#5b5ff7", dark);
  expect(result).toMatchObject({ ok: true, accent: "#5b5ff7", fg: "#ffffff" });
  if (!result.ok) throw new Error(result.message);
  expect(result.message).toBeUndefined();
  expect(result.glow).toMatch(/^#[0-9a-f]{6}80$/);
  expect(result.strong).not.toBe(result.accent);
});

it("lightens an accent that would vanish on a dark theme, and says so", () => {
  const result = deriveAccent("#1a1f4a", dark);
  if (!result.ok) throw new Error(result.message);
  expect(result.accent).not.toBe("#1a1f4a");
  expect(result.message).toMatch(/lightened to #[0-9a-f]{6}/);
  const accent = parseHex(result.accent);
  for (const surface of dark)
    expect(contrast(accent ?? [0, 0, 0], surface)).toBeGreaterThanOrEqual(MIN_CONTRAST);
});

it("darkens on a light theme and refuses what can't be made readable", () => {
  const light = deriveAccent("#ffe14d", [parseHex("#ffffff") ?? [0, 0, 0]]);
  if (!light.ok) throw new Error(light.message);
  expect(light.message).toMatch(/darkened/);
  // No colour reaches 3:1 against both of these mid surfaces.
  const refused = deriveAccent("#5b5ff7", [
    parseHex("#3a3a3a") ?? [0, 0, 0],
    parseHex("#c8c8c8") ?? [0, 0, 0],
  ]);
  expect(refused).toMatchObject({ ok: false });
  expect(deriveAccent("orange", dark)).toMatchObject({ ok: false });
});
