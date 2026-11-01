import { type SettingsTab, settingsTabs } from "../SettingsView";

/** The Settings tab the user was last on. A link to `/settings` that doesn't
 * name a tab comes back to it, across restarts. */
const KEY = "caskade.settings.tab";
const defaultSettingsTab: SettingsTab = "general";

function pickTab(value: unknown): SettingsTab | undefined {
  return settingsTabs.find((t) => t.id === value)?.id;
}

/** `search.tab` when it is valid, the remembered tab otherwise. */
export function resolveSettingsTab(search: Record<string, unknown>): SettingsTab {
  return pickTab(search.tab) ?? rememberedSettingsTab();
}

export function rememberedSettingsTab(): SettingsTab {
  try {
    return pickTab(localStorage.getItem(KEY)) ?? defaultSettingsTab;
  } catch {
    // Unavailable storage: the default.
    return defaultSettingsTab;
  }
}

export function rememberSettingsTab(tab: SettingsTab) {
  try {
    localStorage.setItem(KEY, tab);
  } catch {
    // Storage full or disabled: the tab still works, it just isn't kept.
  }
}
