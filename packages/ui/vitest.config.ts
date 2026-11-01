import { caskadeI18n } from "@caskade/i18n/vite";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [caskadeI18n()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    restoreMocks: true,
    setupFiles: ["./src/testSetup.ts"],
  },
});
