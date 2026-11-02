import { type SettingsTab, settingsTabs } from "../SettingsView";

/** The Settings tab the user was last on. A link to `/settings` that doesn't
 * name a tab comes back to it while the app runs (page reloads included, such
 * as webview recovery's); a new start opens General. */
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
    return pickTab(sessionStorage.getItem(KEY)) ?? defaultSettingsTab;
  } catch {
    // Unavailable storage: the default.
    return defaultSettingsTab;
  }
}

export function rememberSettingsTab(tab: SettingsTab) {
  try {
    sessionStorage.setItem(KEY, tab);
  } catch {
    // Storage full or disabled: the tab still works, it just isn't kept.
  }
}

// Earlier versions kept the tab across restarts.
try {
  localStorage.removeItem(KEY);
} catch {
  // Unavailable storage: nothing was kept.
}
