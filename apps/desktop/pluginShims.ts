import { sharedSpecifiers } from "@caskade/vite-plugin";
import type { Plugin } from "vite";
import { appExports, importMap, shimPath, shimSource } from "./src/plugins/sharedExports";

/** Serves (dev) and emits (build) the `/plugin-shims/*.js` modules and puts
 * the import map in index.html, so UI plugins share the app's React and SDK
 * (TECH-PLAN §4.4). Package export names are read from the installed
 * packages; the app's own modules list theirs in `sharedExports.ts`. */
export function pluginShims(): Plugin {
  let shims: Promise<Map<string, string>> | undefined;
  const all = () => {
    shims ??= Promise.all(
      sharedSpecifiers.map(async (specifier) => {
        const names = appExports[specifier] ?? Object.keys(await import(specifier));
        return [shimPath(specifier), shimSource(specifier, names)] as const;
      }),
    ).then((entries) => new Map(entries));
    return shims;
  };
  return {
    name: "caskade-plugin-shims",
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        const path = (request as { url?: string }).url?.split("?")[0] ?? "";
        all().then((sources) => {
          const source = sources.get(path);
          if (source === undefined) return next();
          response.setHeader("Content-Type", "text/javascript");
          response.end(source);
        }, next);
      });
    },
    async generateBundle() {
      for (const [path, source] of await all())
        this.emitFile({ type: "asset", fileName: path.slice(1), source });
    },
    transformIndexHtml: {
      order: "pre",
      handler: () => [
        {
          tag: "script",
          attrs: { type: "importmap" },
          children: JSON.stringify(importMap()),
          injectTo: "head-prepend",
        },
      ],
    },
  };
}
