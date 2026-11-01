/** The predefined avatars (#203): geometric sci-fi glyphs, drawn here as
 * SVG and bundled as data URLs, so they need no network and stay crisp at
 * any size. Each sits on its own dark tile, so it reads on dark and light
 * themes alike.
 *
 * An id (`avatar:<name>`) names the same picture in every release: change a
 * drawing if you must, never what an id means. The core keeps the same list
 * (`AVATARS` in `crates/caskade-core/src/accounts/mod.rs`) and rejects
 * others; a test keeps the two in step. */
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";

export interface AvatarInfo {
  id: string;
  /** What the picture shows, for screen readers and the picker; translated
   * where it is shown (`t(info.label)`). */
  label: MessageDescriptor;
  /** Its image, a `data:` URL. */
  url: string;
}

/** Tokens in a glyph: `L` the light ink, `M` the mid tone, `D` the tile's dark. */
type Glyph = string;

interface Drawing {
  name: string;
  label: MessageDescriptor;
  /** The tile's hue, in degrees. */
  hue: number;
  glyph: Glyph;
}

const drawings: readonly Drawing[] = [
  {
    name: "default",
    label: msg({
      message: "Caskade",
      context: "avatar",
      comment: "The avatar showing the Caskade logo; usually kept as the brand name",
    }),
    hue: 200,
    glyph: `<circle cx="32" cy="32" r="14"/><path d="M38.5 23.5l4 4-16 16-4-4z" fill="M" stroke="M" stroke-width="1"/>`,
  },
  {
    name: "astronaut",
    label: msg`Astronaut`,
    hue: 210,
    glyph: `<circle cx="32" cy="27" r="14"/><rect x="22" y="20" width="20" height="12" rx="6" fill="M" fill-opacity=".55"/><path d="M27 23.5h5"/><path d="M13 56c2-9 9-14 19-14s17 5 19 14"/><path d="M28 48h8"/>`,
  },
  {
    name: "droid",
    label: msg`Droid`,
    hue: 185,
    glyph: `<rect x="17" y="21" width="30" height="25" rx="6"/><path d="M32 21v-6"/><circle cx="32" cy="12.5" r="2.5" fill="L"/><circle cx="25.5" cy="32" r="3.2" fill="L"/><circle cx="38.5" cy="32" r="3.2" fill="L"/><path d="M26 40h12M12 29v9M52 29v9M24 46v6M40 46v6"/>`,
  },
  {
    name: "rocket",
    label: msg`Rocket`,
    hue: 15,
    glyph: `<path d="M32 9c7 6 10 14 10 24l-3 9H25l-3-9c0-10 3-18 10-24z"/><circle cx="32" cy="26" r="4" fill="M" fill-opacity=".6"/><path d="M22 34l-7 9 9-1M42 34l7 9-9-1"/><path d="M28 47l4 8 4-8" stroke="M"/>`,
  },
  {
    name: "planet",
    label: msg`Ringed planet`,
    hue: 35,
    glyph: `<circle cx="32" cy="32" r="12" fill="M" fill-opacity=".35"/><path d="M22 36c-4 2-6 4-5 6 1.5 3 12 1 22-4s17-11 15.5-14c-1-2-4-2-8-1"/><path d="M42 28c4-2 6-4 5-6-1.5-3-12-1-22 4"/>`,
  },
  {
    name: "moon",
    label: msg`Moon`,
    hue: 240,
    glyph: `<path d="M38 12a20 20 0 1 0 14 30A16 16 0 0 1 38 12z"/><circle cx="27" cy="38" r="3"/><circle cx="21" cy="29" r="1.8" fill="L"/><circle cx="33" cy="46" r="1.6" fill="L"/>`,
  },
  {
    name: "comet",
    label: msg`Comet`,
    hue: 195,
    glyph: `<circle cx="42" cy="22" r="7" fill="L"/><path d="M36.5 27.5L13 51M35 20.5L17 31M43.5 29.5L33 47" /><path d="M24 40l-6 6" stroke="M"/>`,
  },
  {
    name: "atom",
    label: msg`Atom`,
    hue: 165,
    glyph: `<circle cx="32" cy="32" r="3.5" fill="L"/><ellipse cx="32" cy="32" rx="21" ry="8"/><ellipse cx="32" cy="32" rx="21" ry="8" transform="rotate(60 32 32)"/><ellipse cx="32" cy="32" rx="21" ry="8" transform="rotate(-60 32 32)"/>`,
  },
  {
    name: "satellite",
    label: msg`Satellite`,
    hue: 220,
    glyph: `<rect x="26" y="26" width="12" height="12" rx="2" fill="M" fill-opacity=".4"/><rect x="8" y="27" width="14" height="10" rx="1"/><rect x="42" y="27" width="14" height="10" rx="1"/><path d="M22 32h4M38 32h4M15 27v10M49 27v10M32 26v-7M27 15.5a7 7 0 0 1 10 0"/>`,
  },
  {
    name: "ufo",
    label: msg`Flying saucer`,
    hue: 140,
    glyph: `<ellipse cx="32" cy="34" rx="22" ry="7"/><path d="M21 31a11 11 0 0 1 22 0" fill="M" fill-opacity=".4"/><circle cx="23" cy="35" r="1.6" fill="L"/><circle cx="32" cy="36.5" r="1.6" fill="L"/><circle cx="41" cy="35" r="1.6" fill="L"/><path d="M22 46l-4 8M32 45v9M42 46l4 8" stroke="M"/>`,
  },
  {
    name: "alien",
    label: msg`Alien`,
    hue: 110,
    glyph: `<path d="M32 11c11 0 18 8 18 18 0 12-10 23-18 23S14 41 14 29c0-10 7-18 18-18z"/><path d="M19 29c5 0 10 2 10 8-5 0-10-2-10-8zM45 29c-5 0-10 2-10 8 5 0 10-2 10-8z" fill="L"/><path d="M29 45h6"/>`,
  },
  {
    name: "star",
    label: msg`Star`,
    hue: 48,
    glyph: `<path d="M32 11l6.2 12.6 13.8 2-10 9.8 2.4 13.8L32 42.7l-12.4 6.5 2.4-13.8-10-9.8 13.8-2z" fill="M" fill-opacity=".35"/><path d="M32 26v8M28 30h8" stroke="L"/>`,
  },
  {
    name: "galaxy",
    label: msg`Spiral galaxy`,
    hue: 280,
    glyph: `<circle cx="32" cy="32" r="3" fill="L"/><path d="M35 32c0-6-9-7-12-1s2 15 11 14 15-9 12-19-16-14-24-8"/><path d="M29 32c0 6 9 7 12 1"/><circle cx="16" cy="44" r="1.4" fill="L"/><circle cx="50" cy="16" r="1.4" fill="L"/>`,
  },
  {
    name: "pulsar",
    label: msg`Pulsar`,
    hue: 300,
    glyph: `<circle cx="32" cy="32" r="5" fill="L"/><circle cx="32" cy="32" r="12"/><circle cx="32" cy="32" r="20" stroke-dasharray="4 6" stroke="M"/><path d="M32 6v10M32 48v10"/>`,
  },
  {
    name: "crystal",
    label: msg`Crystal`,
    hue: 175,
    glyph: `<path d="M32 9l14 15-14 31-14-31z" fill="M" fill-opacity=".3"/><path d="M18 24h28M26 24l6 31 6-31M26 24l6-15 6 15"/>`,
  },
  {
    name: "radar",
    label: msg`Radar`,
    hue: 130,
    glyph: `<circle cx="32" cy="32" r="21"/><circle cx="32" cy="32" r="12" stroke="M"/><path d="M32 11v42M11 32h42" stroke="M" stroke-width="1.5"/><path d="M32 32L47 17" /><path d="M32 32L47 17a21 21 0 0 1 6 15z" fill="M" fill-opacity=".45" stroke="none"/><circle cx="40" cy="40" r="2.2" fill="L"/>`,
  },
  {
    name: "chip",
    label: msg`Chip`,
    hue: 155,
    glyph: `<rect x="19" y="19" width="26" height="26" rx="3"/><rect x="27" y="27" width="10" height="10" rx="1" fill="L"/><path d="M26 19v-7M32 19v-7M38 19v-7M26 45v7M32 45v7M38 45v7M19 26h-7M19 32h-7M19 38h-7M45 26h7M45 32h7M45 38h7"/>`,
  },
  {
    name: "ghost",
    label: msg`Ghost`,
    hue: 260,
    glyph: `<path d="M18 52V30a14 14 0 0 1 28 0v22l-5-4-4.5 4-4.5-4-4.5 4-4.5-4z"/><circle cx="26.5" cy="31" r="2.8" fill="L"/><circle cx="37.5" cy="31" r="2.8" fill="L"/>`,
  },
  {
    name: "mech",
    label: msg`Mech helmet`,
    hue: 0,
    glyph: `<path d="M15 41V28a17 17 0 0 1 34 0v13l-7 9H22z"/><path d="M20 29h24l-3 8H23z" fill="L"/><path d="M32 11v7M24 50l2-6h12l2 6"/>`,
  },
  {
    name: "bolt",
    label: msg`Energy bolt`,
    hue: 55,
    glyph: `<path d="M36 8L17 36h13l-4 20 21-30H34z" fill="M" fill-opacity=".4"/>`,
  },
  {
    name: "sentinel",
    label: msg`Sentinel eye`,
    hue: 345,
    glyph: `<path d="M9 32c6-10 14-15 23-15s17 5 23 15c-6 10-14 15-23 15S15 42 9 32z"/><circle cx="32" cy="32" r="8"/><circle cx="32" cy="32" r="3" fill="L"/>`,
  },
  {
    name: "starfighter",
    label: msg`Starfighter`,
    hue: 225,
    glyph: `<path d="M32 9l6 22 15 11v6l-15-4-2 8h-8l-2-8-15 4v-6l15-11z" fill="M" fill-opacity=".3"/><path d="M32 22v14" stroke="L"/>`,
  },
  {
    name: "portal",
    label: msg`Portal`,
    hue: 270,
    glyph: `<circle cx="32" cy="32" r="21"/><ellipse cx="33" cy="33" rx="14" ry="16"/><ellipse cx="34" cy="34" rx="8" ry="10" stroke="M"/><ellipse cx="35" cy="35" rx="3" ry="4.5" fill="L"/>`,
  },
  {
    name: "cybercat",
    label: msg`Cyber cat`,
    hue: 320,
    glyph: `<path d="M18 27l-2-14 11 8h10l11-8-2 14c3 4 4 8 4 12 0 9-7 14-18 14S14 48 14 39c0-4 1-8 4-12z"/><path d="M21 34h9l-1 4h-7zM43 34h-9l1 4h7z" fill="L"/><path d="M29 44l3 2 3-2M10 40h8M46 40h8"/>`,
  },
  {
    name: "gamepad",
    label: msg`Gamepad`,
    hue: 190,
    glyph: `<path d="M20 22h24c6 0 10 6 11 14s0 14-5 14c-4 0-6-6-10-6H24c-4 0-6 6-10 6-5 0-6-6-5-14s5-14 11-14z"/><path d="M22 30v9M17.5 34.5h9"/><circle cx="41" cy="31" r="2.2" fill="L"/><circle cx="46" cy="36.5" r="2.2" fill="L"/>`,
  },
];

