/** First-run setup's work, after the choices: saves them, then finds games,
 * emulators, ROMs and RomM's library, reporting each task as it goes. Plain
 * async code over the app's APIs, so tests drive it with fixtures. */
import { plural, t } from "@lingui/core/macro";
import type { DiscoveryApi, DiscoverySnapshot } from "../api/discovery";
import type { EmulationApi, EmulationDetection } from "../api/emulation";
import { formatSource } from "../api/presentation";
import { errorMessage } from "../api/reply";
import { emptyServer, type RommApi, type RommAuth, type RommServer, serverId } from "../api/romm";
import type { SettingsApi } from "../api/settings";
import type { Cue } from "./sound";

export interface RommChoice {
  url: string;
  /** A username and password, or an API token made in RomM. */
  auth: RommAuth;
  /** Only with `password`. */
  username: string;
  /** The password or the token; it goes to the keychain. */
  secret: string;
}

export const emptyRommChoice: RommChoice = { url: "", auth: "password", username: "", secret: "" };

export interface Choices {
  /** Find games installed by Steam, Heroic and the rest. */
  pcGames: boolean;
  /** Keep the play time those launchers know. */
  playTime: boolean;
  /** A ROM collection to set up, or none. */
  romFolder: string | null;
  romm: RommChoice | null;
  /** Fetch artwork and game info in the background. */
  online: boolean;
}

export const defaultChoices: Choices = {
  pcGames: true,
  playTime: true,
  romFolder: null,
  romm: null,
  online: true,
};

export type TaskId = "games" | "emulators" | "roms" | "romm" | "artwork";
export type TaskState = "waiting" | "running" | "done" | "warning" | "failed";
export interface Task {
  id: TaskId;
  label: string;
  state: TaskState;
  detail: string;
}

export interface SetupApis {
  settings: SettingsApi;
  discovery: DiscoveryApi;
  emulation: EmulationApi;
  romm: RommApi;
}

export interface RunOptions {
  update(tasks: Task[]): void;
  cue(cue: Cue): void;
  /** Between polls of running work. */
  pollMs?: number;
  signal?: AbortSignal;
}

/** The tasks a set of choices runs, in order, labelled in the language at
 * the time (setup's first question chose it). */
export function plannedTasks(choices: Choices): Task[] {
  const task = (id: TaskId, label: string): Task => ({ id, label, state: "waiting", detail: "" });
  return [
    choices.pcGames && task("games", t`Games on this PC`),
    choices.romFolder && task("emulators", t`Emulators`),
    choices.romFolder && task("roms", t`Retro games`),
    choices.romm && task("romm", "RomM"),
    choices.online && task("artwork", t`Artwork and game info`),
  ].filter((value): value is Task => Boolean(value));
}

const gameCount = (total: number) => plural({ total }, { one: "# game", other: "# games" });
const noEmulator = (platforms: string[]) =>
  t`No emulator for ${{ platforms: platforms.join(", ") }}`;

async function updateSection(
  api: SettingsApi,
  namespace: string,
  patch: Record<string, unknown>,
): Promise<void> {
  const current = await api.get(namespace);
  await api.set(namespace, { ...(current.value as object), ...patch }, current.revision);
}

/** Saves the choices that are settings. Setup counts as done from here:
 * the first scan and the online lookups may start. */
export async function saveChoices(api: SettingsApi, choices: Choices): Promise<void> {
  await updateSection(api, "core.discovery", { enabled: choices.pcGames });
  // The play history section belongs to discovery's sources; without it
  // there is nothing to keep.
  await updateSection(api, "discovery.import", { seedPlaytime: choices.playTime }).catch(() => {});
  await updateSection(api, "core.metadata", { online: choices.online });
}

export async function markCompleted(api: SettingsApi): Promise<void> {
  await updateSection(api, "core.onboarding", { completed: true });
}

const sleep = (ms: number, signal?: AbortSignal) =>
  new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener("abort", () => {
      clearTimeout(timer);
      resolve();
    });
  });

async function until<T>(
  read: () => Promise<T>,
  done: (value: T) => boolean,
  seen: (value: T) => void,
  options: RunOptions,
): Promise<T> {
  for (;;) {
    const value = await read();
    seen(value);
    if (done(value) || options.signal?.aborted) return value;
    await sleep(options.pollMs ?? 400, options.signal);
  }
}

const finished = (snapshot: DiscoverySnapshot) =>
  ["complete", "failed", "cancelled"].includes(snapshot.phase);
const detected = (detection: EmulationDetection) =>
  ["completed", "cancelled", "failed"].includes(detection.status);

