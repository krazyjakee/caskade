import { commands, events, type SettingsSnapshot as NativeSnapshot, type Value } from "./bindings";
import { lazyApi } from "./lazyApi";
import { type Reply, unwrap } from "./reply";

/** Settings transport boundary; schemas and validation remain authoritative in core. */
export interface SettingsSnapshot {
  namespace: string;
  version: number;
  revision: number;
  value: Record<string, unknown>;
}
export interface SettingsSection {
  namespace: string;
  version: number;
  schema: unknown;
  defaults: unknown;
}
export interface SettingsChanged {
  namespace: string;
  version: number;
}
export interface SettingsApi {
  sections(): Promise<SettingsSection[]>;
  get(namespace: string): Promise<SettingsSnapshot>;
  set(
    namespace: string,
    value: Record<string, unknown>,
    revision: number,
  ): Promise<SettingsSnapshot>;
  reset(namespace: string, revision: number): Promise<SettingsSnapshot>;
  subscribe(listener: (event: SettingsChanged) => void): Promise<() => void>;
}
export interface SettingsTransport {
  settingsSections(): Promise<Reply<SettingsSection[]>>;
  settingsGet(namespace: string): Promise<Reply<SettingsSnapshot>>;
  settingsSet(
    namespace: string,
    value: Record<string, unknown>,
    revision: number,
  ): Promise<Reply<SettingsSnapshot>>;
  settingsReset(namespace: string, revision: number): Promise<Reply<SettingsSnapshot>>;
}
export function createSettingsApi(
  transport: SettingsTransport,
  subscribe: SettingsApi["subscribe"],
): SettingsApi {
  return {
    sections: () => unwrap(transport.settingsSections()),
    get: (namespace) => unwrap(transport.settingsGet(namespace)),
    set: (namespace, value, revision) => unwrap(transport.settingsSet(namespace, value, revision)),
    reset: (namespace, revision) => unwrap(transport.settingsReset(namespace, revision)),
    subscribe,
  };
}

export function objectReply(reply: Reply<NativeSnapshot>): Reply<SettingsSnapshot> {
  if (reply.status === "error") return reply;
  const value = reply.data.value;
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("This settings panel requires an object section.");
  }
  return { status: "ok", data: { ...reply.data, value } };
}
/** Only generated IPC commands/events cross the native boundary. */
const ipcSettingsApi = createSettingsApi(
  {
    settingsSections: () => commands.settingsSections(),
    settingsGet: async (namespace) => objectReply(await commands.settingsGet(namespace)),
    settingsSet: async (namespace, value, revision) =>
      objectReply(
        await commands.settingsSet(namespace, JSON.parse(JSON.stringify(value)) as Value, revision),
      ),
    settingsReset: async (namespace, revision) =>
      objectReply(await commands.settingsReset(namespace, revision)),
  },
  (listener) => events.settingsChanged.listen(({ payload }) => listener(payload)),
);
/** Dev builds started with `VITE_CASKADE_FIXTURES=1` keep the sections in
 * memory instead (README.md); production builds drop the fixture import. */
export const nativeSettingsApi: SettingsApi =
  import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1"
    ? lazyApi(
        () => import("./settingsFixtures").then((module) => module.settingsFixtureApi),
        ["sections", "get", "set", "reset", "subscribe"],
      )
    : ipcSettingsApi;
