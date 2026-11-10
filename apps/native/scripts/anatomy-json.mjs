// Writes the webview's anatomy catalogue (packages/ui/src/anatomy.ts) as
// JSON to stdout, in order, for caskade-ui-theme's test that its generators
// write today's docs/theming/anatomy.md and anatomy.json byte for byte
// (#90):
//
//   node apps/native/scripts/anatomy-json.mjs > target/anatomy.json
//   CASKADE_ANATOMY_JSON=target/anatomy.json cargo test -p caskade-ui-theme anatomy
import { anatomy } from "../../../packages/ui/src/anatomy.ts";

const entries = Object.entries(anatomy).map(([name, entry]) => ({
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
process.stdout.write(`${JSON.stringify(entries)}\n`);
