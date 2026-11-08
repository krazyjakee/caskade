/** `@caskade/sdk`: what a plugin's UI half imports. Inside
 * Caskade the import map resolves it to the app's own instance, so plugins
 * share one React tree and one SDK. Its version is the `caskade:plugin` WIT
 * version (§4.4). */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  type ComponentPropsWithRef,
  type ComponentType,
  createContext,
  createElement,
  Fragment,
  type ReactNode,
  type RefObject,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import type {
  GameExited_Serialize,
  GameStarted_Serialize,
  LibraryChanged_Serialize,
  PluginDisabled,
  PluginReloaded_Serialize,
  PluginsChanged,
  SettingsChanged,
} from "./core";
import { createPluginI18n, type MessageValues, type PluginI18n } from "./i18n";

export type * from "./core";
export {
  createPluginI18n,
  type MessageOptions,
  type MessageValues,
  type PluginI18n,
  type PluginMessages,
} from "./i18n";

/** The API version this SDK implements (`caskade:plugin@…`). 0.1.1 added
 * plugins' own translations (#219): `ctx.i18n`, `useTranslation`, `<Trans>`. */
export const API_VERSION = "0.1.1";

/** UI contribution points the shell renders (§4.3). */
export const pluginSlots = [
  "views",
  "home.sections",
  "game.details.tabs",
  "game.details.panels",
  "game.card.badges",
  "game.card.overlay",
  "game.actions",
  "topbar.items",
  "sidebar.items",
  "settings.pages",
  "commands",
  "notifications",
] as const;
export type PluginSlot = (typeof pluginSlots)[number];

/** Events a UI half can observe, with their payloads. */
export interface PluginEvents {
  "game.started": GameStarted_Serialize;
  "game.exited": GameExited_Serialize;
  "library.changed": LibraryChanged_Serialize;
  /** This plugin's own settings changed (#128). */
  "settings.changed": SettingsChanged;
  /** A backend plugin kept crashing and core turned it off (#125). */
  "plugin.disabled": PluginDisabled;
  /** A plugin was enabled, disabled, installed or removed. */
  "plugins.changed": PluginsChanged;
  /** A dev plugin changed on disk (#131). */
  "plugin.reloaded": PluginReloaded_Serialize;
  /** Caskade's language changed (#219); the app activates plugins again
   * with their catalog for it. */
  "locale.changed": { locale: string };
}
export type PluginEventName = keyof PluginEvents;

interface Base {
  /** Must match an id the manifest declares for the slot. */
  id: string;
}
interface Titled extends Base {
  title: string;
}
type GameProps = { gameId: string };

/** What each slot accepts. */
export interface SlotContributions {
  views: Titled & { component: ComponentType };
  "home.sections": Titled & { component: ComponentType };
  "game.details.tabs": Titled & { component: ComponentType<GameProps> };
  "game.details.panels": Base & { component: ComponentType<GameProps> };
  "game.card.badges": Base & { component: ComponentType<GameProps> };
  "game.card.overlay": Base & { component: ComponentType<GameProps> };
  "game.actions": Titled & { run: (gameId: string) => void | Promise<void> };
  "topbar.items": Base & { component: ComponentType };
  "sidebar.items": Base & { component: ComponentType };
  "settings.pages": Titled & { component: ComponentType };
  commands: Titled & { run: () => void | Promise<void> };
  /** Rendered in the notifications menu. */
  notifications: Base & { component: ComponentType };
}
export type ContributionFor<S extends PluginSlot> = SlotContributions[S];
export type Contribution = SlotContributions[PluginSlot];

export interface PluginContext {
  plugin: { id: string; name: string; version: string };
  slots: {
    register<S extends PluginSlot>(slot: S, contribution: ContributionFor<S>): () => void;
  };
  events: {
    /** `payload` is `PluginEvents[name]`. */
    on(name: PluginEventName, handler: (payload: unknown) => void): () => void;
  };
  backend: {
    /** This plugin's backend half (`handle-request`). */
    call<T = unknown>(method: string, payload?: unknown): Promise<T>;
  };
  /** The values of the plugin's `contributes.settings` schema (#128). */
  settings: {
    get<T = Record<string, unknown>>(): Promise<T>;
    /** Core validates against the schema; an invalid value rejects. */
    set<T = Record<string, unknown>>(value: T): Promise<T>;
    /** Called with the new values after every change; returns an unsubscribe. */
    on<T = Record<string, unknown>>(listener: (value: T) => void): () => void;
  };
  notify(message: string): void;
  /** The plugin's own translations in the language Caskade is in (#219):
   * `ctx.i18n.t("Repack")` for titles registered in `activate`. Inside
   * contributions, `useTranslation()` and `<Trans>`. */
  i18n: PluginI18n;
  /** Library reads outside React, e.g. in event handlers. */
  library: {
    game(id: string): Promise<PluginGame | null>;
  };
}

