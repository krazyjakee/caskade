/** Settings' General, Launchers, Appearance, Installer, Plugins, Accounts,
 * My profile and About tabs, and About's Legal view, in the pseudo-locale
 * (#217): every text, aria-label, title and placeholder they show comes from
 * a catalog, and a row's control is still labelled by its translated row.
 * Licence texts and the published documents stay in English. */
import { loadCatalog, pseudoLocale } from "@caskade/i18n";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { FixtureAccount } from "../../api/accountsFixtures";
import { fixtureCatalogueItems } from "../../api/addonsFixtures";
import { type AppUpdateApi, AppUpdateProvider, type AppUpdateStatus } from "../../api/appUpdate";
import {
  type PluginManagerApi,
  PluginManagerProvider,
  type PluginSummary,
} from "../../api/pluginManager";
import { renderApp } from "../../test/harness";
import { untranslated } from "../../test/untranslated";

vi.setConfig({ testTimeout: 20_000 });

afterEach(cleanup);

const locale = pseudoLocale ?? "en-XA";

/** Names, brands and fixture text: data, not interface. */
const data = [
  // Devices (#231): their names, operating systems and addresses.
  /Den-PC|Steam Deck|Office laptop|Work PC|Living room|Kids' PC/g,
  /Linux|Windows|macOS/g,
  /\d+\.\d+\.\d+\.\d+:\d+/g,
  // Dates and times, which the pseudo-locale formats as English.
  /\b(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{1,2}, \d{4}, \d{1,2}:\d{2}\s?[AP]M/g,
  /Caskade/g,
  /CASKADE/g,
  /IGDB/g,
  /Hasheous/g,
  /igdb\.com/g,
  /hasheous\.org/g,
  /github\.com\/mtkennerly\/ludusavi-manifest/g,
  /Ludusavi manifest|PCGamingWiki|pcgamingwiki\.com/g,
  // Launchers and rating boards.
  /Steam|Heroic|Lutris|Epic Games|GOG Galaxy|EA App|Ubisoft Connect|Battle\.net|itch\.io|Amazon Games/g,
  /PEGI|ESRB|USK|CERO|ACB|GRAC|ClassInd/g,
  // The built-in theme.
  /Nexus/g,
  "Caskade's dark sci-fi look: indigo and cyan accents, glass panels, HUD frames and a drifting grid.",
  // A CSS example, as code.
  /\[data-cs="game-card"\][\s\S]*\}/g,
  // Accounts and plugins.
  /Ada|Kid|Player/g,
  "Playtime stats",
  "Charts of your playtime.",
  "HowLongToBeat",
  /dev\.example\.\w+/g,
  "Faster library scans.",
  // A plugin's permissions name its hosts and folders.
  /example\.com/g,
  /\/data\b/g,
  // The language list names each language in itself.
  /English|Svenska|Pseudo-Accents/g,
  // The add-on catalogue's entries (the harness's fixture catalogue).
  ...fixtureCatalogueItems().flatMap(({ entry }) => [entry.name, entry.summary, entry.description]),
];

const status: AppUpdateStatus = {
  currentVersion: "0.1.0",
  unavailable: null,
  phase: "available",
  release: { version: "0.2.0", notes: "Faster library scans.", date: null },
  checkedAt: null,
  downloaded: 0,
  total: null,
  error: null,
};
const appUpdate: AppUpdateApi = {
  status: async () => status,
  check: async () => status,
  install: async () => status,
  restart: async () => {},
  subscribe: () => () => {},
};

function plugin(overrides: Partial<PluginSummary>): PluginSummary {
  return {
    settingsError: null,
    id: "dev.example.stats",
    name: "Playtime stats",
    version: "0.1.0",
    description: "Charts of your playtime.",
    author: null,
    folder: "/plugins/dev.example.stats",
    status: "enabled",
    errors: [],
    permissions: { network: ["example.com"], fs: ["/data:ro"], process: true, library: "read" },
    contributes: {
      slots: [],
      views: [],
      hooks: [],
      providers: [],
      commands: [],
      settings: null,
      emulation: [],
    },
    hasUi: true,
    hasBackend: false,
    failures: 0,
    lastError: null,
    ...overrides,
  } as PluginSummary;
}
const plugins = [
  plugin({}),
  plugin({ id: "dev.example.hltb", name: "HowLongToBeat", status: "needs_consent", dev: true }),
];
const pluginManager: PluginManagerApi = {
  list: async () => structuredClone(plugins),
  setEnabled: async (id) => plugins.find((p) => p.id === id) as PluginSummary,
  chooseFolder: async () => null,
  preview: async () => ({ plugin: plugins[1] as PluginSummary, installedVersion: null }),
  install: async () => plugins[1] as PluginSummary,
  uninstall: async () => {},
};

const family: FixtureAccount[] = [
  { id: "ada", name: "Ada", role: "admin", avatar: "avatar:rocket", pin: "2468" },
  {
    id: "kid",
    name: "Kid",
    avatar: "avatar:moon",
    permissions: { settings: ["appearance"], install: true },
    maxAge: 12,
  },
];

