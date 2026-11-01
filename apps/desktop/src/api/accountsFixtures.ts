/** In-memory accounts for the browser preview and tests, with the core's
 * rules that the UI depends on: a single account without a PIN starts
 * signed in, PINs are checked and throttled (5 wrong, then a wait), an
 * admin remains, and every admin has a PIN while standard accounts exist
 * (a change that breaks one is refused with the core's words). */
import type { Account, AccountAccess, AccountsApi, PinCheck } from "./accounts";
import type { LibraryEvent } from "./types";

export interface FixtureAccount extends Partial<Account> {
  id: string;
  name: string;
  /** The account's PIN; `hasPin` follows it. */
  pin?: string;
}

/** The library before anyone set accounts up: one admin without a PIN. */
export const defaultFixtureAccounts: FixtureAccount[] = [
  { id: "player", name: "Player", role: "admin" },
];

const TRIES = 5;
const WAIT_MS = 30_000;
/** `caskade_core::accounts::SETTINGS_SECTIONS`: an admin's permissions. */
const SECTIONS = [
  "general",
  "library-discovery",
  "launchers",
  "emulators",
  "appearance",
  "storage",
  "installer",
  "plugins",
];

export function createAccountsFixtureApi(
  given: FixtureAccount[] = defaultFixtureAccounts,
  emit: (event: LibraryEvent) => void = () => {},
  { activeId }: { activeId?: string | null } = {},
): AccountsApi & { pins: Map<string, string>; failures: Map<string, number> } {
  const now = Date.now();
  const pins = new Map<string, string>();
  const failures = new Map<string, number>();
  const accounts: Account[] = given.map(({ pin, ...account }) => {
    if (pin) pins.set(account.id, pin);
    return {
      avatar: "avatar:default",
      role: "standard",
      permissions: { settings: [], install: false },
      maxAge: null,
      unratedNeedsPin: false,
      lockedUntilMs: null,
      createdAtMs: now,
      updatedAtMs: now,
      ...account,
      hasPin: Boolean(pin),
    };
  });
  const only = accounts.length === 1 ? accounts[0] : undefined;
  let active: string | null =
    activeId !== undefined ? activeId : only && !only.hasPin ? only.id : null;
  const changed = () => emit({ type: "account.changed" });
  const find = (id: string) => {
    const account = accounts.find((candidate) => candidate.id === id);
    if (!account) throw new Error(`account ${id} was not found`);
    return account;
  };
  const check = (account: Account, pin: string | null): PinCheck => {
    if (account.lockedUntilMs && account.lockedUntilMs > Date.now())
      return { type: "locked", untilMs: account.lockedUntilMs };
    if (pin !== null && pin === pins.get(account.id)) {
      failures.delete(account.id);
      account.lockedUntilMs = null;
      return { type: "matched" };
    }
    const count = (failures.get(account.id) ?? 0) + 1;
    failures.set(account.id, count);
    if (count % TRIES === 0) {
      account.lockedUntilMs = Date.now() + WAIT_MS;
      changed();
    }
    return { type: "wrong" };
  };
  const copy = <T>(value: T): T => structuredClone(value);
  const name = (text: string) => {
    const trimmed = text.trim();
    if (!trimmed || trimmed.length > 64) throw new Error("an account name is 1 to 64 characters");
    return trimmed;
  };
  const withAccess = (access: AccountAccess) => ({
    ...access,
    permissions:
      access.role === "admin"
        ? { settings: [...SECTIONS], install: true }
        : { settings: [...access.permissions.settings], install: access.permissions.install },
  });
  /** Runs `change`, and puts every account back when it breaks a rule. */
  const checked = <T>(change: () => T): T => {
    const before = structuredClone(accounts);
    const pinsBefore = new Map(pins);
    const result = change();
    const admins = accounts.filter((account) => account.role === "admin");
    const problem =
      admins.length === 0
        ? "the last admin can't be removed or made standard"
        : accounts.some((account) => account.role === "standard") &&
            admins.some((admin) => !admin.hasPin)
          ? "every admin needs a PIN while there are standard accounts"
          : null;
    if (problem) {
      accounts.splice(0, accounts.length, ...before);
      pins.clear();
      for (const [id, pin] of pinsBefore) pins.set(id, pin);
      throw new Error(problem);
    }
    changed();
    return result;
  };
  return {
    pins,
    failures,
    async list() {
      return copy({ accounts, activeId: active });
    },
    async create({ pin, access, ...rest }) {
      if (pin !== null && !/^\d{4,8}$/.test(pin)) throw new Error("a PIN is 4 to 8 digits");
      const account: Account = {
        id: `account-${accounts.length + 1}`,
        ...rest,
        name: name(rest.name),
        ...withAccess(access),
        hasPin: Boolean(pin),
        lockedUntilMs: null,
        createdAtMs: Date.now(),
        updatedAtMs: Date.now(),
      };
      checked(() => {
        if (pin) pins.set(account.id, pin);
        accounts.push(account);
      });
      return copy(account);
    },
    async update(id, update) {
      const index = accounts.findIndex((account) => account.id === id);
      const account = { ...find(id) };
      Object.assign(account, {
        name: name(update.name),
        avatar: update.avatar,
        ...(update.access && withAccess(update.access)),
        updatedAtMs: Date.now(),
      });
      checked(() => {
        accounts[index] = account;
      });
      return copy(account);
    },
    async delete(id) {
      const at = accounts.findIndex((account) => account.id === id);
      if (at < 0) return false;
      checked(() => {
        accounts.splice(at, 1);
        pins.delete(id);
      });
      if (active === id) active = null;
      changed();
      return true;
    },
    async signIn(id, pin) {
      const account = find(id);
      const result = account.hasPin ? check(account, pin) : ({ type: "matched" } as const);
      if (result.type === "matched") {
        active = id;
        changed();
      }
      return result;
    },
    async signOut() {
      active = null;
      changed();
    },
    async setPin(id, pin) {
      const account = find(id);
      if (pin !== null && !/^\d{4,8}$/.test(pin)) throw new Error("a PIN is 4 to 8 digits");
      checked(() => {
        if (pin) pins.set(id, pin);
        else pins.delete(id);
        account.hasPin = Boolean(pin);
        account.lockedUntilMs = null;
        failures.delete(id);
      });
      return copy(account);
    },
    async verifyAdminPin(pin) {
      const admins = accounts.filter((account) => account.role === "admin" && account.hasPin);
      return admins.some((admin) => pins.get(admin.id) === pin)
        ? { type: "matched" }
        : { type: "wrong" };
    },
  };
}
