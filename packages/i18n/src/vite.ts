/// <reference types="node" />
/** The Node side of the webview's gettext pipeline: the Lingui configuration,
 * reading PO and POT files, compiling catalogs and the Vite plugin that does
 * both at build time. Imported by the Vite, Vitest and Storybook configs and
 * by the scripts, so it has no relative imports (see `docs/translating.md`). */
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createCompiledCatalog } from "@lingui/cli/api";
import { type CatalogType, type LinguiConfigNormalized, makeConfig } from "@lingui/conf";
import { formatter } from "@lingui/format-po-gettext";
import { createSwcExtractor, mapMacroOptions, transform } from "@lingui/native-tools";
import type { Plugin } from "vite";

/** Text domains: `ui` is @caskade/ui (shared with plugins and Storybook),
 * `app` is the rest of the webview. */
export const domains = ["ui", "app"] as const;
export type Domain = (typeof domains)[number];

/** Every domain in `locales/`: the webview's and `core`, the Rust side's
 * (extracted by `just i18n`). A language's completion counts them all. */
export const allDomains = [...domains, "core"] as const;
type CatalogDomain = (typeof allDomains)[number];

/** The source language. Its text is in the code, so it has no catalog. */
const sourceLocale = "en";

/** Accented, bracketed and about 35 % longer, to find unmarked strings and
 * labels that overflow. Generated from the templates, never translated. */
export const pseudoLocale = {
  locale: "en-XA",
  prepend: "[",
  append: "]",
  extend: 0.35,
  // Spaces would collapse in HTML and hide the extra length.
  extendCharacter: "~",
};

/** The repository: `locales/` and the sources of each domain are under it. */
export const defaultRoot = fileURLToPath(new URL("../../..", import.meta.url));

const sources: Record<Domain, string> = { ui: "packages/ui/src", app: "apps/desktop/src" };

/** Gettext PO with native plurals (`msgid_plural` / `msgstr[n]`). References
 * name files without line numbers, so moving code doesn't stale a template. */
const poFormat = formatter({ lineNumbers: false });

export const templateFile = (root: string, domain: CatalogDomain) =>
  join(root, "locales", `${domain}.pot`);

export const catalogFile = (root: string, locale: string, domain: CatalogDomain) =>
  join(root, "locales", locale, `${domain}.po`);

/** Languages with a catalog: every `locales/<lang>/` holding a domain's PO
 * file. Adding one is all it takes to ship a language. */
export function translatedLocales(root = defaultRoot): string[] {
  const dir = join(root, "locales");
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isDirectory() &&
        entry.name !== pseudoLocale.locale &&
        allDomains.some((domain) => existsSync(catalogFile(root, entry.name, domain))),
    )
    .map((entry) => entry.name)
    .sort();
}

export function linguiConfig(root = defaultRoot): LinguiConfigNormalized {
  return makeConfig(
    {
      rootDir: root,
      sourceLocale,
      locales: [sourceLocale, ...translatedLocales(root), pseudoLocale.locale],
      pseudoLocale,
      catalogs: domains.map((domain) => {
        const src = join(root, sources[domain]);
        return {
          path: join(root, "locales", "{locale}", domain),
          include: [src],
          exclude: [join(src, "**/*.test.*"), join(src, "**/*.stories.*"), join(src, "test/**")],
        };
      }),
      format: poFormat,
      extractors: [createSwcExtractor()],
      // Components rendered outside an I18nProvider (unit tests, portals)
      // fall back to the global i18n instead of throwing.
      runtimeConfigModule: {
        Trans: ["@caskade/i18n/react", "Trans"],
        useLingui: ["@caskade/i18n/react", "useLingui"],
      },
    },
    { skipValidation: true },
  );
}

/** Reads a PO or POT file into Lingui's catalog shape. A template's Language
 * header is empty, and the gettext format needs one for its plural rules.
 * Line endings are normalised first: a Windows checkout may have CRLF. */