export interface PluginDefinition {
  activate(ctx: PluginContext): void | Promise<void>;
  deactivate?(): void | Promise<void>;
}

/** A UI plugin's default export. */
export function definePlugin(definition: PluginDefinition): PluginDefinition {
  return definition;
}

/** Set around every contributed component, so hooks know their plugin. */
export const PluginScope = createContext<PluginContext | null>(null);

export function usePlugin(): PluginContext {
  const scope = useContext(PluginScope);
  if (!scope) throw new Error("usePlugin() must be used inside a plugin contribution");
  return scope;
}

/** English, outside a plugin's contributions. */
const english = createPluginI18n(null);

/** The plugin's `t` and `plural` in the language Caskade is in, inside its
 * contributions (#219). Outside Caskade, English. */
export function useTranslation(): PluginI18n {
  return useContext(PluginScope)?.i18n ?? english;
}

/** A message from the plugin's catalog, with React elements allowed in its
 * placeholders: `<Trans message="Drop a pack on {library}" values={{ library: <b>…</b> }} />`,
 * or plain text: `<Trans>Game packs</Trans>`. */
export function Trans({
  message,
  children,
  values = {},
  context,
}: {
  message?: string;
  children?: string;
  values?: Record<string, ReactNode>;
  context?: string;
}) {
  const i18n = useTranslation();
  // Looked up with the placeholders unfilled, then filled with the values,
  // elements included, wherever the translation put them.
  const unfilled: MessageValues = Object.fromEntries(
    Object.keys(values).map((name) => [name, `{${name}}`]),
  );
  const translated = i18n.t(message ?? children ?? "", unfilled, { context });
  return createElement(
    Fragment,
    null,
    ...translated.split(/(\{\w+\})/).map((part, index) => {
      const name = /^\{(\w+)\}$/.exec(part)?.[1];
      return createElement(
        Fragment,
        { key: index },
        name !== undefined && name in values ? values[name] : part,
      );
    }),
  );
}

/** A library game as plugins see it. */
export interface PluginGame {
  id: string;
  title: string;
  platform: string;
  status: "installed" | "not-installed" | "emulated";
  favorite: boolean;
  playtimeSeconds: number;
  lastPlayedAt: string | null;
  description: string;
  tags: string[];
}
export interface GameQuery {
  data: PluginGame | null | undefined;
  isPending: boolean;
  error: Error | null;
}
export interface GamesQuery {
  data: PluginGame[] | undefined;
  isPending: boolean;
  error: Error | null;
}
/** Which games `useGames` returns, in what order. */
export interface GamesOptions {
  sort?: "title" | "playtime" | "recent";
  /** At most this many (default 100). */
  limit?: number;
}
/** Desktop or console mode (#152). */
export type Mode = "desktop" | "console";

/** What a controller (or the keyboard standing in for one) asks for. A pad's
 * face buttons are `confirm` (A), `back` (B), `details` (X) and `search`
 * (Y); the shoulders `tab.*` (LB/RB) and `page.*` (LT/RT). */
export type InputAction =
  | "nav.up"
  | "nav.down"
  | "nav.left"
  | "nav.right"
  | "confirm"
  | "back"
  | "details"
  | "search"
  | "tab.prev"
  | "tab.next"
  | "page.prev"
  | "page.next"
  | "menu"
  | "view"
  | "guide";

export interface InputActionOptions {
  /** Shown in console mode's button-hint bar. */
  label?: string;
  /** Only while focus is inside this element; the whole view without one. */
  owner?: RefObject<Element | null>;
  enabled?: boolean;
}