export async function runSetup(apis: SetupApis, choices: Choices, options: RunOptions) {
  const tasks = plannedTasks(choices);
  const set = (id: TaskId, state: TaskState, detail = "") => {
    const task = tasks.find((row) => row.id === id);
    if (!task || (task.state === state && task.detail === detail)) return;
    task.state = state;
    task.detail = detail;
    options.update(tasks.map((row) => ({ ...row })));
  };
  options.update(tasks.map((row) => ({ ...row })));
  await saveChoices(apis.settings, choices);

  // The discovery scan starts before setup counts as done, so the core's
  // own first scan doesn't run a second one.
  let scan: Promise<void> = Promise.resolve();
  if (choices.pcGames) {
    set("games", "running", t`Looking…`);
    const announced = new Set<string>();
    const show = (snapshot: DiscoverySnapshot) => {
      const found = snapshot.sources.filter((source) => source.count > 0);
      for (const source of found) {
        if (announced.has(source.source)) continue;
        announced.add(source.source);
        options.cue("found");
      }
      const listed = found.map((source) => `${formatSource(source.source)} ${source.count}`);
      if (!finished(snapshot)) set("games", "running", listed.join(" · ") || t`Looking…`);
    };
    scan = apis.discovery
      .scan()
      .catch(() => apis.discovery.status())
      .then(() =>
        until(() => apis.discovery.status(), finished, show, options).then((snapshot) => {
          if (snapshot.phase === "failed")
            set("games", "failed", snapshot.error ?? t`The scan didn't finish.`);
          else if (snapshot.found === 0) set("games", "warning", t`None found`);
          // Found but none added: they wait for review in Settings.
          else if (snapshot.imported === 0) set("games", "warning", gameCount(snapshot.found));
          // What was added, as the library counts it: games waiting for
          // review aren't.
          else set("games", "done", gameCount(snapshot.imported));
        }),
      )
      .catch((error) => set("games", "failed", errorMessage(error)));
  }
  await markCompleted(apis.settings);
  if (choices.online) set("artwork", "running", t`Covers fill in as games arrive`);

  if (choices.romFolder) await retroGames(apis.emulation, choices.romFolder, set, options);
  if (choices.romm) await romm(apis.romm, choices.romm, set, options);
  await scan;
  if (choices.online) set("artwork", "done", t`Keeps going in the background`);
  return tasks;
}

async function retroGames(
  api: EmulationApi,
  folder: string,
  set: (id: TaskId, state: TaskState, detail?: string) => void,
  options: RunOptions,
) {
  set("emulators", "running", t`Looking…`);
  try {
    const announced = new Set<string>();
    const show = (detection: EmulationDetection) => {
      for (const candidate of detection.candidates) {
        if (announced.has(candidate.id)) continue;
        announced.add(candidate.id);
        options.cue("found");
      }
      if (!detected(detection))
        set("emulators", "running", names(detection.candidates) || t`Looking…`);
    };
    const started = await api.detect(null);
    const detection = detected(started)
      ? started
      : await until(() => api.detection(), detected, show, options);
    show(detection);
    const fresh = detection.candidates.filter((candidate) => !candidate.configuredAs);
    if (fresh.length > 0)
      await api.importDetected(
        fresh.map((candidate) => candidate.id),
        detection.revision,
      );
    const all = detection.candidates;
    if (all.length === 0) set("emulators", "warning", t`None found`);
    else set("emulators", "done", names(all));
  } catch (error) {
    set("emulators", "failed", errorMessage(error));
  }

  set("roms", "running", t`Sorting your folders…`);
  try {
    const setup = await api.setupRomFolder(folder);
    if (setup.added.length === 0 && setup.existing === 0) {
      set(
        "roms",
        "warning",
        setup.noEmulator.length > 0
          ? noEmulator(setup.noEmulator)
          : t`No console folders found (like snes or psx)`,
      );
      return;
    }
    set("roms", "running", setup.added.map((scanner) => scanner.name).join(" · ") || t`Scanning…`);
    const summaries = await api.scanAll();
    const games = summaries.reduce((sum, summary) => sum + summary.games.length, 0);
    if (games > 0) options.cue("found");
    const missing = setup.noEmulator.length > 0 ? ` · ${noEmulator(setup.noEmulator)}` : "";
    set("roms", setup.noEmulator.length > 0 ? "warning" : "done", `${gameCount(games)}${missing}`);
  } catch (error) {
    set("roms", "failed", errorMessage(error));
  }
}

const names = (candidates: { name: string }[]) =>
  [...new Set(candidates.map((candidate) => candidate.name))].join(" · ");

/** The server a RomM choice saves: named after its host, with RomM's
 * defaults (every platform, synced at startup and every 6 hours). */
export function rommServer(choice: RommChoice, taken: string[]): RommServer {
  const url = /^https?:\/\//.test(choice.url.trim())
    ? choice.url.trim()
    : `https://${choice.url.trim()}`;
  let name = "RomM";
  try {
    name = new URL(url).hostname;
  } catch {
    // The core rejects the address with a clearer message.
  }
  return {
    ...emptyServer(serverId(name, taken)),
    name,
    url,
    auth: choice.auth,
    username: choice.auth === "password" ? choice.username.trim() : "",
  };
}

async function romm(
  api: RommApi,
  choice: RommChoice,
  set: (id: TaskId, state: TaskState, detail?: string) => void,
  options: RunOptions,
) {
  set("romm", "running", t`Connecting…`);
  try {
    const overview = await api.overview();
    const server = rommServer(
      choice,
      overview.settings.servers.map((row) => row.id),
    );
    await api.saveServer(server, choice.secret.trim() || null);
    set("romm", "running", t`Syncing…`);
    const [report] = await api.sync(server.id);
    const games = (report?.added ?? 0) + (report?.updated ?? 0) + (report?.unchanged ?? 0);
    if (games > 0) options.cue("found");
    set("romm", "done", gameCount(games));
  } catch (error) {
    set("romm", "failed", errorMessage(error));
  }
}
