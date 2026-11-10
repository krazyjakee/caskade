/** Writes anatomy-entries.json (the catalogue as data, which the native
 * generator writes docs/theming/anatomy.md from) and anatomy.json (the
 * contract the core's theme lint checks selectors against) from the anatomy
 * catalogue; `--check` fails when either is stale. Run with
 * `node scripts/docs.ts`. */
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { type AnatomyEntry, anatomy, contractAttributes } from "../src/anatomy.ts";

const data = fileURLToPath(new URL("../anatomy-entries.json", import.meta.url));
const json = fileURLToPath(new URL("../anatomy.json", import.meta.url));

/** The catalogue as data, in order, for the native generator of
 * `docs/theming/anatomy.md` (`caskade_ui_theme::anatomy`), which writes the
 * docs from the native UI's components and these for the roots only the
 * webview renders yet. */
export function entries(): string {
  const list = (Object.entries(anatomy) as [string, AnatomyEntry][]).map(([name, entry]) => ({
    name,
    rendered: entry.rendered,
    source: entry.source === "@caskade/ui" ? "ui" : entry.source,
    description: entry.description,
    parts: Object.entries(entry.parts),
    states: Object.entries(entry.states),
    interaction: entry.interaction,
    appOnly: entry.appOnly ?? [],
    contains: entry.contains ?? [],
    slots: entry.slots ?? [],
    container: entry.container ?? false,
  }));
  return `${JSON.stringify(list, null, 2)}\n`;
}

/** The catalogue as data: roots with their parts, states and interaction,
 * and every attribute a selector may use. */
export function contract(): string {
  const entries = Object.entries(anatomy) as [string, AnatomyEntry][];
  const attributes = new Set<string>([
    "data-cs",
    "data-part",
    ...contractAttributes.root,
    ...contractAttributes.parts,
  ]);
  const roots = Object.fromEntries(
    entries.map(([name, entry]) => {
      for (const attribute of [...Object.keys(entry.states), ...entry.interaction])
        attributes.add(attribute);
      return [
        name,
        {
          parts: Object.keys(entry.parts),
          states: entry.states,
          interaction: entry.interaction,
        },
      ];
    }),
  );
  return `${JSON.stringify({ roots, attributes: [...attributes].sort() }, null, 2)}\n`;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const outputs = [
    [data, entries()],
    [json, contract()],
  ] as const;
  if (process.argv.includes("--check")) {
    for (const [file, text] of outputs) {
      let current = "";
      try {
        current = readFileSync(file, "utf8");
      } catch {}
      if (current !== text) {
        console.error(`${file} is stale: run pnpm --filter @caskade/ui docs`);
        process.exit(1);
      }
    }
  } else for (const [file, text] of outputs) writeFileSync(file, text);
}
