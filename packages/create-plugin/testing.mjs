// Temp folders and file reads for tests that generate plugins (no Node
// typings needed by the caller).
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const tempFolder = () => mkdtempSync(join(tmpdir(), "caskade-plugin-"));
export const readText = (dir, path) => readFileSync(join(dir, path), "utf8");
export const removeFolder = (dir) => rmSync(dir, { recursive: true, force: true });
export const joinPath = (...parts) => join(...parts);
export const listFolder = (dir, path) => readdirSync(join(dir, path)).sort();
