import type { PluginEventName, PluginMessages, PluginSlot } from "@caskade/sdk";

/** Slots and plugin events are part of the plugin API (`@caskade/sdk`). */
export { type PluginEventName, type PluginSlot, pluginSlots } from "@caskade/sdk";

/** A contribution the manifest declares; only these can be registered. */
export interface DeclaredContribution {
  slot: PluginSlot;
  id: string;
  title?: string | null;
  /** Where it shows (#152): `desktop`, `console`, or empty for the slot's
   * default (views: both). */
  modes?: readonly string[];
}

/** An enabled plugin with a UI half, as core reports it. */
export interface UiPlugin {
  id: string;
  name: string;
  version: string;
  /** URL of the `ui` ES module (`caskade-plugin://<id>/…`). */
  entry: string;
  /** Optional stylesheet, loaded into the `cs.plugins` cascade layer. */
  style?: string | null;
  contributes: DeclaredContribution[];
}

export interface PluginEvent {
  name: PluginEventName;
  payload: unknown;
}

/** An installed plugin as Settings → Plugins lists it (#128). */
export interface InstalledPlugin {
  id: string;
  name: string;
  enabled: boolean;
  /** It declares `contributes.settings`. */
  hasSettings: boolean;
  /** Why its settings schema was refused; its other parts still work. */
  settingsError: string | null;
}
/** A plugin's settings section, `plugins.<id>`. */
export interface PluginSettingsSnapshot {
  revision: number;
  value: unknown;
}

/** What the plugin loader needs from core. */
export interface PluginUiApi {
  list(): Promise<UiPlugin[]>;
  /** A plugin view opened: starts its backend on `onView:<id>` (#125). */
  viewOpened?(pluginId: string, viewId: string): Promise<void>;
  installed(): Promise<InstalledPlugin[]>;
  settings: {
    get(pluginId: string): Promise<PluginSettingsSnapshot>;
    /** Validated by core against the plugin's schema. */
    set(pluginId: string, value: unknown, revision: number): Promise<PluginSettingsSnapshot>;
  };
  /** The plugin's own catalog for the language in use (#219); none means
   * English. */
  messages?(pluginId: string): Promise<PluginMessages>;
  /** The plugin backend's `handle-request` export. */
  request(pluginId: string, method: string, payload: unknown): Promise<unknown>;
  subscribe(listener: (event: PluginEvent) => void): Promise<() => void>;
}
