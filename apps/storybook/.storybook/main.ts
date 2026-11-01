import { fileURLToPath } from "node:url";
import { caskadeI18n } from "@caskade/i18n/vite";
import type { StorybookConfig } from "@storybook/react-vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { mergeConfig } from "vite";

/** The repository root: stories read `packages/*`, the app's stylesheet and `themes/*`. */
const repo = fileURLToPath(new URL("../../..", import.meta.url));

const config: StorybookConfig = {
  framework: "@storybook/react-vite",
  stories: ["../src/**/*.stories.tsx"],
  core: { disableTelemetry: true },
  typescript: { reactDocgen: false },
  viteFinal: (vite) =>
    mergeConfig(vite, {
      // The pseudo-locale stays in built Storybooks, for checking layouts.
      plugins: [caskadeI18n({ root: repo, pseudo: true }), react(), tailwindcss()],
      server: { fs: { allow: [repo] } },
    }),
};

export default config;
