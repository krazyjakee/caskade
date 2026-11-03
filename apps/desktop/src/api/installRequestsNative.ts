import { isTauri } from "@tauri-apps/api/core";
import { commands, events } from "./bindings";
import type { InstallRequestApi } from "./installRequests";
import { unwrap } from "./reply";

function native() {
  if (!isTauri()) throw new Error("Open Caskade desktop to handle installation links.");
}
export const nativeInstallRequestApi: InstallRequestApi = {
  pending: async () => (isTauri() ? commands.pendingInstallRequests() : []),
  request: (value) => {
    native();
    return unwrap(commands.requestInstallInput(value));
  },
  requestDrop: (path) => {
    native();
    return unwrap(commands.requestDroppedInstall(path));
  },
  async confirm(id, rootId) {
    native();
    return (await unwrap(commands.confirmInstallRequest(id, rootId ?? null))).id;
  },
  async confirmCollection(id) {
    native();
    await unwrap(commands.confirmShareCollection(id));
  },
  async dismiss(id) {
    native();
    await unwrap(commands.dismissInstallRequest(id));
  },
  subscribe: async (listener) =>
    isTauri() ? events.installRequested.listen(({ payload }) => listener(payload)) : () => {},
};
