import type { PluginEventName, PluginMessages, PluginSlot } from "@caskade/sdk";
import type {
  Context_Deserialize as DrawnContext,
  Node_Serialize as DrawnNode,
  Outcome_Serialize as DrawnOutcome,
} from "../api/bindings";

/** Slots and plugin events are part of the plugin API (`@caskade/sdk`). */
export { type PluginEventName, type PluginSlot, pluginSlots } from "@caskade/sdk";

/** A contribution the manifest declares; only these can be registered. */
interface DeclaredContribution {
  slot: PluginSlot;
  id: string;
  title?: string | null;
  /** Where it shows (#152): `desktop`, `console`, or empty for the slot's
   * default (views: both). */
  modes?: readonly string[];
}

/** An enabled plugin with UI, as core reports it: a 0.1 JavaScript half,
 * or a 0.2 backend that draws it (`drawn`, #109). */
export interface UiPlugin {
  id: string;
  name: string;
  version: string;
  /** URL of the `ui` ES module (`caskade-plugin://<id>/…`); empty when
   * `drawn`. */
  entry: string;
  /** Its backend draws its UI (plugin API 0.2, #109). */
  drawn?: boolean;
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
interface PluginSettingsSnapshot {
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
  /** A 0.2 plugin's contribution, drawn by its backend (#109). */
  render?(pluginId: string, slot: string, id: string, context: DrawnContext): Promise<DrawnNode[]>;
  /** What the user did with a drawn contribution (#109). */
  act?(
    pluginId: string,
    slot: string,
    id: string,
    action: string,
    value: unknown,
    context: DrawnContext,
  ): Promise<DrawnOutcome>;
  subscribe(listener: (event: PluginEvent) => void): Promise<() => void>;
}
