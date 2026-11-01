import { isTauri } from "@tauri-apps/api/core";
import { commands, events } from "./bindings";
import { createDiscoveryApi, type DiscoveryApi } from "./discovery";
import { lazyApi } from "./lazyApi";

function native<T>(call: () => Promise<T>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error("Open Caskade desktop to scan installed games."));
  return call();
}
const ipcDiscoveryApi = createDiscoveryApi(
  {
    discoveryStatus: () => native(commands.discoveryStatus),
    discoveryLaunchers: () => native(commands.discoveryLaunchers),
    discoveryScan: () => native(commands.discoveryScan),
    discoveryCancel: () => native(commands.discoveryCancel),
    discoveryReview: (ids, accept, revision) =>
      native(() => commands.discoveryReview(ids, accept, revision)),
  },
  async (listener) =>
    isTauri() ? events.discoveryChanged.listen(({ payload }) => listener(payload)) : () => {},
);
/** Dev builds started with `VITE_CASKADE_FIXTURES=1` show a finished
 * fixture run instead (README.md). */
export const nativeDiscoveryApi: DiscoveryApi =
  import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1"
    ? lazyApi(
        () => import("./discoveryFixtures").then((module) => module.createDiscoveryFixtureApi()),
        ["status", "launchers", "scan", "cancel", "review", "subscribe"],
      )
    : ipcDiscoveryApi;
