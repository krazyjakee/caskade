/** Local accounts (#202, #207): who is signed in, the account picker's list,
 * sign-in with a PIN, and switching. Every rule is the core's; a PIN goes
 * straight to it and is never kept here or in browser storage. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { libraryKeys, useLibraryApi } from "./provider";

type AccountRole = "admin" | "standard";

export interface Account {
  id: string;
  name: string;
  /** An `avatar:<name>` id from `@caskade/ui`'s set. */
  avatar: string;
  role: AccountRole;
  hasPin: boolean;
  permissions: { settings: string[]; install: boolean };
  /** The highest minimum age it plays without an admin PIN; null for none. */
  maxAge: number | null;
  /** Whether unrated games need an admin PIN under its age limit. */
  unratedNeedsPin: boolean;
  /** While wrong PINs lock it: when tries are allowed again (Unix ms). */
  lockedUntilMs: number | null;
  createdAtMs: number;
  updatedAtMs: number;
}

export interface AccountList {
  /** Admins first, then by name. */
  accounts: Account[];
  activeId: string | null;
}

/** The answer to a PIN: never which account matched. */
export type PinCheck =
  | { type: "matched" }
  | { type: "wrong" }
  | { type: "locked"; untilMs: number };

export interface AccountAccess {
  role: AccountRole;
  permissions: { settings: string[]; install: boolean };
  maxAge: number | null;
  unratedNeedsPin: boolean;
}

export interface AccountsApi {
  list(): Promise<AccountList>;
  create(account: {
    name: string;
    avatar: string;
    access: AccountAccess;
    pin: string | null;
  }): Promise<Account>;
  /** `access: null` keeps it; an account changing itself may only send null. */
  update(
    id: string,
    update: { name: string; avatar: string; access: AccountAccess | null },
  ): Promise<Account>;
  delete(id: string): Promise<boolean>;
  /** `pin` is needed when the account has one. */
  signIn(id: string, pin: string | null): Promise<PinCheck>;
  signOut(): Promise<void>;
  /** 4–8 digits, or null to clear it. */
  setPin(id: string, pin: string | null): Promise<Account>;
  /** Whether `pin` is any admin's. */
  verifyAdminPin(pin: string): Promise<PinCheck>;
}

/** Under the library's keys: `account.changed` events refresh them, with
 * the collections that belong to the account. */
const accountKeys = {
  all: [...libraryKeys.all, "accounts"] as const,
};

/** The accounts, or `null` where the library keeps none. */
export function useAccountsApi(): AccountsApi | null {
  return useLibraryApi().accounts ?? null;
}

export function useAccounts() {
  const api = useAccountsApi();
  return useQuery({
    queryKey: accountKeys.all,
    queryFn: () => {
      if (!api) throw new Error("Accounts aren't available here");
      return api.list();
    },
    enabled: Boolean(api),
    // Who is signed in must never be a stale guess; without a core (the
    // browser preview) the app shows at once instead of retrying.
    staleTime: 0,
    retry: false,
  });
}

/** The signed-in account: undefined while it loads, null for nobody. */
export function useActiveAccount(): Account | null | undefined {
  const { data } = useAccounts();
  if (!data) return undefined;
  return data.accounts.find((account) => account.id === data.activeId) ?? null;
}

function useAccountMutation<T, R>(run: (api: AccountsApi, input: T) => Promise<R>) {
  const api = useAccountsApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: T) => {
      if (!api) throw new Error("Accounts aren't available here");
      return run(api, input);
    },
    // Another account's collections replace the last one's.
    onSuccess: () => void client.invalidateQueries({ queryKey: libraryKeys.all }),
  });
}

export function useSignIn() {
  return useAccountMutation((api, input: { id: string; pin: string | null }) =>
    api.signIn(input.id, input.pin),
  );
}

export function useSignOut() {
  return useAccountMutation((api, _: undefined) => api.signOut());
}

/** Renames an account and changes its avatar, and sets its PIN when given. */
export function useUpdateProfile() {
  return useAccountMutation(
    async (api, input: { account: Account; name: string; avatar: string; pin: string | null }) => {
      const { account, name, avatar, pin } = input;
      let updated = await api.update(account.id, { name, avatar, access: null });
      if (pin) updated = await api.setPin(account.id, pin);
      return updated;
    },
  );
}

/** Settings → Accounts (#208): an admin makes, changes and deletes
 * accounts. The core keeps its rules (an admin remains; every admin has a
 * PIN while standard accounts exist) and its refusal is the error. */
export function useCreateAccount() {
  return useAccountMutation((api, account: Parameters<AccountsApi["create"]>[0]) =>
    api.create(account),
  );
}

export function useUpdateAccount() {
  return useAccountMutation(
    (api, input: { id: string; update: Parameters<AccountsApi["update"]>[1] }) =>
      api.update(input.id, input.update),
  );
}

export function useDeleteAccount() {
  return useAccountMutation((api, id: string) => api.delete(id));
}

/** Sets an account's PIN, or clears it with null. */
export function useSetPin() {
  return useAccountMutation((api, input: { id: string; pin: string | null }) =>
    api.setPin(input.id, input.pin),
  );
}

/** What an account's access is now, to edit. */
export const accessOf = (account: Account): AccountAccess => ({
  role: account.role,
  permissions: structuredClone(account.permissions),
  maxAge: account.maxAge,
  unratedNeedsPin: account.unratedNeedsPin,
});

/** Whether `pin` can be a PIN: 4 to 8 digits. */
export const validPin = (pin: string) => /^\d{4,8}$/.test(pin);

const LAST = "caskade.accounts.last";

/** The account that signed in last here, to start the picker on. Only its
 * id is kept. */
export function lastAccountId(): string | null {
  try {
    return localStorage.getItem(LAST);
  } catch {
    return null;
  }
}

export function rememberAccount(id: string) {
  try {
    localStorage.setItem(LAST, id);
  } catch {
    // Private mode: the picker starts on the first account.
  }
}
