/** The app's language (#213). The core owns it: it resolves the Language
 * setting (`core.general.language`, `system` by default) to a language it
 * ships, else English, and announces each change. The webview only follows.
 * Without a core (the browser preview, tests) the setting is resolved here,
 * against the languages this build has, with `navigator.languages` (or
 * `?lang=`) standing in for the OS's. */
import { locales, requestedLocale, sourceLocale } from "@caskade/i18n";
import { isTauri } from "@tauri-apps/api/core";
import { commands, events } from "./bindings";
import type { SettingsApi } from "./settings";
import { nativeSettingsApi } from "./settings";

/** The language in use, what System means here, and the languages there are. */
export interface LocaleInfo {
  locale: string;
  /** What `system` resolves to. */
  system: string;
  /** English, then every language with a catalog. */
  available: string[];
}

export interface LocaleApi {
  current(): Promise<LocaleInfo>;
  /** The language in use after each change. */
  subscribe(listener: (locale: string) => void): Promise<() => void>;
}

/** The Language setting's value for the OS's languages. */
export const SYSTEM = "system";

/** `tag` and the less specific tags it falls back to: `pt-BR`, then `pt`. */
function fallbacks(tag: string): string[] {
  const parts = tag.replace(/_/g, "-").split(/[.@]/)[0]?.split("-") ?? [];
  const chain: string[] = [];
  for (let length = parts.length; length > 0; length--) {
    chain.push(parts.slice(0, length).join("-").toLowerCase());
  }
  return chain;
}

/** The first of `available` the requested tags fall back to, most
 * preferred first; English when none does (as the core's `negotiate`). */
export function negotiate(requested: readonly string[], available: readonly string[]): string {
  for (const tag of requested) {
    for (const candidate of fallbacks(tag)) {
      const match = available.find((locale) => locale.toLowerCase() === candidate);
      if (match) return match;
    }
  }
  return sourceLocale;
}

/** A locale API over a settings API, resolving here what the core would. */
export function createSettingsLocaleApi({
  settings,
  system,
  available = locales,
}: {
  settings: SettingsApi;
  /** The OS's languages, most preferred first. */
  system: () => readonly string[];
  available?: readonly string[];
}): LocaleApi {
  const resolve = (language: string) =>
    negotiate(language === SYSTEM || !language ? system() : [language], available);
  const setting = async () => {
    try {
      const { value } = await settings.get("core.general");
      return typeof value.language === "string" ? value.language : SYSTEM;
    } catch {
      return SYSTEM;
    }
  };
  return {
    current: async () => ({
      locale: resolve(await setting()),
      system: resolve(SYSTEM),
      available: [...available],
    }),
    subscribe: async (listener) => {
      // Only a change of language: the section's other settings change too.
      let last = resolve(await setting());
      return settings.subscribe(({ namespace }) => {
        if (namespace !== "core.general") return;
        void setting().then((language) => {
          const locale = resolve(language);
          if (locale === last) return;
          last = locale;
          listener(locale);
        });
      });
    },
  };
}

const browserLanguages = () => {
  const lang = requestedLocale();
  return lang ? [lang] : (globalThis.navigator?.languages ?? []);
};

const ipcLocaleApi: LocaleApi = {
  current: () => commands.localeCurrent(),
  subscribe: (listener) => events.localeChanged.listen(({ payload }) => listener(payload.locale)),
};

export const nativeLocaleApi: LocaleApi = isTauri()
  ? ipcLocaleApi
  : createSettingsLocaleApi({ settings: nativeSettingsApi, system: browserLanguages });
