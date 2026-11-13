/** Activity in the pseudo-locale (#9): its jobs and notifications, install
 * requests, a job's question and a refresh's card. */
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, it } from "vitest";
import type { JobQuestion } from "../api/install";
import { fixtureJob } from "../api/installFixtures";
import type { InstallRequest, InstallRequestApi } from "../api/installRequests";
import { NotificationsProvider } from "../api/notifications";
import { createNotificationsFixtureApi, fixtureNotifications } from "../api/notificationsFixtures";
import { FIXTURE_LINK, fixtureShareInfo } from "../api/sharesFixtures";
import { createStorageFixtureApi } from "../api/storageFixtures";
import { InstallIngress } from "../views/activity/InstallIngress";
import {
  devices,
  dialogOf,
  modes,
  one,
  open,
  over,
  paths,
  ready,
  shown,
  strings,
} from "./pseudoHarness";

// Activity.

it("shows Activity's jobs in the pseudo-locale", async () => {
  const job = fixtureJob();
  await open("/activity");
  await waitFor(() =>
    expect(one('[data-view="activity"]')?.textContent).toContain("Fixture installation"),
  );
  // The job's own words are the core's (#214).
  const data = strings([
    job.title,
    job.message,
    job.targetDir,
    ...(job.warnings ?? []).map((warning) => warning.message),
    ...(job.dependencies ?? []).map((dependency) => dependency.name),
    ...(job.strategies ?? []).flatMap((strategy) => [strategy.label, strategy.reason]),
  ]);
  expect(shown(['[data-view="activity"]'], [...data, ...paths])).toEqual([]);
});

it("shows Activity's notifications in the pseudo-locale", async () => {
  await open("/activity?tab=notifications", {
    wrap: (children: ReactNode) => (
      <NotificationsProvider api={createNotificationsFixtureApi()}>
        {children}
      </NotificationsProvider>
    ),
  });
  await waitFor(() =>
    expect(one('[data-view="activity"]')?.textContent).toContain("10 games added"),
  );
  // Titles and bodies are the core's (#214).
  const data = strings(
    fixtureNotifications().flatMap((item) => [
      item.title,
      item.body,
      ...item.sources.map((source) => source.source),
    ]),
  );
  expect(shown(['[data-view="activity"]'], data)).toEqual([]);
});

// Install requests. The app mounts these beside the router.

const requestsApi = (pending: InstallRequest[]): InstallRequestApi => ({
  pending: async () => structuredClone(pending),
  request: async () => pending[0] as InstallRequest,
  requestDrop: async () => pending[0] as InstallRequest,
  confirm: async () => "fixture-job",
  confirmCollection: async () => {},
  dismiss: async () => {},
  subscribe: async () => () => {},
});

const withRequests = (api: InstallRequestApi) => (children: ReactNode) => (
  <>
    {children}
    <InstallIngress api={api} />
  </>
);

const dropped = (kind: "shortcut" | "executable" | "folder" | "file"): InstallRequest => ({
  requestId: kind,
  kind: "path",
  value: "/home/me/game",
  plan: {
    kind,
    title: "Fixture Game 0002",
    dropped: "/home/me/game.desktop",
    input: "/home/me/game",
    launch: { path: "/home/me/game/run.sh", args: ["--windowed"], workingDir: null },
  },
});

const peer = (sizeBytes: number | null): InstallRequest => ({
  requestId: "peer",
  kind: "peer",
  value: "Fixture Game 0002",
  peer: {
    deviceId: "b".repeat(64),
    deviceName: "Steam Deck",
    entryId: "entry-1",
    gameId: null,
    title: "Fixture Game 0002",
    sizeBytes,
  },
});

const shared = (share = fixtureShareInfo()): InstallRequest => ({
  requestId: "share",
  kind: "share",
  value: FIXTURE_LINK,
  share,
});

