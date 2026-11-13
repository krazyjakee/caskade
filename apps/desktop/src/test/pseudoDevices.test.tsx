/** Devices in the pseudo-locale (#9): pairing, streaming, games moving
 * between devices, add-ons, emulated releases, and what shows when something
 * fails. */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, it, vi } from "vitest";
import type { FixtureAccount } from "../api/accountsFixtures";
import { fixtureCatalogueItems } from "../api/addonsFixtures";
import { createFixtureGames } from "../api/fixtures";
import { type GameEmulation, GameEmulationProvider } from "../api/gameEmulation";
import { StreamingApiProvider } from "../api/streaming";
import { createStreamingFixtureApi } from "../api/streamingFixtures";
import { type SyncApi, SyncProvider, type Transfer } from "../api/sync";
import { createSyncFixtureApi } from "../api/syncFixtures";
import type { GameDetails } from "../api/types";
import { AppErrorBoundary } from "../shell/ErrorFallback";
import {
  all,
  devices,
  dialogOf,
  gameHeading,
  gameMenu,
  managed,
  menuItem,
  modes,
  one,
  open,
  over,
  paths,
  reactAria,
  ready,
  shown,
  strings,
} from "./pseudoHarness";

// Pairing.

const withStreaming = (options: Parameters<typeof createStreamingFixtureApi>[0] = {}) => {
  const api = createStreamingFixtureApi(options);
  return {
    api,
    wrap: (children: ReactNode) => (
      <StreamingApiProvider api={api}>{children}</StreamingApiProvider>
    ),
  };
};

const pairingData = [devices, /\d+\.\d+\.\d+\.\d+(?::\d+)?/g, /Sunshine|Moonlight/g];

const pairStudy = async () => {
  const row = (await screen.findByText("Study")).closest("li") as HTMLElement;
  fireEvent.click(await within(row).findByRole("button", { name: /Ƥàĩŕ/ }));
  await dialogOf("pair-dialog");
};

it.each(["paired", "wrongCode", "declined", "expired", "failed"] as const)(
  "shows pairing to stream ending %s in the pseudo-locale",
  async (outcome) => {
    const { api, wrap } = withStreaming();
    await open("/settings?tab=streaming", { wrap });
    await pairStudy();
    await ready('[data-cs="pair-dialog"] [data-part="code"]');
    expect(over(pairingData)).toEqual([]);
    act(() => api.finishPairing("fixture-study", outcome));
    await waitFor(() =>
      expect(one('[data-cs="pair-dialog"]')?.getAttribute("data-status")).toBe(outcome),
    );
    expect(over(pairingData)).toEqual([]);
  },
);

it("shows pairing a device of your own, and one that can't be asked, in the pseudo-locale", async () => {
  const linked = withStreaming({ linked: ["fixture-study"] });
  await open("/settings?tab=streaming", { wrap: linked.wrap });
  await pairStudy();
  await waitFor(() =>
    expect(one('[data-cs="pair-dialog"]')?.getAttribute("data-status")).toBe("code"),
  );
  expect(over(pairingData)).toEqual([]);
  cleanup();

  const failing = withStreaming();
  vi.spyOn(failing.api, "pairWith").mockRejectedValue("gone");
  await open("/settings?tab=streaming", { wrap: failing.wrap });
  await pairStudy();
  await waitFor(() =>
    expect(one('[data-cs="pair-dialog"]')?.getAttribute("data-status")).toBe("failed"),
  );
  // The reason is the core's (#214).
  expect(over([...pairingData, "gone"])).toEqual([]);
});

it.each(modes)("asks to let a device stream from here in the pseudo-locale (%s)", async (mode) => {
  const { wrap } = withStreaming({
    hosting: { state: "ready", version: "2026.914.233613", problem: null, warning: null },
    requests: [{ id: "r1", name: "Steam Deck", address: "192.168.1.40" }],
  });
  await open("/", { mode, wrap });
  await dialogOf("stream-pair-prompt");
  expect(over(pairingData)).toEqual([]);
});

it("asks whether to use streaming set up by someone else in the pseudo-locale", async () => {
  const { wrap } = withStreaming({ existing: true });
  await open("/settings?tab=streaming", { wrap });
  await ready(
    '[role="tabpanel"] [data-cs="streaming-settings"] [role="switch"], [role="tabpanel"] [data-cs="streaming-settings"] input[role="switch"]',
  );
  fireEvent.click(one('[data-cs="streaming-settings"] [role="switch"]') as Element);
  await dialogOf("streaming-existing");
  expect(over(pairingData)).toEqual([]);
});

