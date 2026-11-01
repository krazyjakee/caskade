import { defineConfig } from "vitest/config";
import { caskadeI18n } from "./src/vite";

export default defineConfig({
  plugins: [caskadeI18n()],
  test: { environment: "node", include: ["src/**/*.test.{ts,tsx}"], restoreMocks: true },
});
