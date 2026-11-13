/** The app's language (#213), as the core resolves it. `main.tsx` activates
 * it before the first render; `LocaleProvider` follows the core's changes
 * (the Language setting, in either shell or first-run setup), loads the
 * language's catalogs and activates them, so every view re-renders in it
 * without a restart, and keeps `<html lang>` in step. */
import { i18n, loadCatalog, sourceLocale, translated } from "@caskade/i18n";
import type { Messages } from "@lingui/core";
import { useLingui } from "@lingui/react/macro";
import { createContext, type ReactNode, useContext, useEffect, useState } from "react";
import { I18nProvider as AriaI18nProvider } from "react-aria-components";
import { type LocaleApi, type LocaleInfo, nativeLocaleApi, SYSTEM } from "../api/locale";
import { formatLocale } from "../api/presentation";
import { effectSubscription } from "../api/subscription";

const english: LocaleInfo = {
  locale: sourceLocale,
  system: sourceLocale,
  available: [sourceLocale],
};

const LocaleContext = createContext<LocaleInfo>(english);

/** The language in use, what System means and the languages there are. */
export function useLocale(): LocaleInfo {
  return useContext(LocaleContext);
}

export function applyLocale(locale: string, root: HTMLElement = document.documentElement) {
  root.lang = locale;
}

export function LocaleProvider({
  api = nativeLocaleApi,
  initial,
  root = document.documentElement,
  children,
}: {
  api?: LocaleApi;
  /** Read and activated before the first render, so English never flashes
   * in; asked for here when not given. */
  initial?: LocaleInfo;
  root?: HTMLElement;
  children: ReactNode;
}) {
  const [info, setInfo] = useState<LocaleInfo>(
    initial ?? { ...english, locale: i18n.locale || sourceLocale },
  );

  useEffect(() => applyLocale(info.locale, root), [info.locale, root]);

  useEffect(() => {
    if (initial) return;
    let live = true;
    api
      .current()
      .then((current) => {
        if (live) setInfo((info) => ({ ...current, locale: info.locale }));
      })
      .catch((error: unknown) => console.warn("languages unavailable", error));
    return () => {
      live = false;
    };
  }, [api, initial]);

  useEffect(() => {
    // Only the latest switch lands, however long each one's catalogs take.
    let latest = 0;
    return effectSubscription(api.subscribe, (locale) => {
      const turn = ++latest;
      const loading: Promise<Messages> =
        locale === sourceLocale ? Promise.resolve({}) : loadCatalog(locale);
      loading
        .then((messages) => {
          if (turn !== latest) return;
          i18n.loadAndActivate({ locale, messages });
          setInfo((info) => ({ ...info, locale }));
        })
        .catch((error: unknown) => console.warn("language not loaded", locale, error));
    });
  }, [api]);

  // React Aria's own text (a Select's "Select an item", screen reader
  // announcements) and its date and number fields follow the app's
  // language, not the webview's.
  return (
    <LocaleContext value={info}>
      <AriaI18nProvider locale={formatLocale()}>{children}</AriaI18nProvider>
    </LocaleContext>
  );
}

/** A language by its own name ("Deutsch", "Svenska"), or its tag. */
export function languageName(tag: string): string {
  try {
    const name = new Intl.DisplayNames([tag], { type: "language" }).of(tag);
    if (name) return name.charAt(0).toLocaleUpperCase(tag) + name.slice(1);
  } catch {
    // Not a tag this engine knows.
  }
  return tag;
}

interface LanguageOption {
  id: string;
  label: string;
}

/** The Language setting's choices: System, showing what it resolves to,
 * then each language by its own name with how much of it is translated.
 * `current`, the setting's value, is kept when it isn't one of them. */
export function useLanguageOptions(current?: string): LanguageOption[] {
  const { t } = useLingui();
  const { system, available } = useLocale();
  const systemName = languageName(system);
  const options: LanguageOption[] = [
    {
      id: SYSTEM,
      label: t({
        message: `System (${systemName})`,
        comment: "The Language setting's first choice: the OS's language, then the one it means",
      }),
    },
  ];
  for (const tag of available) {
    const name = languageName(tag);
    const share = translated[tag];
    if (tag === sourceLocale || share === undefined || share >= 1) {
      options.push({ id: tag, label: name });
      continue;
    }
    const percent = Math.floor(share * 100);
    options.push({
      id: tag,
      label: t({
        message: `${name} (${percent}% translated)`,
        comment: "A language in the Language setting, and how much of Caskade is translated to it",
      }),
    });
  }
  if (current && !options.some((option) => option.id === current)) {
    const name = languageName(current);
    options.push({
      id: current,
      label: t({
        message: `${name} (not available, shown in English)`,
        comment: "A language chosen earlier that this version of Caskade doesn't have",
      }),
    });
  }
  return options;
}
