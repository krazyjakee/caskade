import { t } from "@lingui/core/macro";
import { isTauri } from "@tauri-apps/api/core";
import { commands, events, type JobSnapshot_Serialize } from "./bindings";
import { lazyApi } from "./lazyApi";
import { unwrap } from "./reply";
import type { StorageApi } from "./storage";

export type StorageTransport = Pick<
  typeof commands,
  | "storageList"
  | "storageAdd"
  | "storageEdit"
  | "storageDefault"
  | "storageRemove"
  | "storageMoveAndRemove"
  | "storageChooseDirectory"
  | "moveGame"
  | "uninstallGame"
  | "jobGet"
  | "settingsGet"
>;
/** Keep-files removal and move-all sequencing wait for actual backend jobs,
 * rather than mistaking a queued response for completed filesystem work. */
export function createStorageApi(transport: StorageTransport): StorageApi {
  async function complete(first: JobSnapshot_Serialize) {
    let job = first;
    for (;;) {
      if (job.status === "completed") return;
      if (job.status === "failed" || job.status === "cancelled")
        throw new Error(job.error ?? `Job ${job.status}`);
      if (job.status === "needsUser")
        throw new Error(t`This job needs a response in Activity before it can continue.`);
      await new Promise((resolve) => setTimeout(resolve, 400));
      job = await unwrap(transport.jobGet(job.id));
    }
  }
  return {
    list: () => unwrap(transport.storageList()),
    add: (path, label) => unwrap(transport.storageAdd(path, label ?? null)),
    edit: (id, label, template) => unwrap(transport.storageEdit(id, label || null, template)),
    setDefault: (id) => unwrap(transport.storageDefault(id)),
    remove: async (id) => {
      await unwrap(transport.storageRemove(id));
    },
    moveAndRemove: async (id, target) =>
      complete(await unwrap(transport.storageMoveAndRemove(id, target))),
    chooseDirectory: () => unwrap(transport.storageChooseDirectory()),
    preferences: async () => {
      const snapshot = await unwrap(transport.settingsGet("core.storage"));
      const value = snapshot.value;
      if (
        !value ||
        typeof value !== "object" ||
        Array.isArray(value) ||
        typeof value.alwaysUseDefault !== "boolean"
      )
        throw new Error("Storage preferences are invalid.");
      return { alwaysUseDefault: value.alwaysUseDefault };
    },
    move: async (entry, target) => complete(await unwrap(transport.moveGame(entry, target))),
    uninstall: async (entry) => complete(await unwrap(transport.uninstallGame(entry))),
  };
}
async function native<T>(call: () => Promise<T>): Promise<T> {
  if (!isTauri()) throw new Error("Open Caskade desktop to manage storage locations.");
  return call();
}
const ipcStorageApi: StorageApi = {
  ...createStorageApi({
    storageList: () => native(() => commands.storageList()),
    storageAdd: (path, label) => native(() => commands.storageAdd(path, label)),
    storageEdit: (id, label, template) => native(() => commands.storageEdit(id, label, template)),
    storageDefault: (id) => native(() => commands.storageDefault(id)),
    storageRemove: (id) => native(() => commands.storageRemove(id)),
    storageMoveAndRemove: (id, target) => native(() => commands.storageMoveAndRemove(id, target)),
    storageChooseDirectory: () => native(() => commands.storageChooseDirectory()),
    moveGame: (entry, target) => native(() => commands.moveGame(entry, target)),
    uninstallGame: (entry) => native(() => commands.uninstallGame(entry)),
    jobGet: (id) => native(() => commands.jobGet(id)),
    settingsGet: (namespace) => native(() => commands.settingsGet(namespace)),
  }),
  subscribe: async (listener) => {
    if (!isTauri()) return () => {};
    const subscriptions = await Promise.allSettled([
      events.libraryChanged.listen(listener),
      events.settingsChanged.listen(({ payload }) => {
        if (payload.namespace === "core.storage" || payload.namespace === "storage.roots")
          listener();
      }),
    ]);
    const cleanup = () => {
      for (const item of subscriptions) if (item.status === "fulfilled") item.value();
    };
    if (subscriptions.some((item) => item.status === "rejected")) {
      cleanup();
      throw new Error("Storage event subscription failed.");
    }
    return cleanup;
  },
};
/** Dev builds started with `VITE_CASKADE_FIXTURES=1` list synthetic roots
 * instead (README.md). */
export const nativeStorageApi: StorageApi =
  import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1"
    ? lazyApi(
        () => import("./storageFixtures").then((module) => module.createStorageFixtureApi()),
        [
          "list",
          "preferences",
          "add",
          "edit",
          "setDefault",
          "remove",
          "moveAndRemove",
          "chooseDirectory",
          "move",
          "uninstall",
          "subscribe",
        ],
      )
    : ipcStorageApi;