export async function readCatalog(
  file: string,
  raw = readFileSync(file, "utf8"),
): Promise<CatalogType> {
  const content = raw.replace(/\r\n/g, "\n");
  const source = /^"Language: *[^\s\\"]/m.test(content)
    ? content
    : content
        .replace(/^"Language:.*\n/m, "")
        .replace(/^msgstr ""$/m, `msgstr ""\n"Language: ${sourceLocale}\\n"`);
  return await poFormat.parse(source, { locale: undefined, sourceLocale, filename: file });
}

/** One entry of a PO or POT file as written, for the checks and the
 * completion count: the core's catalogs aren't Lingui's. */
export interface PoEntry {
  /** The line of its `msgid`, from 1. */
  line: number;
  context?: string;
  /** Empty for the header. */
  id: string;
  plural?: string;
  /** `msgstr`, or `msgstr[0]`, `msgstr[1]`, … for a plural. */
  translations: string[];
  flags: string[];
  /** A `#~` entry, kept by msgmerge after its message went. */
  obsolete: boolean;
}

const escapes: Record<string, string> = { n: "\n", t: "\t", r: "\r" };
const unescapePo = (text: string) =>
  text.replace(/\\(.)/g, (_, char: string) => escapes[char] ?? char);

/** The entries of a PO or POT file, header included. */
export function parsePo(text: string): PoEntry[] {
  const entries: PoEntry[] = [];
  let entry: PoEntry | undefined;
  let field: "context" | "id" | "plural" | number | undefined;
  const finish = () => {
    if (entry && field !== undefined) entries.push(entry);
    entry = undefined;
    field = undefined;
  };
  const start = (line: number) => {
    entry ??= { line, id: "", translations: [], flags: [], obsolete: false };
    return entry;
  };
  const append = (value: string) => {
    if (!entry || field === undefined) return;
    if (typeof field === "number") {
      entry.translations[field] = (entry.translations[field] ?? "") + value;
    } else entry[field] = (entry[field] ?? "") + value;
  };
  for (const [index, raw] of text.split(/\r?\n/).entries()) {
    let line = raw.trim();
    const obsolete = line.startsWith("#~");
    if (obsolete) {
      if (line.startsWith("#~|")) continue;
      line = line.slice(2).trim();
    }
    if (!line) {
      if (!obsolete) finish();
      continue;
    }
    if (line.startsWith("#")) {
      // A comment after a translation starts the next entry.
      if (typeof field === "number") finish();
      const current = start(index + 1);
      if (line.startsWith("#,")) {
        current.flags.push(
          ...line
            .slice(2)
            .split(",")
            .map((flag) => flag.trim())
            .filter(Boolean),
        );
      }
      continue;
    }
    const keyword = /^(msgctxt|msgid_plural|msgid|msgstr)(?:\[(\d+)\])?\s*"(.*)"$/.exec(line);
    if (keyword) {
      const [, name, form, value = ""] = keyword;
      if ((name === "msgctxt" || name === "msgid") && typeof field === "number") finish();
      const current = start(index + 1);
      current.obsolete ||= obsolete;
      if (name === "msgstr") {
        field = Number(form ?? 0);
        current.translations[field] ??= "";
      } else {
        field = name === "msgctxt" ? "context" : name === "msgid" ? "id" : "plural";
        if (field === "id") current.line = index + 1;
        current[field] = "";
      }
      append(unescapePo(value));
      continue;
    }
    const continued = /^"(.*)"$/.exec(line);
    if (continued) append(unescapePo(continued[1] ?? ""));
  }
  finish();
  return entries;
}

/** What identifies a message: its context and English text. */
export const entryKey = (entry: PoEntry) => `${entry.context ?? ""}\u0004${entry.id}`;

/** The header, and entries msgmerge kept after their message went, aren't
 * messages. */
export const isMessage = (entry: PoEntry) =>
  !entry.obsolete && (entry.id !== "" || entry.context !== undefined);

/** Translated: every form filled in and not fuzzy, so it doesn't show English. */
const isTranslated = (entry: PoEntry) =>
  isMessage(entry) &&
  !entry.flags.includes("fuzzy") &&
  entry.translations.length > 0 &&
  entry.translations.every(Boolean);

const flags = (message: CatalogType[string]) =>
  (message.extra as { flags?: string[] } | undefined)?.flags ?? [];

