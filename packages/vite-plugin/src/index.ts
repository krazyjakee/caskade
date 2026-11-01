/** `@caskade/vite-plugin` (TECH-PLAN §4.4): builds a plugin's UI half as one
 * ES module, `dist/ui.js`, that the app loads with dynamic `import()`. */
import type { Plugin } from "vite";

/** Modules a UI plugin never bundles: the app's import map resolves them to
 * its own instances, so every plugin shares one React tree and one SDK. */
export const sharedSpecifiers = [
  "react",
  "react-dom",
  "react-dom/client",
  "react/jsx-runtime",
  "@caskade/sdk",
  "@caskade/ui",
] as const;

/** The cascade layer plugin styles live in (TECH-PLAN §5). */
export const STYLE_LAYER = "cs.plugins";

export interface CaskadePluginOptions {
  /** The module with the `definePlugin()` default export. */
  entry?: string;
  /** Where `ui.js` (and `ui.css`) go; `plugin.json`'s `ui` points here. */
  outDir?: string;
}

const shared = (id: string) =>
  sharedSpecifiers.some((specifier) => id === specifier || id.startsWith(`${specifier}/`));

export function caskadePlugin(options: CaskadePluginOptions = {}): Plugin {
  return {
    name: "caskade-plugin",
    config: () => ({
      define: { "process.env.NODE_ENV": JSON.stringify("production") },
      // The import map has `react/jsx-runtime`, not the dev runtime, so JSX
      // compiles for production whatever the mode.
      oxc: { jsx: { runtime: "automatic", development: false } },
      build: {
        lib: {
          entry: options.entry ?? "src/index.tsx",
          formats: ["es"],
          fileName: () => "ui.js",
          cssFileName: "ui",
        },
        outDir: options.outDir ?? "dist",
        cssCodeSplit: false,
        rolldownOptions: { external: shared },
      },
    }),
    // The shell imports `ui.css` into this layer too; declaring it here keeps
    // the rules layered when a stylesheet is loaded some other way.
    generateBundle: {
      // After Vite's CSS plugin has emitted `ui.css`.
      order: "post",
      handler(_options, bundle) {
        for (const file of Object.values(bundle))
          if (file.type === "asset" && file.fileName.endsWith(".css"))
            file.source = `@layer ${STYLE_LAYER} {\n${String(file.source)}\n}\n`;
      },
    },
  };
}