/** Each with its name, as the core words it (#214). */
const programs = () => [
  { part: "host" as const, label: "Hosting", installed: "v1.0", latest: "v1.1", update: true },
  { part: "client" as const, label: "Client", installed: "v6", latest: "v6", update: false },
];

it("shows Streaming updates in the pseudo-locale", async () => {
  const { wrap } = withStreaming({
    hosting: { state: "ready", version: "2026.914.233613", problem: null, warning: null },
    programs: programs(),
  });
  await open("/settings?tab=streaming", { wrap });
  await waitFor(() => expect(all('[data-cs="streaming-program"]')).toHaveLength(2));
  expect(shown(['[data-cs="streaming-program"]'], [...pairingData, /Hosting|Client/g])).toEqual([]);
});

const pairing = (state: string, overrides: object = {}) => ({
  deviceId: "9".repeat(64),
  name: "Office laptop",
  kind: "laptop",
  os: "mac",
  code: "123456",
  incoming: true,
  confirmed: false,
  state,
  ...overrides,
});

/** The devices fixture with this pairing under way. */
function pairingWith(current: ReturnType<typeof pairing>): SyncApi {
  const api = createSyncFixtureApi();
  return {
    ...api,
    status: async () => ({ ...(await api.status()), pairing: current }) as never,
  };
}

const withSync = (api: SyncApi) => (children: ReactNode) => (
  <SyncProvider api={api}>{children}</SyncProvider>
);

/** One account, not an admin. */
const standard: FixtureAccount[] = [{ id: "kid", name: "Kid" }];

it.each(modes)("shows pairing a device in the pseudo-locale (%s)", async (mode) => {
  const states = [
    pairing("waiting"),
    pairing("waiting", { incoming: false, confirmed: true }),
    pairing("paired"),
    pairing("cancelled"),
    pairing("failed"),
  ];
  for (const current of states) {
    await open("/", { mode, wrap: withSync(pairingWith(current)) });
    await dialogOf("pair-device");
    expect(over([devices])).toEqual([]);
    cleanup();
  }
});

it("tells someone who isn't an admin who can pair in the pseudo-locale", async () => {
  await open("/", { accounts: standard, wrap: withSync(pairingWith(pairing("waiting"))) });
  await dialogOf("pair-device");
  expect(over([devices])).toEqual([]);
  cleanup();

  const link = createSyncFixtureApi();
  link.openLink({ deviceId: "c".repeat(64), expiresAtMs: Date.UTC(2026, 10, 9), expired: false });
  await open("/", { accounts: standard, wrap: withSync(link) });
  await dialogOf("pair-device");
  expect(over([devices, /\bc{8}\b/g])).toEqual([]);
});

it.each(modes)("asks about a pairing link in the pseudo-locale (%s)", async (mode) => {
  const deviceId = "c".repeat(64);
  const link = /\bc{8}\b/g;
  // One that has expired.
  const old = createSyncFixtureApi();
  old.openLink({ deviceId, expiresAtMs: Date.UTC(2026, 9, 9), expired: true });
  await open("/", { mode, wrap: withSync(old) });
  await dialogOf("pair-device");
  expect(over([devices, link])).toEqual([]);
  cleanup();

  // One to accept.
  const sync = createSyncFixtureApi();
  sync.openLink({ deviceId, expiresAtMs: Date.UTC(2026, 10, 9), expired: false });
  await open("/", { mode, wrap: withSync(sync) });
  await dialogOf("pair-device");
  expect(over([devices, link])).toEqual([]);
  fireEvent.click([...all('[data-cs="pair-device"] button')].at(-1) as Element);
  await waitFor(() => expect(all('[data-cs="pair-device"] button')).toHaveLength(1));
  expect(over([devices, /Office desktop/g])).toEqual([]);
});

it("shows games moving between devices in the pseudo-locale", async () => {
  const transfer = (state: Transfer["state"], direction: Transfer["direction"]): Transfer => ({
    id: `${state}-${direction}`,
    title: "Fixture Game 0002",
    deviceName: "Steam Deck",
    direction,
    doneBytes: 1024 ** 3,
    totalBytes: 2 * 1024 ** 3,
    bytesPerSecond: 20 * 1024 ** 2,
    link: state === "waitingDirect" ? "relay" : "lan",
    state,
  });
  const sync = createSyncFixtureApi();
  sync.setTransfers([
    transfer("running", "incoming"),
    transfer("waitingDirect", "outgoing"),
    transfer("done", "incoming"),
    transfer("failed", "outgoing"),
  ]);
  await open("/settings?tab=devices", { wrap: withSync(sync) });
  await waitFor(() => expect(all('[data-cs="transfer"]')).toHaveLength(4));
  expect(shown(['[data-cs="transfer"]'], [devices])).toEqual([]);
});

// Add-ons and emulated releases.