/** The translations a catalog contributes: entries that are translated, not
 * fuzzy and not obsolete, and still in the template when there is one.
 * Everything else shows the English source text. */
function usableTranslations(catalog: CatalogType, template?: CatalogType): Record<string, string> {
  const usable: Record<string, string> = {};
  for (const [id, message] of Object.entries(catalog)) {
    if (!message.translation || message.obsolete || flags(message).includes("fuzzy")) continue;
    if (template && !(id in template)) continue;
    usable[id] = message.translation;
  }
  return usable;
}

interface CompiledCatalog {
  /** Compiled messages by id, for `i18n.load`. */
  messages: Record<string, unknown>;
  /** Translations that failed to compile; those show English. */
  errors: string[];
  /** The files it was compiled from. */
  files: string[];
}

function compile(locale: string, messages: Record<string, string>, pseudo: boolean) {
  const { source, errors } = createCompiledCatalog(locale, messages, {
    namespace: "json",
    strict: true,
    ...(pseudo ? { pseudoLocale: locale, pseudoLocaleOptions: pseudoLocale } : {}),
  });
  // A message that failed to compile is left out.
  return {
    messages: (JSON.parse(source) as { messages: Record<string, unknown> }).messages,
    errors: errors.map((error) => `${locale}: ${JSON.stringify(error.source)}: ${error.error}`),
  };
}

/** One language's catalog for one domain, from `locales/<lang>/<domain>.po`
 * and the domain's template; the pseudo-locale is made from the template. */
export async function compileDomain(
  locale: string,
  domain: Domain,
  root = defaultRoot,
): Promise<CompiledCatalog> {
  const pot = templateFile(root, domain);
  const template = existsSync(pot) ? await readCatalog(pot) : {};
  if (locale === pseudoLocale.locale) {
    const source = Object.fromEntries(
      Object.entries(template).map(([id, message]) => [id, message.message ?? id]),
    );
    return { ...compile(locale, source, true), files: [pot] };
  }
  const po = catalogFile(root, locale, domain);
  if (!existsSync(po)) return { messages: {}, errors: [], files: [pot] };
  const translations = usableTranslations(await readCatalog(po), template);
  return { ...compile(locale, translations, false), files: [po, pot] };
}

/** A PO file on its own (a test catalog, later a plugin's), without a template. */
export async function compilePoFile(
  file: string,
  content = readFileSync(file, "utf8"),
): Promise<CompiledCatalog> {
  const catalog = await readCatalog(file, content);
  const locale = /^"Language: *([^\\"]+)/m.exec(content)?.[1] ?? "und";
  return { ...compile(locale, usableTranslations(catalog), false), files: [file] };
}

