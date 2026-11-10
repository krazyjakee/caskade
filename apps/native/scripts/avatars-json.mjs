// Prints the webview's shipped avatars (packages/ui/src/avatars.ts) as JSON,
// `{ "<name>": "<data: URL>" }`, computed by avatars.ts's own `hex` and
// `svg`: the native app's `ui::avatar` test checks it draws the same ones.
//
//   node apps/native/scripts/avatars-json.mjs
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const source = readFileSync(path.join(repo, "packages/ui/src/avatars.ts"), "utf8");
const between = (from, to) => source.slice(source.indexOf(from), source.indexOf(to));
// avatars.ts's functions, without their TypeScript annotations.
const code = (
  between("function hex(", "function svg(") + between("function svg(", "/** Every avatar")
)
  .replace(/: number/g, "")
  .replace(/\): string/g, ")")
  .replace("{ hue, glyph }: Drawing", "{ hue, glyph }")
  .replace("tone: string", "tone");
const svg = new Function(`${code}; return svg;`)();
const drawing = /name: "([^"]+)",[\s\S]*?hue: (\d+),\s*glyph: `([^`]*)`/g;
const avatars = {};
for (const [, name, hue, glyph] of source.matchAll(drawing)) {
  avatars[name] = `data:image/svg+xml,${encodeURIComponent(svg({ hue: Number(hue), glyph }))}`;
}
console.log(JSON.stringify(avatars));
