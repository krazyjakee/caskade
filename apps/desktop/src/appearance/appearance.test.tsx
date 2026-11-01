import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { SettingsApi, SettingsChanged, SettingsSnapshot } from "../api/settings";
import type { InstalledTheme, ResolvedTheme, ThemeReloaded, ThemesApi } from "../themes/api";
import { getActiveTheme, setActiveTheme } from "../themes/apply";
import { AppearanceRoot, appearanceKey } from "./AppearanceRoot";
import { applyAppearance } from "./apply";
import { startAppearance } from "./startup";

// jsdom loads no stylesheets: finish each theme <link> as it is added.
const loads = new MutationObserver((records) => {
  for (const record of records)
    for (const node of record.addedNodes)
      if (node instanceof HTMLLinkElement)
        queueMicrotask(() => node.dispatchEvent(new Event("load")));
});
beforeEach(() => loads.observe(document.head, { childList: true }));
afterEach(() => {
  loads.disconnect();
  cleanup();
  setActiveTheme(null);
  for (const element of document.head.querySelectorAll("[data-cs-theme], [data-cs-user]"))
    element.remove();
  delete document.documentElement.dataset.colorScheme;
});

function root() {
  const element = document.createElement("html");
  element.style.setProperty("--cs-color-surface-base", "#060914");
  element.style.setProperty("--cs-color-surface-raised", "#0d1224d9");
  return element;
}

const defaults = {
  theme: "caskade.default",
  density: "comfortable",
  cardSize: 176,
  uiScale: 100,
  showArtTitles: true,
  motion: "system",
  reduceEffects: false,
  accent: null,
  customCss: "",
  customCssEnabled: true,
};

const nexus: InstalledTheme = {
  id: "caskade.default",
  name: "Nexus",
  version: "1.0.0",
  modes: ["dark"],
  previews: [],
  source: "builtin",
  problems: [],
};
const paper: InstalledTheme = {
  ...nexus,
  id: "dev.example.paper",
  name: "Paper",
  modes: ["light"],
  source: "user",
};

/** The settings section in memory, with change events like the core's. */
function settingsBackend(value: Record<string, unknown> = defaults) {
  let snapshot: SettingsSnapshot = { namespace: "core.appearance", version: 2, revision: 0, value };
  const listeners = new Set<(event: SettingsChanged) => void>();
  const api = {
    get: async () => snapshot,
    subscribe: async (listener: (event: SettingsChanged) => void) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  } as unknown as SettingsApi;
  return {
    api,
    current: () => snapshot.value,
    /** A save from elsewhere (the Appearance panel). */
    save(change: Record<string, unknown>) {
      snapshot = {
        ...snapshot,
        revision: snapshot.revision + 1,
        value: { ...snapshot.value, ...change },
      };
      act(() => {
        for (const listener of listeners)
          listener({ namespace: "core.appearance", version: snapshot.version });
      });
    },
  };
}

/** The core's themes: `resolve(null)` reads the settings, a missing or
 * broken theme falls back to the default with a notice. */