const catalogsId = "virtual:caskade-i18n/catalogs";
const catalogPrefix = "virtual:caskade-i18n/catalog/";
const macroImport = /from\s*["']@lingui\/(?:core|react)\/macro["']/;

/** A domain's messages and how many of them a language translates. */
interface Completion {
  translated: number;
  total: number;
}

/** Each shipped language's completion per domain (`allDomains`): its
 * translated entries that are still in the domain's template, over the
 * template's messages. A missing PO file counts as nothing translated. */
export function completion(root = defaultRoot): Record<string, Record<CatalogDomain, Completion>> {
  const read = (file: string) => (existsSync(file) ? parsePo(readFileSync(file, "utf8")) : []);
  const templates = allDomains.map(
    (domain) => new Set(read(templateFile(root, domain)).filter(isMessage).map(entryKey)),
  );
  const result: Record<string, Record<CatalogDomain, Completion>> = {};
  for (const locale of translatedLocales(root)) {
    const domainsDone = allDomains.map((domain, index) => {
      const template = templates[index] ?? new Set<string>();
      const translated = read(catalogFile(root, locale, domain)).filter(
        (entry) => isTranslated(entry) && template.has(entryKey(entry)),
      ).length;
      return [domain, { translated, total: template.size }] as const;
    });
    result[locale] = Object.fromEntries(domainsDone) as Record<CatalogDomain, Completion>;
  }
  return result;
}

/** How much of each language is translated, from 0 to 1: its translated
 * messages over the templates', every domain (`allDomains`) together. The
 * Language setting shows it. */
export async function completeness(root = defaultRoot): Promise<Record<string, number>> {
  const result: Record<string, number> = {};
  for (const [locale, byDomain] of Object.entries(completion(root))) {
    const counts = Object.values(byDomain);
    const total = counts.reduce((sum, count) => sum + count.total, 0);
    const done = counts.reduce((sum, count) => sum + count.translated, 0);
    result[locale] = total ? done / total : 1;
  }
  return result;
}

async function catalogsModule(root: string, pseudo: boolean): Promise<string> {
  const entries: [string, readonly Domain[]][] = translatedLocales(root).map((locale) => [
    locale,
    domains.filter((domain) => existsSync(catalogFile(root, locale, domain))),
  ]);
  if (pseudo) entries.push([pseudoLocale.locale, domains]);
  const loaders = entries.map(
    ([locale, present]) =>
      `  ${JSON.stringify(locale)}: {${present
        .map(
          (domain) =>
            `${domain}: () => import(${JSON.stringify(`${catalogPrefix}${locale}/${domain}`)})`,
        )
        .join(", ")}},`,
  );
  return [
    `export const pseudoLocale = ${pseudo ? JSON.stringify(pseudoLocale.locale) : "undefined"};`,
    `export const catalogs = {\n${loaders.join("\n")}\n};`,
    `export const completeness = ${JSON.stringify(await completeness(root))};`,
  ].join("\n");
}

const messagesModule = (messages: Record<string, unknown>) =>
  `export const messages = JSON.parse(${JSON.stringify(JSON.stringify(messages))});`;

interface I18nPluginOptions {
  /** The repository root; by default the one this package is in. */
  root?: string;
  /** Offer the pseudo-locale. By default in dev servers, tests and debug app
   * builds (`TAURI_ENV_DEBUG`), not in release builds. */
  pseudo?: boolean;
}

/** Lingui's macros, transformed natively (no Babel), and the catalogs:
 * `virtual:caskade-i18n/catalogs` lists a lazy loader per language and
 * domain, each compiled from its PO file into a chunk of its own, and a
 * `.po` file imported directly compiles on its own. */
export function caskadeI18n(options: I18nPluginOptions = {}): Plugin {
  const root = options.root ?? defaultRoot;
  let pseudo = options.pseudo ?? true;
  let macro: ReturnType<typeof mapMacroOptions> | undefined;
  return {
    name: "caskade-i18n",
    enforce: "pre",
    configResolved(config) {
      pseudo = options.pseudo ?? (!config.isProduction || Boolean(process.env.TAURI_ENV_DEBUG));
    },
    resolveId(id) {
      if (id === catalogsId || id.startsWith(catalogPrefix)) return `\0${id}`;
    },
    async load(id) {
      if (id === `\0${catalogsId}`) {
        return { code: await catalogsModule(root, pseudo), moduleType: "js" };
      }
      if (id.startsWith(`\0${catalogPrefix}`)) {
        const [locale = "", domain] = id.slice(catalogPrefix.length + 1).split("/");
        const compiled = await compileDomain(locale, domain as Domain, root);
        for (const file of compiled.files) this.addWatchFile(file);
        for (const error of compiled.errors) this.warn(error);
        return { code: messagesModule(compiled.messages), moduleType: "js" };
      }
      const file = id.replace(/\?.*$/, "");
      if (file.endsWith(".po")) {
        const compiled = await compilePoFile(file);
        this.addWatchFile(file);
        for (const error of compiled.errors) this.warn(error);
        return { code: messagesModule(compiled.messages), moduleType: "js" };
      }
    },
    transform: {
      filter: { id: /\.[cm]?[jt]sx?(?:\?|$)/, code: macroImport },
      async handler(code, id) {
        const file = id.replace(/\?.*$/, "");
        if (file.includes("/node_modules/") || !macroImport.test(code)) return;
        // English stays in the code (`message`), so the source language
        // needs no catalog and a missing translation falls back to it.
        macro ??= mapMacroOptions(linguiConfig(root), { descriptorFields: "message" });
        const result = await transform(code, basename(file), { macro });
        return { code: result.code, map: result.map };
      },
    },
  };
}
