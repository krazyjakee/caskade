import { isTauri } from "@tauri-apps/api/core";
import type { AccountsApi } from "./accounts";
import { commands } from "./bindings";
import { unwrap } from "./reply";

function native() {
  if (!isTauri()) throw new Error("Open Caskade desktop to use accounts.");
}

/** The core's accounts (`commands/accounts.rs`); `account.changed` reaches
 * the app through the library's subscription. */
export const nativeAccounts: AccountsApi = {
  async list() {
    native();
    return unwrap(commands.accountsList());
  },
  async create(account) {
    native();
    return unwrap(commands.accountCreate(account));
  },
  async update(id, update) {
    native();
    return unwrap(commands.accountUpdate(id, update));
  },
  async delete(id) {
    native();
    return unwrap(commands.accountDelete(id));
  },
  async signIn(id, pin) {
    native();
    return unwrap(commands.accountSignIn(id, pin));
  },
  async signOut() {
    native();
    await unwrap(commands.accountSignOut());
  },
  async setPin(id, pin) {
    native();
    return unwrap(commands.accountSetPin(id, pin));
  },
  async verifyAdminPin(pin) {
    native();
    return unwrap(commands.accountVerifyAdminPin(pin));
  },
};
