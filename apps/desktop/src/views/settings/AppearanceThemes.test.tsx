import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { SettingsApi, SettingsSnapshot } from "../../api/settings";
import { type InstalledTheme, type ThemesApi, ThemesApiContext } from "../../themes/api";
import { setActiveTheme } from "../../themes/apply";
import { AppearanceSettings } from "./AppearanceSettings";
import { SettingsProvider } from "./useSettings";

afterEach(() => {
  cleanup();
  setActiveTheme(null);
});

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

const installed: InstalledTheme[] = [
  {
    id: "caskade.default",
    name: "Nexus",
    version: "1.0.0",
    author: "Caskade",
    modes: ["dark"],
    previews: [],
    source: "builtin",
    problems: [],
  },
  {
    id: "dev.example.paper",
    name: "Paper",
    version: "0.2.0",
    author: "Jane Doe",
    description: "Ink on paper.",
    modes: ["light"],
    previews: ["caskade-theme://localhost/dev.example.paper/assets/shot.png"],
    source: "user",
    problems: [],
  },
  {
    id: "dev.example.broken",
    name: "Broken",
    version: "0.1.0",
    modes: ["dark"],
    previews: [],
    source: "user",
    problems: [
      { file: "theme.css", line: 3, column: 1, message: "class selectors aren't allowed" },
    ],
  },
];

function show(value: Record<string, unknown> = defaults) {
  let snapshot: SettingsSnapshot = { namespace: "core.appearance", version: 2, revision: 0, value };
  const saved: Record<string, unknown>[] = [];
  const api: SettingsApi = {
    sections: async () => [],
    get: async () => structuredClone(snapshot),
    set: async (_namespace, next, revision) => {
      saved.push(next);
      snapshot = { ...snapshot, revision: revision + 1, value: next };
      return structuredClone(snapshot);
    },
    reset: async () => snapshot,
    subscribe: async () => () => {},
  };
  const themes: ThemesApi = {
    list: async () => installed,
    reload: async () => installed,
    resolve: async () => {
      throw new Error("not used");
    },
    onReloaded: async () => () => {},
  };
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ThemesApiContext.Provider value={themes}>
        <SettingsProvider api={api}>
          <AppearanceSettings />
        </SettingsProvider>
      </ThemesApiContext.Provider>
    </QueryClientProvider>,
  );
  return saved;
}

it("lists every theme with its author and previews, and the problems of one that can't be used", async () => {
  const saved = show();
  const picker = await screen.findByRole("radiogroup", { name: "Theme" });
  const paper = await within(picker).findByRole("radio", { name: /Paper/ });
  const option = paper.closest("label") as HTMLElement;
  expect(option.textContent).toContain("by Jane Doe");
  expect(option.textContent).toContain("Ink on paper.");
  expect(option.querySelector("img")?.getAttribute("src")).toBe(
    "caskade-theme://localhost/dev.example.paper/assets/shot.png",
  );
  expect((within(picker).getByRole("radio", { name: /Nexus/ }) as HTMLInputElement).checked).toBe(
    true,
  );
  const broken = within(picker).getByRole("radio", { name: /Broken/ }) as HTMLInputElement;
  expect(broken.disabled).toBe(true);
  expect(broken.closest("label")?.textContent).toContain(
    "theme.css:3:1: class selectors aren't allowed",
  );

  fireEvent.click(paper);
  await waitFor(() => expect(saved.at(-1)?.theme).toBe("dev.example.paper"));
});

it("shows why the chosen theme isn't the one applied", async () => {
  setActiveTheme({
    id: "caskade.default",
    name: "Nexus",
    requested: "dev.example.gone",
    modes: ["dark"],
    url: null,
    notice: "The theme dev.example.gone isn't installed any more.",
    safeMode: false,
  });
  show({ ...defaults, theme: "dev.example.gone" });
  // The panel replaces "Loading settings…" once the section is read.
  await screen.findByRole("radiogroup", { name: "Theme" });
  const notice = await screen.findByText("The theme dev.example.gone isn't installed any more.");
  expect(notice.getAttribute("role")).toBe("status");
});

it("saves custom CSS, refuses remote imports with a message, and can switch it off", async () => {
  const saved = show();
  const editor = await screen.findByRole("textbox", { name: "Custom CSS" });
  const save = screen.getByRole("button", { name: "Save CSS" });
  fireEvent.change(editor, {
    target: { value: '@import url("https://example.com/look.css");\n[data-cs="hero"] {}' },
  });
  fireEvent.click(save);
  expect((await screen.findByRole("alert")).textContent).toContain(
    "Line 1: @import of https://example.com/look.css is refused: custom CSS can't load anything from other origins.",
  );
  expect(saved).toEqual([]);

  const css = '[data-cs="hero"] { color: hotpink; }';
  fireEvent.change(editor, { target: { value: css } });
  fireEvent.click(save);
  await waitFor(() => expect(saved.at(-1)?.customCss).toBe(css));
  expect(screen.queryByRole("alert")).toBeNull();
  // The gutter numbers each line.
  fireEvent.change(editor, { target: { value: `${css}\n\n` } });
  expect(editor.parentElement?.parentElement?.textContent).toContain("123");

  fireEvent.click(screen.getByRole("switch", { name: "Disable custom CSS" }));
  await waitFor(() => expect(saved.at(-1)?.customCssEnabled).toBe(false));
  expect(saved.at(-1)?.customCss).toBe(css);
});

it("tells the user when safe mode keeps the custom CSS off", async () => {
  setActiveTheme({
    id: "caskade.default",
    name: "Nexus",
    requested: "caskade.default",
    modes: ["dark"],
    url: null,
    notice: "Safe mode: Caskade started with the default theme and without your custom CSS.",
    safeMode: true,
  });
  show({ ...defaults, customCss: '[data-cs="hero"] { color: red; }' });
  await screen.findByRole("radiogroup", { name: "Theme" });
  // The sliders' value outputs are statuses too; these are the notices.
  const notices = (await screen.findAllByRole("status"))
    .map((status) => status.textContent)
    .filter((text) => text?.startsWith("Safe mode"));
  expect(notices).toEqual([
    "Safe mode: Caskade started with the default theme and without your custom CSS.",
    "Safe mode is on: your custom CSS isn't applied until Caskade starts normally.",
  ]);
});
