import { caskadeI18n } from "@caskade/i18n/vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { pluginShims } from "./pluginShims";

// Port 1420 is the Tauri convention, so the same dev server can back `tauri dev` later.
export default defineConfig({
  // Lingui's macros and the PO catalogs, compiled per language (docs/translating.md).
  plugins: [caskadeI18n(), react(), tailwindcss(), pluginShims()],
  clearScreen: false,
  // `tauri dev` and `tauri build --debug` set TAURI_ENV_DEBUG; debug builds
  // keep WebKit's menu on Shift+right click (`shell/nativeMenu.ts`).
  envPrefix: ["VITE_", "TAURI_ENV_DEBUG"],
  server: {
    port: 1420,
    strictPort: true,
  },
});
