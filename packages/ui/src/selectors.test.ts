// @vitest-environment node
/** The selector lint (#138): the package's and the app's stylesheets style
 * components through the anatomy catalogue only, so a theme never has to
 * target a class name. */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import { type AnatomyEntry, anatomy } from "./anatomy";

const dir = (path: string) => fileURLToPath(new URL(path, import.meta.url));

function cssFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() && entry.name !== "node_modules"
      ? cssFiles(join(dir, entry.name))
      : entry.name.endsWith(".css")
        ? [join(dir, entry.name)]
        : [],
  );
}

/** Rule selectors, skipping at-rules whose blocks hold no selectors
 * (`@keyframes`, `@utility`, `@theme`, `@font-face`). */
function selectors(css: string): string[] {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const found: string[] = [];
  const opaque = /^@(keyframes|utility|theme|font-face|property)\b/;
  let start = 0;
  let depth = 0;
  let skipDepth = -1;
  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (char === ";" && depth >= 0) start = i + 1;
    else if (char === "{") {
      const prelude = text.slice(start, i).trim();
      if (skipDepth < 0) {
        if (opaque.test(prelude)) skipDepth = depth;
        else if (!prelude.startsWith("@")) found.push(prelude);
      }
      depth++;
      start = i + 1;
    } else if (char === "}") {
      depth--;
      if (depth === skipDepth) skipDepth = -1;
      start = i + 1;
    }
  }
  return found;
}

const isPartOf = (part: string) =>
  (Object.values(anatomy) as AnatomyEntry[]).some((entry) => part in entry.parts);

/** What's wrong with one selector list. */
function lint(selector: string): string[] {
  const problems: string[] = [];
  const bare = selector.replace(/"[^"]*"/g, '""');
  if (/\.[A-Za-z_-]/.test(bare)) problems.push(`${selector}: class selector`);
  if (/#(?!root\b)[A-Za-z_-]/.test(bare)) problems.push(`${selector}: id selector`);
  for (const complex of selector.split(",")) {
    let root: string | undefined;
    for (const match of complex.matchAll(/\[data-(cs|part)="([^"]+)"\]/g)) {
      const [, kind, value = ""] = match;
      if (kind === "cs") {
        if (!(value in anatomy)) problems.push(`${selector}: "${value}" is not a catalogued root`);
        root = value;
      } else if (root && root in anatomy) {
        const entry: AnatomyEntry = anatomy[root as keyof typeof anatomy];
        if (!(value in entry.parts))
          problems.push(`${selector}: "${value}" is not a part of ${root}`);
      } else if (!isPartOf(value)) problems.push(`${selector}: "${value}" is no catalogued part`);
    }
  }
  return problems;
}

it("styles components only through catalogued anatomy selectors", () => {
  const files = [...cssFiles(dir(".")), ...cssFiles(dir("../../../apps/desktop/src"))];
  expect(files.length).toBeGreaterThan(0);
  const problems = files.flatMap((file) =>
    selectors(readFileSync(file, "utf8")).flatMap((selector) =>
      lint(selector).map((problem) => `${file}: ${problem}`),
    ),
  );
  expect(problems).toEqual([]);
});

it("flags class names, unknown roots and parts", () => {
  expect(
    selectors('@keyframes a { from { opacity: 0 } } .x { } @media (x) { [data-cs="hero"] { } }'),
  ).toEqual([".x", '[data-cs="hero"]']);
  expect(lint('[data-cs="game-card"] [data-part="art"]')).toEqual([]);
  expect(lint(".card:hover")).toEqual([".card:hover: class selector"]);
  expect(lint('[data-cs="cardz"]')).toEqual([
    '[data-cs="cardz"]: "cardz" is not a catalogued root',
  ]);
  expect(lint('[data-cs="hero"] [data-part="stores"]')).toEqual([
    '[data-cs="hero"] [data-part="stores"]: "stores" is not a part of hero',
  ]);
  expect(lint('[data-part="nope"]')).toEqual(['[data-part="nope"]: "nope" is no catalogued part']);
});

it("orders the layers so themes beat components and plugins and lose to the user's CSS", () => {
  const index = readFileSync(join(dir("../../../apps/desktop/src/styles"), "index.css"), "utf8");
  expect(index).toContain(
    "@layer cs.reset, cs.tokens, cs.base, cs.components, cs.plugins, cs.theme, cs.user;",
  );
});
