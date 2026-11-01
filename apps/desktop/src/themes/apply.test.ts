import { afterEach, expect, it, vi } from "vitest";
import { commands } from "../api/bindings";
import type { ResolvedTheme, ThemesApi } from "./api";
import {
  applyActiveTheme,
  applyResolvedTheme,
  applyTheme,
  colorScheme,
  DEFAULT_THEME,
  getActiveTheme,
  setActiveTheme,
} from "./apply";

afterEach(() => {
  vi.unstubAllGlobals();
  for (const element of document.head.querySelectorAll("[data-cs-theme]")) element.remove();
  delete document.documentElement.dataset.colorScheme;
  setActiveTheme(null);
});

const links = () =>
  [...document.head.querySelectorAll<HTMLLinkElement>("link[data-cs-theme]")].map((link) => [
    link.dataset.csTheme,
    link.getAttribute("href"),
  ]);

function resolved(id: string, overrides: Partial<ResolvedTheme> = {}): ResolvedTheme {
  return {
    id,
    name: id,
    requested: id,
    modes: ["dark"],
    url: `caskade-theme://localhost/${id}/theme.css`,
    notice: null,
    safeMode: false,
    ...overrides,
  };
}

it("inlines the default theme in the cs.theme layer without the core", async () => {
  await applyTheme(DEFAULT_THEME);
  const [style, ...rest] = document.head.querySelectorAll("style[data-cs-theme]");
  expect(rest).toHaveLength(0);
  // Vitest stubs CSS imports, so only the layer wrapper is visible here.
  expect(style?.textContent?.startsWith("@layer cs.theme {\n")).toBe(true);
  expect(style?.textContent?.endsWith("}\n")).toBe(true);
  await expect(applyTheme("dev.example.neon")).rejects.toThrow("needs the Caskade core");
});

it("swaps the core's compiled stylesheet in one link", async () => {
  vi.stubGlobal("isTauri", true);
  const stylesheet = vi.spyOn(commands, "themeStylesheet").mockImplementation(async (id) => ({
    status: "ok",
    data: `caskade-theme://localhost/${id}/theme.css`,
  }));
  await applyTheme(DEFAULT_THEME, { timeoutMs: 1 });
  await applyTheme("dev.example.neon", { timeoutMs: 1 });
  expect(links()).toEqual([
    ["dev.example.neon", "caskade-theme://localhost/dev.example.neon/theme.css"],
  ]);
  stylesheet.mockResolvedValueOnce({ status: "error", error: "theme dev.x isn't installed" });
  await expect(applyTheme("dev.x")).rejects.toThrow("isn't installed");
  expect(document.head.querySelectorAll("link[data-cs-theme]")).toHaveLength(1);
});

it("the colour scheme follows the OS when the theme has it, else the theme's default", () => {
  expect(colorScheme(["dark"], true)).toBe("dark");
  expect(colorScheme(["light", "dark"], false)).toBe("dark");
  expect(colorScheme(["light", "dark"], true)).toBe("light");
  expect(colorScheme(["light"], false)).toBe("light");
  expect(colorScheme([], true)).toBe("dark");
});

it("applies a resolved theme once, re-fetches it after a dev edit and sets the scheme", async () => {
  const root = document.documentElement;
  await applyResolvedTheme(resolved("dev.example.paper", { modes: ["light"] }), { timeoutMs: 1 });
  expect(links()).toEqual([
    ["dev.example.paper", "caskade-theme://localhost/dev.example.paper/theme.css"],
  ]);
  expect(root.dataset.colorScheme).toBe("light");
  expect(getActiveTheme()?.id).toBe("dev.example.paper");
  const first = document.head.querySelector("link[data-cs-theme]");
  // The same theme again changes nothing.
  await applyResolvedTheme(resolved("dev.example.paper", { modes: ["light"] }), { timeoutMs: 1 });
  expect(document.head.querySelector("link[data-cs-theme]")).toBe(first);
  // A dev reload asks for a fresh copy.
  await applyResolvedTheme(resolved("dev.example.paper", { modes: ["light"] }), {
    timeoutMs: 1,
    reload: true,
  });
  expect(links()).toEqual([
    ["dev.example.paper", expect.stringMatching(/\/dev\.example\.paper\/theme\.css\?v=\d+$/)],
  ]);
  await applyResolvedTheme(resolved(DEFAULT_THEME), { timeoutMs: 1 });
  expect(links().map(([id]) => id)).toEqual([DEFAULT_THEME]);
  expect(root.dataset.colorScheme).toBe("dark");
});

it("only the latest choice applies when switching quickly", async () => {
  let release = () => {};
  const slow = new Promise<void>((resolve) => {
    release = resolve;
  });
  const api = {
    resolve: async (id: string | null) => {
      if (id === "dev.example.slow") await slow;
      return resolved(id ?? DEFAULT_THEME);
    },
  } as unknown as ThemesApi;
  const first = applyActiveTheme(api, "dev.example.slow", { timeoutMs: 1 });
  await applyActiveTheme(api, "dev.example.fast", { timeoutMs: 1 });
  release();
  await first;
  expect(links().map(([id]) => id)).toEqual(["dev.example.fast"]);
  expect(getActiveTheme()?.id).toBe("dev.example.fast");
});
