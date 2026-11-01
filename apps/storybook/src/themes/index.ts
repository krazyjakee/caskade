/** The themes the toolbar offers: every folder under the repository's
 * `themes/` (the built-in `caskade.default`, the second theme, and any theme
 * folder you copy or symlink there), read at build time through
 * `import.meta.glob`. In `pnpm storybook` Vite watches the folder, so edits
 * to a theme's files restyle the open story. */
import contract from "@caskade/tokens/manifest.json";
import { compileTheme, type ThemeFolder, type ThemeManifest, themeChain } from "./compile";

const manifests = import.meta.glob<ThemeManifest>("../../../../themes/*/theme.json", {
  eager: true,
  import: "default",
});
const tokenFiles = import.meta.glob<unknown>("../../../../themes/*/tokens.json", {
  eager: true,
  import: "default",
});
const stylesheets = import.meta.glob<string>("../../../../themes/*/theme.css", {
  eager: true,
  query: "?raw",
  import: "default",
});
const assetFiles = import.meta.glob<string>("../../../../themes/*/assets/**/*", {
  eager: true,
  query: "?url",
  import: "default",
});

const folder = (key: string) => key.split("/")[5] ?? "";
const inFolder = <T>(files: Record<string, T>, dir: string, file: string) =>
  files[`../../../../themes/${dir}/${file}`];

/** The theme folders with a usable `theme.json`, sorted by name. */
export const themes: readonly ThemeFolder[] = Object.entries(manifests)
  .filter(([, manifest]) => typeof manifest?.id === "string" && typeof manifest.name === "string")
  .map(([key, manifest]): ThemeFolder => {
    const dir = folder(key);
    const prefix = `../../../../themes/${dir}/`;
    const assets: Record<string, string> = {};
    for (const [path, url] of Object.entries(assetFiles))
      if (path.startsWith(`${prefix}assets/`)) assets[path.slice(prefix.length)] = url;
    return {
      dir,
      manifest,
      tokens: inFolder(tokenFiles, dir, "tokens.json"),
      css: inFolder(stylesheets, dir, "theme.css") ?? "",
      assets,
    };
  })
  .sort((a, b) => a.manifest.name.localeCompare(b.manifest.name));

/** No theme: the contract's default tokens and the components' own styles. */
export const NO_THEME = "none";
export const DEFAULT_THEME = "caskade.default";

/** The compiled stylesheet for `id`; throws a `ThemeError` naming the problems. */
export function themeStylesheet(id: string): string {
  if (id === NO_THEME) return "";
  return compileTheme(themeChain(id, themes), contract);
}

/** The colour scheme a theme starts in: its first mode, dark by default. */
export function defaultScheme(id: string): "dark" | "light" {
  return themes.find((theme) => theme.manifest.id === id)?.manifest.modes?.[0] ?? "dark";
}

/** Swaps the playground's theme stylesheet when `id` changes. */
export function applyTheme(id: string, css: string) {
  const current = document.head.querySelector<HTMLStyleElement>("style[data-cs-theme]");
  if (current?.dataset.csTheme === id && current.textContent === css) return;
  const style = document.createElement("style");
  style.dataset.csTheme = id;
  style.textContent = css;
  document.head.append(style);
  current?.remove();
}
