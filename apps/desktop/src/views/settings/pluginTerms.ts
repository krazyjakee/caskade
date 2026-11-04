/** What a plugin adds and hears about, in words (#53): the permission
 * review lists these instead of the manifest's ids
 * (`crates/caskade-plugin-host/src/manifest.rs`). An id this table doesn't
 * know shows as it is. */
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";

const slots: Record<string, MessageDescriptor> = {
  views: msg({ message: "A page of its own", context: "plugin slot" }),
  "home.sections": msg({ message: "A section on Home", context: "plugin slot" }),
  "game.details.tabs": msg({ message: "A tab on game pages", context: "plugin slot" }),
  "game.details.panels": msg({ message: "A panel on game pages", context: "plugin slot" }),
  "game.card.badges": msg({ message: "A badge on game cards", context: "plugin slot" }),
  "game.card.overlay": msg({ message: "An overlay on game cards", context: "plugin slot" }),
  "game.actions": msg({ message: "An item in the game menu", context: "plugin slot" }),
  "topbar.items": msg({ message: "A button in the top bar", context: "plugin slot" }),
  "sidebar.items": msg({ message: "An item in the sidebar", context: "plugin slot" }),
  "settings.pages": msg({ message: "A settings page", context: "plugin slot" }),
  commands: msg({ message: "A command", context: "plugin slot" }),
  notifications: msg({ message: "Notifications", context: "plugin slot" }),
};

/** Hooks that can change what Caskade does, by what they change. */
const interceptors: Record<string, MessageDescriptor> = {
  "launch.before": msg({ message: "games before they start", context: "plugin hook" }),
  "launch.after": msg({ message: "what happens after a game exits", context: "plugin hook" }),
  "library.import": msg({ message: "games joining the library", context: "plugin hook" }),
  "metadata.merge": msg({ message: "game details", context: "plugin hook" }),
  "install.plan": msg({ message: "how games install", context: "plugin hook" }),
  "install.after": msg({ message: "finished installs", context: "plugin hook" }),
  "install.moved": msg({ message: "moved games", context: "plugin hook" }),
};

/** Events, by the first part of their name. */
const events: Record<string, MessageDescriptor> = {
  app: msg({ message: "Caskade starting", context: "plugin event" }),
  library: msg({ message: "library changes", context: "plugin event" }),
  game: msg({ message: "games starting, stopping and moving", context: "plugin event" }),
  settings: msg({ message: "settings changes", context: "plugin event" }),
  emulation: msg({ message: "emulator changes", context: "plugin event" }),
  install: msg({ message: "installs", context: "plugin event" }),
  uninstall: msg({ message: "installs", context: "plugin event" }),
  input: msg({ message: "controllers connecting", context: "plugin event" }),
  achievements: msg({ message: "achievements", context: "plugin event" }),
};

const providers: Record<string, MessageDescriptor> = {
  "library-source": msg({ message: "games for your library", context: "plugin provider" }),
  "metadata-provider": msg({ message: "game details", context: "plugin provider" }),
  runner: msg({ message: "a way to run games", context: "plugin provider" }),
  "install-handler": msg({ message: "a way to install games", context: "plugin provider" }),
  "url-resolver": msg({ message: "downloads from web pages", context: "plugin provider" }),
};

type Translate = (descriptor: MessageDescriptor) => string;

export const slotName = (t: Translate, slot: string) => (slots[slot] ? t(slots[slot]) : slot);

/** A plugin's hooks as two lists: what it can change, and what it hears
 * about, each named once. */
export function hookNames(t: Translate, hooks: readonly string[]) {
  const changes = new Set<string>();
  const hears = new Set<string>();
  for (const hook of hooks) {
    const interceptor = interceptors[hook];
    if (interceptor) {
      changes.add(t(interceptor));
      continue;
    }
    const event = events[hook.split(".")[0] ?? ""];
    hears.add(event ? t(event) : hook);
  }
  return { changes: [...changes], hears: [...hears] };
}

export const providerName = (t: Translate, provider: string) =>
  providers[provider] ? t(providers[provider]) : provider;
