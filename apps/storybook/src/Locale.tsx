/** The Language toolbar's decorator: @caskade/ui's catalog for the chosen
 * language, the way the app loads it. English needs none. */
import { activateLocale, i18n } from "@caskade/i18n";
import { I18nProvider } from "@lingui/react";
import { type ReactNode, useEffect } from "react";

export function Locale({ locale, children }: { locale: string; children: ReactNode }) {
  useEffect(() => {
    if (i18n.locale !== locale) void activateLocale(locale, ["ui"]);
  }, [locale]);
  return <I18nProvider i18n={i18n}>{children}</I18nProvider>;
}