it.each(modes)("shows an add-on's details in the pseudo-locale (%s)", async (mode) => {
  const items = fixtureCatalogueItems();
  const data = [
    ...strings(
      items.flatMap(({ entry }) => [
        entry.name,
        entry.author,
        entry.summary,
        entry.description,
        entry.license,
        entry.homepage,
      ]),
    ),
    /\d+\.\d+\.\d+/g,
    /https?:\/\/\S+/g,
    /\b(?:\w+\.)*caskade\.\w+/g,
  ];
  await open("/settings?tab=plugins", { mode });
  fireEvent.click(await screen.findByRole("button", { name: /Paper/ }));
  await dialogOf("addon-details");
  expect(over(data)).toEqual([]);
});

/** A release that can't play yet, as the core describes it. */
const emulated: GameEmulation = {
  entryId: "entry-1",
  platformId: "nintendo_snes",
  platform: {
    id: "nintendo_snes",
    name: "Super Nintendo",
    short: "SNES",
    extensions: ["sfc"],
    bios: [],
  },
  target: { type: "platform_default" },
  args: { type: "default" },
  discs: [
    { name: "Disc 1", path: "/roms/Game (Disc 1).cue", exists: true, size: 10 },
    { name: "Disc 2", path: "/roms/Game (Disc 2).cue", exists: false, size: 10 },
  ],
  profile: null,
  compatible: [
    {
      emulatorId: "ra",
      profileId: "snes",
      emulatorName: "RetroArch",
      profileName: "Snes9x",
      available: true,
    },
  ],
  bios: [
    {
      file: "scph5501.bin",
      required: true,
      found: false,
      state: "missing",
      path: null,
      platform: "sony_playstation",
    },
    {
      file: "dc/optional.bin",
      required: false,
      found: null,
      state: null,
      path: null,
      platform: null,
    },
  ],
  readiness: { type: "no_emulator" },
  message: "No emulator for SNES",
};

it.each(modes)("shows an emulated release in the pseudo-locale (%s)", async (mode) => {
  const games = createFixtureGames(2).map((game) => ({
    ...game,
    entries: game.entries.map((entry) => ({ ...entry, emulated: true })),
  }));
  const api = {
    gameEmulation: async () => structuredClone(emulated),
    setGameEmulation: async () => structuredClone(emulated),
    splitArgs: async (text: string) => [text],
  };
  await open("/games/fixture-1", {
    mode,
    games,
    wrap: (children: ReactNode) => (
      <GameEmulationProvider api={api}>{children}</GameEmulationProvider>
    ),
  });
  await gameHeading();
  await dialogOf("emulation");
  const data = [
    /\{\w+\}/g,
    /RetroArch|Snes9x|Super Nintendo|SNES|No emulator for/g,
    /\bDisc \d\b/g,
    /scph5501|optional|\bdc\b/g,
    ...paths,
    ...reactAria,
  ];
  expect(shown(['[data-cs="emulation"]'], data)).toEqual([]);
});

// When things fail.

it.each(modes)("says a view couldn't be shown in the pseudo-locale (%s)", async (mode) => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  const [game] = createFixtureGames(1);
  await open("/games/fixture-0", {
    mode,
    games: [{ ...(game as GameDetails), entries: undefined as never }],
  });
  await dialogOf("view-error");
  // What threw says it in the language it was written in.
  const message = one('[data-cs="view-error"] [data-part="message"]')?.textContent ?? "";
  expect(shown(['[data-cs="view-error"]'], [message])).toEqual([]);
});

function Crash(): ReactNode {
  throw new Error("provider failed");
}

it("says the app crashed in the pseudo-locale", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  await open("/", {
    wrap: () => (
      <AppErrorBoundary>
        <Crash />
      </AppErrorBoundary>
    ),
  });
  await dialogOf("view-error");
  expect(shown(['[data-cs="view-error"]'], ["provider failed"])).toEqual([]);
});

it.each(modes)("says a tool didn't start in the pseudo-locale (%s)", async (mode) => {
  const app = await managed(mode);
  await app.library.actions.save("fixture-1", {
    id: "",
    entryId: "entry-1",
    name: "Mod manager",
    kind: "utility",
    target: { type: "executable", path: "/fixture/mods", args: [], cwd: null, env: {} },
    tracking: { mode: "automatic", folder: null, names: [] },
    proton: null,
  });
  vi.spyOn(app.library.actions, "runTool").mockRejectedValue(new Error("The tool is missing"));
  await gameHeading();
  await gameMenu();
  fireEvent.click(await menuItem(/Mod manager/));
  await dialogOf("tool-failed");
  expect(over(["Mod manager", "The tool is missing"])).toEqual([]);
});