const installRequests: [string, InstallRequest][] = [
  ["a link", { requestId: "url", kind: "url", value: "https://example.com/game.zip" }],
  ["a path", { requestId: "path", kind: "path", value: "/home/me/game.zip" }],
  ["a dropped shortcut", dropped("shortcut")],
  ["a dropped program", dropped("executable")],
  ["a dropped folder", dropped("folder")],
  ["a dropped file", dropped("file")],
  ["a copy from another device", peer(1_200_000_000)],
  ["a copy of unknown size", peer(null)],
  ["a shared game", shared()],
  [
    "a shared save",
    shared(
      fixtureShareInfo({ contents: { kind: "save", game: "Hades", here: "hades", files: 3 } }),
    ),
  ],
  [
    "a shared web game",
    shared(
      fixtureShareInfo({ contents: { kind: "web", url: "https://example.com/game", here: null } }),
    ),
  ],
  [
    "a shared ROM",
    shared(
      fixtureShareInfo({
        contents: { kind: "rom", platform: "Super Nintendo", emulator: "RetroArch", ready: false },
      }),
    ),
  ],
  [
    "a shared collection",
    shared(
      fixtureShareInfo({
        title: "Co-op nights",
        contents: {
          kind: "collection",
          name: "Co-op nights",
          games: [
            { title: "Hades", igdbId: "113112", here: "installed", gameId: "fixture-1" },
            { title: "It Takes Two", igdbId: "119171", here: "missing", gameId: null },
          ],
        },
      }),
    ),
  ],
];

const sharedWords = /Alex's Desk|Hades|Co-op nights|It Takes Two|Super Nintendo|RetroArch/g;

it.each(installRequests)("asks before installing %s in the pseudo-locale", async (_, request) => {
  await open("/", {
    storage: createStorageFixtureApi(),
    wrap: withRequests(requestsApi([request])),
  });
  await dialogOf("install-confirmation");
  const data = [/https?:\/\/\S+/g, ...paths, devices, sharedWords, "--windowed"];
  expect(over(data)).toEqual([]);
});

it("shows console mode's hints over an install request in the pseudo-locale", async () => {
  await open("/", {
    mode: "console",
    wrap: withRequests(requestsApi(installRequests.map(([, request]) => request).slice(0, 1))),
  });
  await dialogOf("install-confirmation");
  await ready('[data-cs="hint-bar"] [data-part="hint"]');
  expect(over([/https?:\/\/\S+/g, ...paths])).toEqual([]);
});

it("says when install requests can't be read in the pseudo-locale", async () => {
  const api = { ...requestsApi([]), pending: () => Promise.reject("gone") };
  await open("/", { wrap: withRequests(api) });
  await dialogOf("install-input-error");
  expect(shown(['[data-cs="install-input-error"]'])).toEqual([]);
});

const question: JobQuestion = {
  id: "7",
  kind: "launch_target",
  title: "Choose a launch target",
  message: "Several executable files were found.",
  choices: [
    { id: "0", label: "Fixture game", detail: "Game.exe" },
    { id: "1", label: "Level editor", detail: "Editor.exe" },
  ],
};

it.each(modes)("shows a job's question in the pseudo-locale (%s)", async (mode) => {
  // To answer, only to read, and with nothing to choose.
  for (const asked of [question, { ...question, canAnswer: false }, { ...question, choices: [] }]) {
    await open("/activity", {
      mode,
      jobs: [fixtureJob({ status: "needs_user", question: asked })],
    });
    fireEvent.click(await screen.findByRole("button", { name: /Ŕēvĩēŵ ćĥōĩćēś/ }));
    await dialogOf("job-question");
    // The job's words are the core's (#214).
    const data = strings([
      question.title,
      question.message,
      ...question.choices.flatMap((choice) => [choice.label, choice.detail]),
    ]);
    expect(over(data)).toEqual([]);
    cleanup();
  }
});

it("shows a refresh's card in Activity in the pseudo-locale", async () => {
  const refreshing = '[data-cs="games-refresh"]';
  const status = () => one(refreshing)?.getAttribute("data-status");
  // A run, how it ends with a game that failed, and a cancelled one.
  const app = await open("/activity");
  await ready('[data-view="activity"]');
  await act(() => app.library.edits.refreshStart(["fixture-1", "fail-2"], "missing"));
  await waitFor(() => expect(status()).toBe("running"));
  expect(shown([refreshing])).toEqual([]);
  act(() => app.library.edits.finishRefresh());
  await waitFor(() => expect(status()).toBe("done"));
  await ready(`${refreshing} [data-part="retry"]`);
  expect(shown([refreshing])).toEqual([]);
  cleanup();

  const cancelled = await open("/activity");
  await ready('[data-view="activity"]');
  await act(() => cancelled.library.edits.refreshStart(["fixture-1", "fixture-2"], "automatic"));
  await waitFor(() => expect(status()).toBe("running"));
  await act(() => cancelled.library.edits.refreshCancel().then(() => {}));
  act(() => cancelled.library.edits.finishRefresh());
  await waitFor(() => expect(status()).toBe("cancelled"));
  await ready(`${refreshing} [data-part="continue"]`);
  expect(shown([refreshing])).toEqual([]);
});