const wrap = (children: ReactNode) => (
  <AppUpdateProvider api={appUpdate}>
    <PluginManagerProvider api={pluginManager}>{children}</PluginManagerProvider>
  </AppUpdateProvider>
);

async function open(path: string) {
  renderApp(path, {
    locale,
    messages: await loadCatalog(locale),
    accounts: family,
    activeAccount: "ada",
    wrap,
  });
}

const one = (selector: string) => document.querySelector(selector);
const shown = (selector: string, options?: Parameters<typeof untranslated>[1]) =>
  untranslated([one('[data-view="settings"]'), ...document.querySelectorAll(selector)], {
    data,
    ...options,
  });

it.each([
  ["general", '[data-settings="general"] [data-cs="settings-section"]'],
  ["launchers", '[data-settings="launchers"] [data-part="launcher"]'],
  ["appearance", '[data-settings="appearance"] [role="radio"]'],
  ["installer", '[data-settings="core.installer"]'],
  ["plugins", '[data-part="plugin"][data-status="needs_consent"]'],
  ["accounts", '[data-settings="accounts"] [data-part="account"]'],
  ["devices", '[data-cs="paired-device"]'],
  ["profile", '[data-settings="profile"]'],
  ["about", '[data-cs="about"]'],
])("shows the %s tab in the pseudo-locale", async (tab, ready) => {
  await open(`/settings?tab=${tab}`);
  await waitFor(() => expect(one(ready)).not.toBeNull());
  const skip =
    tab === "installer"
      ? // Rows filled in from the core's schema are its to translate (#214).
        '[data-cs="setting-row"]'
      : tab === "plugins"
        ? // Plugins' own settings pages are the plugin host's (plugins/PluginViews.tsx).
          '[role="tabpanel"] > div > :not(section)'
        : undefined;
  // Checked once, after its data (`ready`): a check that may pass early
  // would pass on a tab still loading.
  expect(shown('[role="tabpanel"]', { skip })).toEqual([]);
});

it("labels a row's control with the translated row", async () => {
  await open("/settings?tab=general");
  const label = await waitFor(() => {
    const element = one('[data-settings="general"] [data-cs="setting-row"] [data-part="label"]');
    expect(element?.textContent).toMatch(/^\[.+\]$/);
    return element as HTMLElement;
  });
  const row = label.closest('[data-cs="setting-row"]') as HTMLElement;
  const description = within(row).getByText(/^\[.+\]$/, {
    selector: '[data-part="description"]',
  });
  const control = within(row).getByRole("switch", { name: label.textContent ?? "" });
  expect(control.getAttribute("aria-describedby")).toContain(description.id);
});

it("shows a plugin's review in the pseudo-locale", async () => {
  await open("/settings?tab=plugins");
  const row = await waitFor(() => {
    const element = one('[data-part="plugin"][data-status="needs_consent"]');
    expect(element).not.toBeNull();
    return element as HTMLElement;
  });
  const buttons = within(row).getAllByRole("button");
  fireEvent.click(buttons.at(-1) as HTMLElement);
  await waitFor(() => expect(one('[data-cs="plugin-review"]')).not.toBeNull());
  expect(untranslated([one('[data-cs="plugin-review"]')], { data })).toEqual([]);
});

it("shows the account editor and the delete confirmation in the pseudo-locale", async () => {
  await open("/settings?tab=accounts");
  await waitFor(() => expect(document.querySelectorAll('[data-part="account"]')).toHaveLength(2));
  const kid = document.querySelectorAll('[data-part="account"]')[1] as HTMLElement;
  fireEvent.click(kid.querySelector('[data-part="edit"]') as HTMLElement);
  await waitFor(() =>
    expect(one('[data-cs="account-editor"] [data-part="access"]')).not.toBeNull(),
  );
  expect(untranslated([one('[data-cs="account-editor"]')], { data })).toEqual([]);
  // Escape closes the editor from inside it: wait for the dialog to take
  // focus, which it does a moment after it opens.
  await waitFor(() =>
    expect(one('[data-cs="account-editor"]')?.contains(document.activeElement)).toBe(true),
  );
  fireEvent.keyDown(document.activeElement as HTMLElement, { key: "Escape" });
  await waitFor(() => expect(one('[data-cs="account-editor"]')).toBeNull());

  fireEvent.click(kid.querySelector('[data-part="delete"]') as HTMLElement);
  const dialog = await screen.findByRole("alertdialog");
  expect(untranslated([dialog], { data })).toEqual([]);
});

it.each(["licenses", "packages", "terms", "privacy"])(
  "shows Legal's %s around its English documents",
  async (section) => {
    await open(`/settings?tab=about&section=${section}`);
    await waitFor(() => expect(one('[data-cs="legal"] [data-part="nav"]')).not.toBeNull());
    await waitFor(() =>
      expect(one('[data-cs="legal"] [data-part="document"], [data-cs="legal"] h3')).not.toBeNull(),
    );
    // The documents and the packages' licences stay as published; their
    // labels are translated.
    expect(
      shown('[data-cs="legal"]', {
        skip: '[data-part="document"] *, [data-cs="legal"] li li, [aria-expanded] span.font-mono',
      }),
    ).toEqual([]);
  },
);
