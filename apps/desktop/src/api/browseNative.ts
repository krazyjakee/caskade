import { isTauri } from "@tauri-apps/api/core";
import { commands } from "./bindings";
import type { BrowseApi } from "./browse";
import { lazyApi } from "./lazyApi";
import { unwrap } from "./reply";

const ipcBrowseApi: BrowseApi = {
  async list(path, showHidden = false) {
    if (!isTauri()) throw new Error("Open Caskade desktop to browse folders.");
    return unwrap(commands.browseDir(path, showHidden));
  },
};

/** Dev builds started with `VITE_CASKADE_FIXTURES=1` browse a small fake
 * tree instead (README.md). */
export const nativeBrowseApi: BrowseApi =
  import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1"
    ? lazyApi(
        () => import("./browseFixtures").then((module) => module.createBrowseFixtureApi()),
        ["list"],
      )
    : ipcBrowseApi;
