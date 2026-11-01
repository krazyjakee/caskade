import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import {
  createPluginI18n,
  definePlugin,
  FocusSection,
  type GameQuery,
  HostScope,
  type HostServices,
  type PluginContext,
  type PluginMessages,
  PluginScope,
  pluginSlots,
  Trans,
  useBackendQuery,
  useFocusable,
  useGame,
  useInputAction,
  useMode,
  useSettings,
  useTranslation,
} from "./index";

afterEach(cleanup);

function context(): PluginContext {
  let value: Record<string, unknown> = { limit: 5 };
  const listeners = new Set<(value: unknown) => void>();
  return {
    plugin: { id: "dev.example.stats", name: "Stats", version: "1.0.0" },
    slots: { register: () => () => {} },
    events: { on: () => () => {} },
    backend: { call: async (method, payload) => ({ method, payload }) as never },
    library: { game: async () => null },
    settings: {
      get: async () => value as never,
      set: async (next) => {
        value = next as Record<string, unknown>;
        for (const listener of listeners) listener(value);
        return next;
      },
      on: (listener) => {
        listeners.add(listener as (value: unknown) => void);
        return () => listeners.delete(listener as (value: unknown) => void);
      },
    },
    notify: vi.fn(),
    i18n: createPluginI18n(null),
  };
}
function Scoped({ children, ctx }: { children: ReactNode; ctx: PluginContext }) {
  const game: GameQuery = {
    data: {
      id: "g1",
      title: "Celeste",
      platform: "PC",
      status: "installed",
      favorite: false,
      playtimeSeconds: 60,
      lastPlayedAt: null,
      description: "",
      tags: [],
    },
    isPending: false,
    error: null,
  };
  return (
    <QueryClientProvider client={new QueryClient()}>
      <HostScope.Provider
        value={{
          useGame: () => game,
          useGames: () => ({ data: game.data ? [game.data] : [], isPending: false, error: null }),
        }}
      >
        <PluginScope.Provider value={ctx}>{children}</PluginScope.Provider>
      </HostScope.Provider>
    </QueryClientProvider>
  );
}

function Panel() {
  const answer = useBackendQuery<{ method: string }>("lookup");
  const { settings, setSettings } = useSettings<{ limit: number }>();
  const game = useGame("g1");
  return (
    <div>
      <p>{answer.data?.method ?? "…"}</p>
      <p>{game.data?.title}</p>
      <button type="button" onClick={() => setSettings({ limit: 9 })}>
        Limit {settings?.limit ?? "…"}
      </button>
    </div>
  );
}

it("gives contributed components their plugin's backend, settings and games", async () => {
  render(
    <Scoped ctx={context()}>
      <Panel />
    </Scoped>,
  );
  await screen.findByText("lookup");
  expect(screen.getByText("Celeste")).toBeTruthy();
  fireEvent.click(await screen.findByRole("button", { name: "Limit 5" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Limit 9" })).toBeTruthy());
});

function Orphan() {
  useGame("g1");
  return null;
}

it("explains hooks used outside a plugin or the app", () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  expect(() => render(<Orphan />)).toThrow("useGame() needs the Caskade app");
  expect(definePlugin({ activate() {} }).activate).toBeTypeOf("function");
  expect(pluginSlots).toContain("settings.pages");
});

function Tiles() {
  const mode = useMode();
  useInputAction("details", () => true, { label: "Details" });
  const tile = useFocusable<HTMLDivElement>({ focusKey: "tile" });
  return (
    <FocusSection id="tiles">
      <div ref={tile.ref}>Mode {mode}</div>
    </FocusSection>
  );
}

function Paged() {
  useInputAction("tab.next", () => true, { label: "Next" });
  return <p>Mode {useMode()}</p>;
}

it("console mode's hooks work outside Caskade: desktop mode, no bindings, plain sections", () => {
  render(<Tiles />);
  const tile = screen.getByText("Mode desktop");
  expect(tile.parentElement?.dataset.focusSection).toBe("tiles");
  expect(tile.tabIndex).toBe(0);
  expect(tile.dataset.focusKey).toBe("tile");
});

it("inside the app, the host answers useMode and receives useInputAction", () => {
  const bound: string[] = [];
  const host: HostServices = {
    useGame: () => ({ data: null, isPending: false, error: null }),
    useGames: () => ({ data: [], isPending: false, error: null }),
    useMode: () => "console",
    useInputAction: (action, _handler, options) => {
      bound.push(`${action}:${options?.label}`);
    },
  };
  render(
    <HostScope.Provider value={host}>
      <Paged />
    </HostScope.Provider>,
  );
  expect(screen.getByText("Mode console")).toBeTruthy();
  expect(bound[0]).toBe("tab.next:Next");
});

const swedish: PluginMessages = {
  locale: "sv",
  messages: [
    { context: null, id: "Game packs", translations: ["Spelpaket"] },
    { context: null, id: "Installed {game}", translations: ["Installerade {game}"] },
    {
      context: null,
      id: "Drop a pack on {library}",
      translations: ["Släpp ett paket på {library}"],
    },
    { context: "verb", id: "Pack", translations: ["Packa"] },
    { context: null, id: "One pack", translations: ["Ett paket", "{n} paket"] },
  ],
  plurals: Array.from({ length: 200 }, (_, n) => (n === 1 ? 0 : 1)),
};

it("translates a plugin's own messages and shows English for the rest (#219)", () => {
  const i18n = createPluginI18n(swedish);
  expect(i18n.locale).toBe("sv");
  expect(i18n.t("Game packs")).toBe("Spelpaket");
  expect(i18n.t("Installed {game}", { game: "Celeste" })).toBe("Installerade Celeste");
  expect(i18n.t("Pack")).toBe("Pack");
  expect(i18n.t("Pack", undefined, { context: "verb" })).toBe("Packa");
  expect(i18n.t("Not translated")).toBe("Not translated");
  expect(i18n.plural(1, "One pack", "{n} packs")).toBe("Ett paket");
  expect(i18n.plural(3, "One pack", "{n} packs")).toBe("3 paket");
  expect(i18n.plural(1001, "One pack", "{n} packs")).toBe("1001 paket");
  const english = createPluginI18n(null);
  expect(english.locale).toBe("en");
  expect(english.plural(2, "One pack", "{n} packs")).toBe("2 packs");
});

/** A plugin contribution that reads its plugin's catalog. */
function PacksPanel() {
  const { t } = useTranslation();
  return (
    <section aria-label={t("Game packs")}>
      <Trans>Game packs</Trans>
      <p>
        <Trans message="Drop a pack on {library}" values={{ library: <b>Bibliotek</b> }} />
      </p>
    </section>
  );
}

it("useTranslation and Trans read the plugin's catalog inside its contributions (#219)", () => {
  render(
    <PluginScope.Provider value={{ ...context(), i18n: createPluginI18n(swedish) }}>
      <PacksPanel />
    </PluginScope.Provider>,
  );
  const panel = screen.getByRole("region", { name: "Spelpaket" });
  expect(panel.textContent).toBe("SpelpaketSläpp ett paket på Bibliotek");
  expect(panel.querySelector("b")?.textContent).toBe("Bibliotek");
  cleanup();
  // Outside a plugin, English.
  render(<PacksPanel />);
  expect(screen.getByRole("region", { name: "Game packs" })).toBeTruthy();
});