/** What the app provides for hooks that read its data. */
export interface HostServices {
  useGame(gameId: string): GameQuery;
  useGames(options?: GamesOptions): GamesQuery;
  /** Command bindings for pad actions (#152). */
  useInputAction?(action: InputAction, handler: () => unknown, options?: InputActionOptions): void;
  /** The current mode. */
  useMode?(): Mode;
}
/** Provided by the app around plugin contributions. */
export const HostScope = createContext<HostServices | null>(null);

/** A game from the library. */
export function useGame(gameId: string): GameQuery {
  const host = useContext(HostScope);
  if (!host) throw new Error("useGame() needs the Caskade app");
  return host.useGame(gameId);
}

/** Library games, e.g. the most played (`{ sort: "playtime", limit: 10 }`). */
export function useGames(options?: GamesOptions): GamesQuery {
  const host = useContext(HostScope);
  if (!host) throw new Error("useGames() needs the Caskade app");
  return host.useGames(options);
}

/** Calls this plugin's backend half and caches the answer. */
export function useBackendQuery<T = unknown>(method: string, payload?: unknown) {
  const plugin = usePlugin();
  return useQuery({
    queryKey: ["plugin", plugin.plugin.id, method, payload ?? null],
    queryFn: () => plugin.backend.call<T>(method, payload),
  });
}

/** This plugin's settings, kept current, and a validated setter. */
export function useSettings<T = Record<string, unknown>>() {
  const plugin = usePlugin();
  const client = useQueryClient();
  const id = plugin.plugin.id;
  const key = ["plugin", id, "settings"];
  const query = useQuery({ queryKey: key, queryFn: () => plugin.settings.get<T>() });
  useEffect(
    () => plugin.settings.on<T>((value) => client.setQueryData(["plugin", id, "settings"], value)),
    [plugin, client, id],
  );
  const mutation = useMutation({
    mutationFn: (value: T) => plugin.settings.set<T>(value),
    onSuccess: (value) => client.setQueryData(key, value),
  });
  return {
    settings: query.data,
    error: query.error ?? mutation.error,
    saving: mutation.isPending,
    setSettings: (value: T) => mutation.mutate(value),
  };
}

const ignoreAction = () => {};
const desktopMode = (): Mode => "desktop";

/** Handles a pad action (or the key that stands in for it) while focus is
 * inside `owner`. Return false from `handler` to let the next binding, or
 * the default (move focus, press, go back), have it. Labelled bindings show
 * in console mode's hint bar. Outside Caskade it does nothing. */
export function useInputAction(
  action: InputAction,
  handler: () => unknown,
  options?: InputActionOptions,
): void {
  const host = useContext(HostScope);
  const bind = host?.useInputAction ?? ignoreAction;
  bind(action, handler, options);
}

/** Whether Caskade is in desktop or console mode. */
export function useMode(): Mode {
  const host = useContext(HostScope);
  const read = host?.useMode ?? desktopMode;
  return read();
}

/** A group of focusables a pad enters as one: it remembers where focus last
 * was and goes back there (a shelf, a grid, a form). */
export function FocusSection({ id, ...props }: { id: string } & ComponentPropsWithRef<"div">) {
  return createElement("div", { "data-focus-section": id, ...props });
}

/** Makes a custom element a focus stop for a pad. Controls built on
 * `@caskade/ui` or React Aria already are. `focusKey` brings focus back to
 * it after a re-render or a return to the view. */
export function useFocusable<T extends HTMLElement>(options: { focusKey?: string } = {}) {
  const [element, setElement] = useState<T | null>(null);
  const [focused, setFocused] = useState(false);
  const ref = useCallback((node: T | null) => setElement(node), []);
  useEffect(() => {
    if (!element) return;
    if (element.tabIndex < 0) element.tabIndex = 0;
    if (options.focusKey) element.dataset.focusKey = options.focusKey;
    const on = () => setFocused(true);
    const off = () => setFocused(false);
    element.addEventListener("focus", on);
    element.addEventListener("blur", off);
    setFocused(document.activeElement === element);
    return () => {
      element.removeEventListener("focus", on);
      element.removeEventListener("blur", off);
    };
  }, [element, options.focusKey]);
  const focus = useCallback(() => element?.focus(), [element]);
  return { ref, focused, focus };
}
