/// <reference path="./modules.d.ts" />
/** The webview's i18n runtime: the global Lingui `i18n` (the handle for code
 * outside React), the languages this build has, and loading one language's
 * compiled catalogs. English is the source text in the code and needs no
 * catalog; whatever a catalog lacks shows English. */

import { catalogs, completeness, pseudoLocale } from "virtual:caskade-i18n/catalogs";
import { i18n, type Messages } from "@lingui/core";
import { type CompiledMessage, compileMessage } from "@lingui/message-utils/compileMessage";

export { i18n, pseudoLocale };

/** How much of each language's text is translated, from 0 to 1, as of the
 * build; the Language setting shows it. */
export const translated: Readonly<Record<string, number>> = completeness;

export type Domain = "ui" | "app";

export const sourceLocale = "en";

/** English, every language under `locales/`, and the pseudo-locale in dev
 * builds and tests. */
export const locales: readonly string[] = [sourceLocale, ...Object.keys(catalogs)];

// The macros leave the English text in the code. Lingui compiles those
// strings only in development, so the source language needs the compiler in
// release builds too; each message compiles once.
const compiled = new Map<string, CompiledMessage>();
i18n.setMessagesCompiler((message) => {
  let result = compiled.get(message);
  if (!result) {
    result = compileMessage(message);
    compiled.set(message, result);
  }
  return result;
});
if (!i18n.locale) i18n.loadAndActivate({ locale: sourceLocale, messages: {} });

/** One language's compiled messages for the given domains. */
export async function loadCatalog(
  locale: string,
  domains: readonly Domain[] = ["ui", "app"],
): Promise<Messages> {
  const loaders = catalogs[locale];
  if (!loaders) return {};
  const loaded = await Promise.all(domains.map((domain) => loaders[domain]?.()));
  const messages: Messages = {};
  for (const module of loaded) Object.assign(messages, module?.messages);
  return messages;
}

/** Loads a language and makes it the active one; an I18nProvider re-renders. */
export async function activateLocale(locale: string, domains?: readonly Domain[]): Promise<void> {
  const messages = locale === sourceLocale ? {} : await loadCatalog(locale, domains);
  i18n.loadAndActivate({ locale, messages });
}

/** A language asked for in the page's `?lang=` (`just web`, Storybook), if
 * this build has it. In the browser preview it stands in for the OS's
 * language; the app follows the core's (#213). */
export function requestedLocale(search = globalThis.location?.search ?? ""): string | undefined {
  const lang = new URLSearchParams(search).get("lang");
  return lang && locales.includes(lang) ? lang : undefined;
}
