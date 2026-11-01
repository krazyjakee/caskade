// Builds a plugin folder's UI half with the app's Vite and
// @caskade/vite-plugin, the same as the folder's own vite.config.ts, without
// installing its dependencies (tests, #130): node build-plugin-ui.mjs <folder>
import { caskadePlugin } from "@caskade/vite-plugin";
import { build } from "vite";

const root = process.argv[2];
if (!root) throw new Error("usage: build-plugin-ui.mjs <plugin folder>");
await build({ root, configFile: false, logLevel: "warn", plugins: [caskadePlugin()] });