/** An HSL colour as hex: SVG attributes take it everywhere. */
function hex(h: number, s: number, l: number): string {
  const sat = s / 100;
  const light = l / 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = sat * Math.min(light, 1 - light);
  const channel = (n: number) => {
    const value = light - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
    return Math.round(value * 255)
      .toString(16)
      .padStart(2, "0");
  };
  return `#${channel(0)}${channel(8)}${channel(4)}`;
}

function svg({ hue, glyph }: Drawing): string {
  const light = hex(hue, 95, 86);
  const mid = hex(hue, 90, 62);
  const dark = hex(hue, 70, 9);
  const ink = glyph.replace(/"(L|M|D)"/g, (_, tone: string) =>
    tone === "L" ? `"${light}"` : tone === "M" ? `"${mid}"` : `"${dark}"`,
  );
  return [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">`,
    `<defs><linearGradient id="t" x1="0" y1="0" x2="1" y2="1">`,
    `<stop offset="0" stop-color="${hex(hue, 65, 27)}"/><stop offset="1" stop-color="${dark}"/>`,
    `</linearGradient><radialGradient id="g" cx=".5" cy=".38" r=".55">`,
    `<stop offset="0" stop-color="${mid}" stop-opacity=".5"/><stop offset="1" stop-color="${mid}" stop-opacity="0"/>`,
    `</radialGradient></defs>`,
    `<rect width="64" height="64" fill="url(#t)"/><rect width="64" height="64" fill="url(#g)"/>`,
    `<path d="M0 16H64M0 48H64M16 0V64M48 0V64" stroke="${light}" stroke-opacity=".07"/>`,
    `<g fill="none" stroke="${light}" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">${ink}</g>`,
    `</svg>`,
  ].join("");
}

/** Every avatar, in the picker's order; the first is the default. */
export const avatars: readonly AvatarInfo[] = drawings.map((drawing) => ({
  id: `avatar:${drawing.name}`,
  label: drawing.label,
  url: `data:image/svg+xml,${encodeURIComponent(svg(drawing))}`,
}));

export const DEFAULT_AVATAR = "avatar:default";

const byId = new Map(avatars.map((avatar) => [avatar.id, avatar]));

/** The avatar an id names, or undefined for one this release doesn't ship. */
export function avatarInfo(id: string | null | undefined): AvatarInfo | undefined {
  return id ? byId.get(id) : undefined;
}
