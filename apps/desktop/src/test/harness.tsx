/** The whole app for action-driven tests (#148, #150, #151, #153): the
 * real routes and shells over fixture APIs, a fake window, a console-mode
 * API that records its calls, and a pad the test presses. */

import { i18n, sourceLocale } from "@caskade/i18n";
import { HostScope } from "@caskade/sdk";
import type { Messages } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { act, render, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { vi } from "vitest";
import { createAccountsFixtureApi, type FixtureAccount } from "../api/accountsFixtures";
import { createEmulationFixtureApi } from "../api/emulationFixtures";
import { createFixtureApi, createFixtureGames } from "../api/fixtures";
import type { JobSnapshot } from "../api/install";
import { createInstallFixtureApi, fixtureJob } from "../api/installFixtures";
import { createSettingsLocaleApi } from "../api/locale";
import { createLibraryQueryClient, LibraryProvider } from "../api/provider";
import { StorageProvider } from "../api/StorageProvider";
import type { StorageApi } from "../api/storage";
import type { GameDetails } from "../api/types";
import type { Action, GlyphFamily, PadAction } from "../input";
import { PadSource } from "../input";
import { LocaleProvider } from "../locale/LocaleProvider";
import { host } from "../plugins/PluginProvider";
import { PluginRegistry, PluginRegistryContext } from "../plugins/registry";
import { routeTree } from "../router";
import { type AppWindow, AppWindowContext } from "../shell/appWindow";
import { type ConsoleApi, type Mode, ModeProvider } from "../shell/console/mode";
import { fakeWindow } from "../shell/windowFixture";
import { ActivityProvider } from "../views/activity/ActivityProvider";
import { DiscoveryProvider } from "../views/settings/discovery/DiscoveryProvider";
import { EmulationProvider } from "../views/settings/emulators/EmulationProvider";
import { DefaultSettingsApi } from "../views/settings/useSettings";
import { createDiscoveryFixture, createSettingsFixture } from "./settingsFixture";

const refuse = (name: string) => async () => Promise.reject(new Error(`unexpected ${name}`));

/** The element with focus. */
export const focused = () => document.activeElement as HTMLElement | null;

/** Waits for `check`; a timeout says what it waited for and where focus is. */
export const until = (check: () => boolean, what: string) =>
  waitFor(() => {
    if (!check())
      throw new Error(`waiting for ${what}; focus is on ${focused()?.outerHTML.slice(0, 160)}`);
  });

/** No storage roots. */
export const emptyStorage: StorageApi = {
  list: async () => [],
  preferences: async () => ({ alwaysUseDefault: false }),
  add: refuse("add"),
  edit: refuse("edit"),
  setDefault: refuse("setDefault"),
  remove: refuse("remove"),
  chooseDirectory: async () => null,
  move: refuse("move"),
  uninstall: refuse("uninstall"),
};

/** A pad whose presses reach the app as the core's `inputActionEvent`s. */
export function fakePad(glyphs: GlyphFamily = "xbox") {
  let send: (action: PadAction) => void = () => {};
  return {
    subscribe: async (listener: (action: PadAction) => void) => {
      send = listener;
      return () => {};
    },
    press: (action: Action, family: GlyphFamily = glyphs) =>
      act(() => {
        send({ action, glyphs: family });
      }),
  };
}

/** Records console mode switches and can announce one from "the core". */
export function fakeConsoleApi() {
  const calls: Mode[] = [];
  let announce: (mode: Mode) => void = () => {};
  const api: ConsoleApi = {
    initial: async () => "desktop",
    set: async (mode) => {
      calls.push(mode);
    },
    subscribe: async (listener) => {
      announce = listener;
      return () => {};
    },
  };
  return { api, calls, announce: (mode: Mode) => act(() => announce(mode)) };
}

/** Fixture games, each played a day after the one before (the last most
 * recently), so Home's Recently Played has them all. */
export function playedGames(count = 8): GameDetails[] {
  return createFixtureGames(count).map((game, index) => ({
    ...game,
    lastPlayedAt: new Date(Date.UTC(2026, 9, 1 + index)).toISOString(),
  }));
}

export interface AppOptions {
  mode?: Mode;
  games?: GameDetails[];
  frame?: AppWindow["frame"];
  registry?: PluginRegistry;
  /** Install jobs in Activity. */
  jobs?: JobSnapshot[];
  /** Storage locations; none by default, and every change refused. */
  storage?: StorageApi;
  /** Extra providers around the router (settings, emulation, …). */
  wrap?: (children: ReactNode) => ReactNode;
  /** Local accounts; one admin without a PIN (signed in) by default. */
  accounts?: FixtureAccount[];
  /** Who is signed in at start; by default the core's rule (a single
   * account without a PIN, otherwise nobody). */
  activeAccount?: string | null;
  /** The language; English by default. Its messages come from `messages`
   * (say, a test PO file imported directly) or `loadCatalog(locale)`. */
  locale?: string;
  messages?: Messages;
  /** The OS's languages, for the Language setting's System; `en-US` by
   * default. The setting switches the language as the core would. */
  systemLanguages?: string[];
}

export function renderApp(path: string, options: AppOptions = {}) {
  const library = createFixtureApi(options.games ?? createFixtureGames(8));
  const accounts = createAccountsFixtureApi(options.accounts, library.emit, {
    activeId: options.activeAccount,
  });
  library.accounts = accounts;
  const settings = createSettingsFixture();
  const locale = createSettingsLocaleApi({
    settings: settings.api,
    system: () => options.systemLanguages ?? ["en-US"],
  });
  const emulation = createEmulationFixtureApi();
  const installs = createInstallFixtureApi(options.jobs ?? [fixtureJob()]);
  const pad = fakePad();
  const console = fakeConsoleApi();
  const window = { ...fakeWindow(), frame: options.frame ?? "custom" };
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const app = <RouterProvider router={router} />;
  i18n.loadAndActivate({
    locale: options.locale ?? sourceLocale,
    messages: options.messages ?? {},
  });
  const view = render(
    <I18nProvider i18n={i18n}>
      <LocaleProvider api={locale}>
        <LibraryProvider api={library} client={createLibraryQueryClient()}>
          <StorageProvider api={options.storage ?? emptyStorage}>
            <ActivityProvider api={installs} storageApi={options.storage ?? emptyStorage}>
              <PluginRegistryContext.Provider value={options.registry ?? new PluginRegistry()}>
                <ModeProvider api={console.api} initial={options.mode ?? "desktop"}>
                  <AppWindowContext value={window}>
                    <DefaultSettingsApi value={settings.api}>
                      <DiscoveryProvider api={createDiscoveryFixture()}>
                        <EmulationProvider api={emulation}>
                          <PadSource value={pad.subscribe}>
                            <HostScope.Provider value={host}>
                              {options.wrap ? options.wrap(app) : app}
                            </HostScope.Provider>
                          </PadSource>
                        </EmulationProvider>
                      </DiscoveryProvider>
                    </DefaultSettingsApi>
                  </AppWindowContext>
                </ModeProvider>
              </PluginRegistryContext.Provider>
            </ActivityProvider>
          </StorageProvider>
        </LibraryProvider>
      </LocaleProvider>
    </I18nProvider>,
  );
  return {
    ...view,
    router,
    pad,
    console,
    library,
    accounts,
    window,
    settings,
    emulation,
    installs,
  };
}

const original = Element.prototype.getBoundingClientRect;
const rect = (x: number, y: number, w: number, h: number) =>
  ({ x, y, left: x, top: y, width: w, height: h, right: x + w, bottom: y + h }) as DOMRect;

/** jsdom has no layout. Elements with `data-rect="x y w h"` get that box;
 * otherwise focusable controls stack in document order, so spatial
 * navigation has something to walk. Returns the undo. */
export function fakeLayout(): () => void {
  // The stacking order, recomputed only when the document changes.
  let order: Map<Element, number> | null = null;
  const observer = new MutationObserver(() => {
    order = null;
  });
  observer.observe(document, { subtree: true, childList: true, attributes: true });
  const indexOf = (element: Element) => {
    if (!order) {
      const all = document.querySelectorAll(
        "a[href], button, input, select, textarea, [tabindex], [role]",
      );
      order = new Map(Array.from(all, (item, index) => [item, index]));
    }
    return order.get(element) ?? -1;
  };
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const own = this.getAttribute("data-rect");
    if (own) {
      const [x, y, w, h] = own.split(" ").map(Number) as [number, number, number, number];
      return rect(x, y, w, h);
    }
    if (this.matches('[role="dialog"], [data-placement], [data-cs="view"]'))
      return rect(0, 0, 1600, 900);
    // A settings row is where its first control is.
    const first = this.matches('[data-cs="setting-row"]')
      ? this.querySelector("a[href], button, input, select, textarea, [tabindex], [role]")
      : null;
    const index = indexOf(first ?? this);
    if (index < 0) return rect(0, 0, 0, 0);
    return rect(300, index * 40, 200, 36);
  };
  return () => {
    observer.disconnect();
    Element.prototype.getBoundingClientRect = original;
  };
}

/** A 900×600 viewport for the virtualized library grid. */
export function fakeViewport() {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(900);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private callback: (entries: unknown[]) => void) {}
      observe(target: Element) {
        this.callback([
          {
            target,
            contentRect: { width: 900, height: 600 },
            borderBoxSize: [{ inlineSize: 900, blockSize: 600 }],
          },
        ]);
      }
      unobserve() {}
      disconnect() {}
    },
  );
  Element.prototype.scrollIntoView = () => {};
}
