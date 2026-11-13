// Builds an example plugin's halves into its dist/ (#133):
//   node plugins/examples/build.mjs <example folder> [more…]
// The backend builds for wasm32-wasip2 with cargo and draws the plugin's
// UI (plugin API 0.2); 0.1's JavaScript UI halves are no longer built.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const here = new URL(".", import.meta.url).pathname;
const folders = process.argv.slice(2);
if (folders.length === 0) throw new Error("usage: build.mjs <example folder>…");

for (const folder of folders.map((name) => resolve(here, name))) {
  const manifest = JSON.parse(readFileSync(join(folder, "plugin.json"), "utf8"));
  if (manifest.ui) throw new Error(`${folder}: 0.1 JavaScript UI halves are no longer built`);
  if (manifest.backend) {
    const crate = join(folder, "backend/Cargo.toml");
    const output = execFileSync(
      "cargo",
      [
        "build",
        "--release",
        "--locked",
        "--target",
        "wasm32-wasip2",
        "--manifest-path",
        crate,
        "--message-format=json-render-diagnostics",
      ],
      { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], maxBuffer: 64 << 20 },
    );
    const wasm = output
      .split("\n")
      .filter((line) => line.startsWith("{"))
      .map((line) => JSON.parse(line))
      .filter(
        (message) => message.reason === "compiler-artifact" && message.manifest_path === crate,
      )
      .flatMap((message) => message.filenames)
      .find((file) => file.endsWith(".wasm"));
    if (!wasm) throw new Error(`no component built for ${manifest.id}`);
    mkdirSync(join(folder, "dist"), { recursive: true });
    copyFileSync(wasm, join(folder, manifest.backend));
  }
  console.log(`built ${manifest.id}`);
}
