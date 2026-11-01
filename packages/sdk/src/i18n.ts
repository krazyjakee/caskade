/** A UI plugin's own translations (#219). The plugin ships PO catalogs in
 * `locales/<lang>.po`, in a domain named after its id, with the English
 * text as the message id. The host reads the catalog of the language
 * Caskade is in and hands it to the plugin as `ctx.i18n`; `useTranslation`
 * and `<Trans>` read it inside contributions. When the language changes,
 * Caskade activates the plugin again with the new catalog, so registered
 * titles and rendered text follow it. Anything a catalog lacks shows its
 * English. */

/** A plugin's catalog for the language in use, as the host sends it. */
export interface PluginMessages {
  /** The catalog's language; `en` when the plugin has none for it. */
  locale: string;
  messages: { context: string | null; id: string; translations: string[] }[];
  /** The plural form for each count below 200; a larger count `n` takes
   * the form of `100 + n % 100`. */
  plurals: number[];
}

/** Values for `{name}` placeholders. */
export type MessageValues = Record<string, string | number>;

export interface MessageOptions {
  /** gettext's `msgctxt`, for the same English with different meanings. */
  context?: string;
}

export interface PluginI18n {
  /** The language the text is in: a BCP 47 tag, `en` for English. */
  locale: string;
  /** `message` in the plugin's catalog, placeholders filled in:
   * `t("Installed {game}", { game: title })`. */
  t(message: string, values?: MessageValues, options?: MessageOptions): string;
  /** The singular or a plural form for `count`, which fills `{n}`:
   * `plural(count, "One pack", "{n} packs")`. */
  plural(
    count: number,
    one: string,
    other: string,
    values?: MessageValues,
    options?: MessageOptions,
  ): string;
}

const key = (id: string, context?: string | null) => (context ? `${context}\u0004${id}` : id);

/** Fills `{name}` placeholders; unknown ones stay as written. */
export function fill(text: string, values: MessageValues = {}): string {
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in values ? String(values[name]) : whole,
  );
}

/** A plugin's `t` and `plural` over its catalog; English without one. */
export function createPluginI18n(catalog?: PluginMessages | null): PluginI18n {
  const translations = new Map<string, string[]>();
  for (const message of catalog?.messages ?? [])
    translations.set(key(message.id, message.context), message.translations);
  const plurals = catalog?.plurals ?? [];
  const form = (count: number) => {
    const n = Math.max(0, Math.floor(Math.abs(count)));
    const index = n < plurals.length ? plurals[n] : plurals[100 + (n % 100)];
    return index ?? (n === 1 ? 0 : 1);
  };
  return {
    locale: catalog?.locale ?? "en",
    t(message, values, options) {
      const translated = translations.get(key(message, options?.context))?.[0];
      return fill(translated || message, values);
    },
    plural(count, one, other, values, options) {
      const forms = translations.get(key(one, options?.context));
      const index = form(count);
      const english = count === 1 ? one : other;
      return fill(forms?.[index] || english, { n: count, ...values });
    },
  };
}
