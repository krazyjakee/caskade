/** A user accent (Appearance → Accent, #141): its derived tones, kept
 * readable against the active theme's surfaces. Pure; colours are #rrggbb. */

import { t } from "@lingui/core/macro";

type Rgb = [number, number, number];

export function parseHex(text: string): Rgb | null {
  const match = /^#?([0-9a-f]{6})$/i.exec(text.trim());
  if (!match?.[1]) return null;
  const value = Number.parseInt(match[1], 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

/** `#rrggbb` or `#rrggbbaa` composited over `under` (for translucent surfaces). */
export function parseSurface(text: string, under: Rgb = [0, 0, 0]): Rgb | null {
  const match = /^#?([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(text.trim());
  if (!match?.[1]) return null;
  const base = parseHex(`#${match[1]}`) as Rgb;
  const alpha = match[2] ? Number.parseInt(match[2], 16) / 255 : 1;
  return base.map((channel, i) =>
    Math.round(channel * alpha + (under[i] ?? 0) * (1 - alpha)),
  ) as Rgb;
}

const hex = (rgb: Rgb) =>
  `#${rgb
    .map((channel) =>
      Math.round(Math.min(255, Math.max(0, channel)))
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;

const linear = (channel: number) => {
  const c = channel / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};
const gamma = (c: number) =>
  255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.max(c, 0) ** (1 / 2.4) - 0.055);

/** WCAG 2 contrast ratio. */
export function contrast(a: Rgb, b: Rgb): number {
  const luminance = ([r, g, b2]: Rgb) =>
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b2);
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (high + 0.05) / (low + 0.05);
}

type Oklch = { l: number; c: number; h: number };

function toOklch([r, g, b]: Rgb): Oklch {
  const [lr, lg, lb] = [linear(r), linear(g), linear(b)];
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return { l: L, c: Math.hypot(A, B), h: Math.atan2(B, A) };
}

function fromOklch({ l: L, c, h }: Oklch): Rgb {
  const A = c * Math.cos(h);
  const B = c * Math.sin(h);
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  return [
    gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    gamma(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
  ];
}

/** Non-text UI (selection, rails, primary buttons) against its surface. */
export const MIN_CONTRAST = 3;

export type AccentResult =
  | {
      ok: true;
      /** What to apply: `--cs-color-accent`, `-strong`, `-fg` and `--cs-color-glow`. */
      accent: string;
      strong: string;
      fg: string;
      glow: string;
      /** Set when the colour had to change to stay readable. */
      message?: string;
    }
  | { ok: false; message: string };

/**
 * The tones for `chosen` on surfaces `surfaces` (the theme's base and raised
 * colours, opaque). An accent below 3:1 against any surface is moved in
 * OKLCH lightness, away from the surfaces, until it passes; one that can't
 * pass is refused.
 */
export function deriveAccent(chosen: string, surfaces: Rgb[]): AccentResult {
  const rgb = parseHex(chosen);
  if (!rgb) return { ok: false, message: t`${{ colour: chosen }} is not a #rrggbb colour` };
  if (surfaces.length === 0) return { ok: false, message: t`The theme's surfaces are unknown` };
  const worst = (colour: Rgb) => Math.min(...surfaces.map((surface) => contrast(colour, surface)));
  const lch = toOklch(rgb);
  const darkTheme = surfaces.every((surface) => toOklch(surface).l < 0.5);
  let accent = rgb;
  let adjusted = false;
  for (let step = 0; worst(accent) < MIN_CONTRAST; step++) {
    const l = lch.l + (darkTheme ? 0.02 : -0.02) * (step + 1);
    if (l <= 0 || l >= 1) return refuse(chosen);
    accent = fromOklch({ ...lch, l });
    adjusted = true;
  }
  const tone = toOklch(accent);
  // Like the default accent: a lighter, slightly shifted "strong" tone and a
  // translucent glow.
  const strong = fromOklch({
    l: Math.min(0.95, tone.l + (darkTheme ? 0.06 : -0.06)),
    c: tone.c,
    h: tone.h + 0.12,
  });
  const fg = (["#ffffff", "#0b0e1a"] as const)
    .map((candidate) => ({ candidate, ratio: contrast(parseHex(candidate) as Rgb, accent) }))
    .sort((a, b) => b.ratio - a.ratio)[0];
  // Button labels on the accent: the better of white and near-black.
  if (!fg || fg.ratio < MIN_CONTRAST) return refuse(chosen);
  const accentHex = hex(accent);
  return {
    ok: true,
    accent: accentHex,
    strong: hex(strong),
    fg: fg.candidate,
    glow: `${hex(fromOklch({ ...tone, l: Math.min(0.95, tone.l + 0.04) }))}80`,
    ...(adjusted && {
      message: darkTheme
        ? t`${{ colour: chosen }} was lightened to ${{ accent: accentHex }} to stay readable on this theme.`
        : t`${{ colour: chosen }} was darkened to ${{ accent: accentHex }} to stay readable on this theme.`,
    }),
  };
}

const refuse = (chosen: string): AccentResult => ({
  ok: false,
  message: t`${{ colour: chosen }} can't be made readable on this theme's surfaces; pick another colour.`,
});