function themesBackend(installed: () => InstalledTheme[], setting: () => string, safeMode = false) {
  const listeners = new Set<(event: ThemeReloaded) => void>();
  const api: ThemesApi = {
    list: async () => installed(),
    reload: async () => installed(),
    resolve: async (id): Promise<ResolvedTheme> => {
      const requested = id ?? setting();
      const found = safeMode
        ? undefined
        : installed().find((theme) => theme.id === requested && theme.problems.length === 0);
      const theme = found ?? nexus;
      return {
        id: theme.id,
        name: theme.name,
        requested,
        modes: theme.modes,
        url: `caskade-theme://localhost/${theme.id}/theme.css`,
        notice: safeMode
          ? "Safe mode"
          : found || requested === nexus.id
            ? null
            : `The theme ${requested} isn't installed any more. Caskade is using the default theme instead.`,
        safeMode,
      };
    },
    onReloaded: async (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  return {
    api,
    edited: (theme: string) =>
      act(() => {
        for (const listener of listeners) listener({ theme, error: null });
      }),
  };
}

const themeLinks = () =>
  [...document.head.querySelectorAll<HTMLLinkElement>("link[data-cs-theme]")].map(
    (link) => link.getAttribute("href") ?? "",
  );

/** Mounts AppearanceRoot and waits for its first read of the settings: a
 * change event before that joins the read already in flight. */
async function mount(settings: SettingsApi, themes: ThemesApi, html = root()) {
  const client = new QueryClient();
  render(
    <QueryClientProvider client={client}>
      <AppearanceRoot api={settings} themes={themes} root={html} />
    </QueryClientProvider>,
  );
  await waitFor(() => expect(client.getQueryData(appearanceKey)).toBeDefined());
  return html;
}

it("maps the settings to root attributes and token overrides", () => {
  const html = root();
  applyAppearance(html, {
    density: "compact",
    uiScale: 125,
    motion: "reduced",
    reduceEffects: true,
    accent: "#e0457b",
  });
  expect(html.dataset).toMatchObject({ density: "compact", motion: "reduced", effects: "reduced" });
  expect(html.style.getPropertyValue("--cs-ui-scale")).toBe("1.25");
  expect(html.style.getPropertyValue("--cs-color-accent")).toBe("#e0457b");
  expect(html.style.getPropertyValue("--cs-color-glow")).toMatch(/80$/);

  // Back to the theme's accent and full effects.
  applyAppearance(html, { ...defaults, density: "comfortable", motion: "system" } as never);
  expect(html.dataset.effects).toBeUndefined();
  expect(html.dataset.motion).toBe("system");
  expect(html.style.getPropertyValue("--cs-color-accent")).toBe("");
});

it("restyles live when core.appearance changes", async () => {
  const settings = settingsBackend();
  const themes = themesBackend(
    () => [nexus],
    () => "caskade.default",
  );
  const html = await mount(settings.api, themes.api);
  await waitFor(() => expect(html.dataset.density).toBe("comfortable"));
  settings.save({ density: "compact", uiScale: 90, reduceEffects: true, accent: "#1a1f4a" });
  await waitFor(() => expect(html.dataset.density).toBe("compact"));
  expect(html.dataset.effects).toBe("reduced");
  expect(html.style.getPropertyValue("--cs-ui-scale")).toBe("0.9");
  // An unreadable accent was lightened before it was applied.
  const accent = html.style.getPropertyValue("--cs-color-accent");
  expect(accent).toMatch(/^#[0-9a-f]{6}$/);
  expect(accent).not.toBe("#1a1f4a");
});

it("switching themes swaps the stylesheet without a reload, and the choice survives a restart", async () => {
  const settings = settingsBackend();
  const theme = () => settings.current().theme as string;
  const themes = themesBackend(() => [nexus, paper], theme);
  const html = root();
  await startAppearance({ settings: settings.api, themes: themes.api, root: html });
  expect(themeLinks()).toEqual(["caskade-theme://localhost/caskade.default/theme.css"]);
  expect(html.dataset.colorScheme).toBe("dark");
  await mount(settings.api, themes.api, html);

  settings.save({ theme: "dev.example.paper" });
  await waitFor(() =>
    expect(themeLinks()).toEqual(["caskade-theme://localhost/dev.example.paper/theme.css"]),
  );
  expect(html.dataset.colorScheme).toBe("light");
  expect(getActiveTheme()?.name).toBe("Paper");

  // Restart: startup applies the saved choice before the first render.
  cleanup();
  setActiveTheme(null);
  for (const link of document.head.querySelectorAll("[data-cs-theme]")) link.remove();
  await startAppearance({ settings: settings.api, themes: themes.api, root: html });
  expect(themeLinks()).toEqual(["caskade-theme://localhost/dev.example.paper/theme.css"]);
  expect(getActiveTheme()?.notice).toBeNull();
});

it("a theme removed while selected falls back to the default with a notice", async () => {
  let installed = [nexus, paper];
  const settings = settingsBackend({ ...defaults, theme: "dev.example.paper" });
  const themes = themesBackend(
    () => installed,
    () => settings.current().theme as string,
  );
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  await startAppearance({ settings: settings.api, themes: themes.api, root: root() });
  expect(themeLinks()).toEqual(["caskade-theme://localhost/dev.example.paper/theme.css"]);

  installed = [nexus];
  await startAppearance({ settings: settings.api, themes: themes.api, root: root() });
  expect(themeLinks()).toEqual(["caskade-theme://localhost/caskade.default/theme.css"]);
  const active = getActiveTheme();
  expect(active?.requested).toBe("dev.example.paper");
  expect(active?.notice).toBe(
    "The theme dev.example.paper isn't installed any more. Caskade is using the default theme instead.",
  );
  expect(warn).toHaveBeenCalledWith(active?.notice);
});

it("a dev theme's edit re-applies it at once", async () => {
  const settings = settingsBackend({ ...defaults, theme: "dev.example.paper" });
  const themes = themesBackend(
    () => [nexus, { ...paper, source: "dev" }],
    () => settings.current().theme as string,
  );
  const html = root();
  await startAppearance({ settings: settings.api, themes: themes.api, root: html });
  await mount(settings.api, themes.api, html);
  themes.edited("dev.example.other");
  themes.edited("dev.example.paper");
  await waitFor(() =>
    expect(themeLinks()).toEqual([
      expect.stringMatching(
        /^caskade-theme:\/\/localhost\/dev\.example\.paper\/theme\.css\?v=\d+$/,
      ),
    ]),
  );
});

it("custom CSS applies live in the user layer; the switch and safe mode remove it", async () => {
  const css = '[data-cs="hero"] { color: hotpink; }';
  const userStyle = () => document.head.querySelector("style[data-cs-user]")?.textContent ?? null;
  const settings = settingsBackend();
  const themes = themesBackend(
    () => [nexus],
    () => "caskade.default",
  );
  const html = root();
  await startAppearance({ settings: settings.api, themes: themes.api, root: html });
  await mount(settings.api, themes.api, html);
  expect(userStyle()).toBeNull();

  settings.save({ customCss: css });
  await waitFor(() => expect(userStyle()).toContain(`@layer cs.user {\n${css}\n}`));
  settings.save({ customCssEnabled: false });
  await waitFor(() => expect(userStyle()).toBeNull());
  settings.save({ customCssEnabled: true });
  await waitFor(() => expect(userStyle()).not.toBeNull());

  // A refused sheet isn't applied at all.
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  settings.save({ customCss: `${css}\n@import url(https://example.com/x.css);` });
  await waitFor(() => expect(userStyle()).toBeNull());
  expect(String(warn.mock.calls.at(-1)?.[0])).toContain("is refused");

  // Safe mode: the default theme and no user CSS, from the first paint on.
  cleanup();
  setActiveTheme(null);
  const safe = settingsBackend({ ...defaults, theme: "dev.example.paper", customCss: css });
  const safeThemes = themesBackend(
    () => [nexus, paper],
    () => "dev.example.paper",
    true,
  );
  await startAppearance({ settings: safe.api, themes: safeThemes.api, root: html });
  expect(userStyle()).toBeNull();
  expect(themeLinks()).toEqual(["caskade-theme://localhost/caskade.default/theme.css"]);
  await mount(safe.api, safeThemes.api, html);
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(userStyle()).toBeNull();
});
