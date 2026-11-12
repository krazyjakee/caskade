// The npm packages the app's production bundle contains, for the
// third-party notices (scripts/legal-notices.py, #130). Builds the bundle in
// memory, as `vite build` does for a release, and lists the package of every
// module and asset in it (fonts included): build-time tools, types and code
// the bundler drops never ship, so they aren't listed. Prints JSON sorted by
// name and version: node apps/desktop/scripts/shipped-packages.mjs
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const NODE_MODULES = `${sep}node_modules${sep}`;

const built = await build({ root, logLevel: "error", build: { write: false } });
const outputs = (Array.isArray(built) ? built : [built]).flatMap((result) => result.output);

const files = new Set();
for (const item of outputs) {
  if (item.type === "chunk") {
    for (const id of item.moduleIds ?? Object.keys(item.modules ?? {})) files.add(id);
  } else {
    for (const name of item.originalFileNames ?? []) files.add(join(root, name));
  }
}

/** The licence expression a manifest gives, old `licenses` arrays included. */
function licence(manifest) {
  const { license, licenses } = manifest;
  if (typeof license === "string") return license;
  if (license?.type) return license.type;
  if (Array.isArray(licenses) && licenses.length > 0)
    return licenses.map((each) => each.type ?? each).join(" OR ");
  return "Unknown";
}

function repository(manifest) {
  const repo = manifest.repository;
  const url = typeof repo === "string" ? repo : repo?.url;
  return manifest.homepage ?? url?.replace(/^git\+/, "").replace(/\.git$/, "") ?? "";
}

function author(manifest) {
  const who = manifest.author;
  return typeof who === "string" ? who.replace(/\s*<[^>]*>/, "") : (who?.name ?? null);
}

const packages = new Map();
for (const raw of files) {
  const file = raw.replace(/^\0/, "").split("?")[0];
  const at = file.lastIndexOf(NODE_MODULES);
  if (at < 0) continue;
  const parts = file.slice(at + NODE_MODULES.length).split(sep);
  const name = parts[0].startsWith("@") ? `${parts[0]}/${parts[1]}` : parts[0];
  const path = join(file.slice(0, at + NODE_MODULES.length), ...name.split("/"));
  if (!existsSync(join(path, "package.json"))) continue;
  const manifest = JSON.parse(readFileSync(join(path, "package.json"), "utf8"));
  packages.set(`${manifest.name}@${manifest.version}`, {
    name: manifest.name,
    version: manifest.version,
    license: licence(manifest),
    repository: repository(manifest),
    author: author(manifest),
    path,
  });
}

const sorted = [...packages.values()].sort(
  (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
);
process.stdout.write(`${JSON.stringify(sorted, null, 2)}\n`);
