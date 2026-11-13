/** Extraction: marked strings into the templates (`locales/<domain>.pot`),
 * then the templates merged into every language's PO file by GNU gettext's
 * msgmerge, which keeps translations, marks changed entries fuzzy and keeps
 * removed ones as obsolete `#~` entries. */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { getCatalogs } from "@lingui/cli/api";
import type { CatalogType, ExtractedCatalogType } from "@lingui/conf";
import { generateMessageId } from "@lingui/message-utils/generateMessageId";
import {
  catalogFile,
  type Domain,
  defaultRoot,
  domains,
  linguiConfig,
  templateFile,
  translatedLocales,
} from "./vite.ts";

interface Template {
  domain: Domain;
  file: string;
  /** The template as extraction writes it. */
  content: string;
  /** Whether that differs from the file on disk. */
  stale: boolean;
}

const collator = new Intl.Collator("en-US");

function sorted(messages: ExtractedCatalogType): CatalogType {
  const key = (id: string) => messages[id]?.message ?? id;
  const ids = Object.keys(messages).sort(
    (a, b) =>
      collator.compare(key(a), key(b)) ||
      collator.compare(messages[a]?.context ?? "", messages[b]?.context ?? "") ||
      collator.compare(a, b),
  );
  return Object.fromEntries(ids.map((id) => [id, messages[id] as CatalogType[string]]));
}

/** A message of the native UI (#91), as the core's extractor lists them in
 * `locales/native.json` (`just i18n` writes it from the `t!` family). */
interface NativeMessage {
  id: string;
  context?: string;
  plural?: string;
  comments?: string[];
  references: string[];
}

/** The native UI's messages; none before `just i18n` has listed them. */
function nativeMessages(root = defaultRoot): NativeMessage[] {
  const file = `${root}/locales/native.json`;
  if (!existsSync(file)) return [];
  return (JSON.parse(readFileSync(file, "utf8")) as { messages: NativeMessage[] }).messages;
}

/** A whole-message plural as Lingui extracts one, `{n, plural, one {…}
 * other {…}}`: its one and other forms, which the PO file's msgid and
 * msgid_plural are. */
const WHOLE_PLURAL = /^\{\s*\w+\s*,\s*plural\s*,\s*one\s*\{(.*)\}\s*other\s*\{(.*)\}\s*\}$/s;

function sameMessage(entry: ExtractedCatalogType[string], native: NativeMessage): boolean {
  if ((entry.context ?? undefined) !== (native.context ?? undefined)) return false;
  const message = entry.message ?? "";
  if (native.plural === undefined) return message === native.id;
  const forms = WHOLE_PLURAL.exec(message);
  return forms?.[1] === native.id && forms?.[2] === native.plural;
}

/** Adds the native UI's messages to the webview's catalogs, so both share
 * the English, the contexts and the translations. A message the webview
 * has gains the native file as a reference; the rest are added to `app`,
 * unless `ui` has them. The webview's entries keep their order. */
export function addNative(
  catalogs: Partial<Record<Domain, ExtractedCatalogType>>,
  native: readonly NativeMessage[],
): void {
  for (const message of native) {
    const found = (["ui", "app"] as const).flatMap((domain) => {
      const catalog = catalogs[domain];
      if (!catalog) return [];
      const key = Object.keys(catalog).find((id) => {
        const entry = catalog[id];
        return entry !== undefined && sameMessage(entry, message);
      });
      return key === undefined ? [] : [{ catalog, key }];
    })[0];
    const origins = message.references.map((file) => [file] as [string]);
    if (found) {
      const entry = found.catalog[found.key];
      if (!entry) continue;
      entry.origin = [...(entry.origin ?? []), ...origins];
      for (const comment of message.comments ?? []) {
        if (!entry.comments?.includes(comment))
          entry.comments = [...(entry.comments ?? []), comment];
      }
      continue;
    }
    const app = catalogs.app;
    if (!app) continue;
    const text =
      message.plural === undefined
        ? message.id
        : `{count, plural, one {${message.id}} other {${message.plural}}}`;
    app[generateMessageId(text, message.context)] = {
      message: text,
      context: message.context,
      comments: message.comments ?? [],
      origin: origins,
      placeholders: {},
    } as ExtractedCatalogType[string];
  }
}

/** Extracts every domain's template without writing it. */
export async function extractTemplates(root = defaultRoot): Promise<Template[]> {
  const catalogs = await getCatalogs(linguiConfig(root));
  const collected = await Promise.all(
    catalogs.map(async (catalog) => {
      const domain = catalog.name as Domain;
      const messages = await catalog.collect();
      if (!messages) throw new Error(`Extracting the ${domain} domain failed`);
      return { catalog, domain, messages };
    }),
  );
  addNative(
    Object.fromEntries(collected.map(({ domain, messages }) => [domain, messages])),
    nativeMessages(root),
  );
  return await Promise.all(
    collected.map(async ({ catalog, domain, messages }) => {
      const file = templateFile(root, domain);
      const existing = existsSync(file) ? readFileSync(file, "utf8") : undefined;
      // The existing header (its POT-Creation-Date) is kept, so extracting
      // unchanged code writes the same file.
      const content = await catalog.format.serialize(file, sorted(messages), undefined, existing);
      return { domain, file, content, stale: content !== existing };
    }),
  );
}

/** Writes the stale templates; returns the files written. */
export async function writeTemplates(root = defaultRoot): Promise<string[]> {
  const written: string[] = [];
  for (const template of await extractTemplates(root)) {
    if (!template.stale) continue;
    mkdirSync(dirname(template.file), { recursive: true });
    writeFileSync(template.file, template.content);
    written.push(template.file);
  }
  return written;
}

/** Starts `po` from `pot` with msginit, which fills in the language's
 * header (`Language`, `Plural-Forms`). */
function startCatalog(locale: string, pot: string, po: string): void {
  mkdirSync(dirname(po), { recursive: true });
  execFileSync(
    "msginit",
    [
      "--no-translator",
      "--no-wrap",
      `--locale=${locale.replace("-", "_")}`,
      `--input=${pot}`,
      `--output-file=${po}`,
    ],
    { stdio: ["ignore", "ignore", "inherit"] },
  );
}

/** Merges the templates into every language's PO files, starting the ones
 * missing (and the languages in `add`) with msginit. A missing `core.po`
 * is started too; the core's extractor (`just i18n`) merges it. Needs GNU
 * gettext. */
export function mergeCatalogs(root = defaultRoot, add: readonly string[] = []): void {
  for (const locale of new Set([...translatedLocales(root), ...add])) {
    const corePot = templateFile(root, "core");
    const corePo = catalogFile(root, locale, "core");
    if (existsSync(corePot) && !existsSync(corePo)) startCatalog(locale, corePot, corePo);
    for (const domain of domains) {
      const pot = templateFile(root, domain);
      // A domain with no template yet has nothing to merge.
      if (!existsSync(pot)) continue;
      const po = catalogFile(root, locale, domain);
      if (!existsSync(po)) startCatalog(locale, pot, po);
      // Also after msginit, so a new file is in msgmerge's layout already.
      // --update leaves the file alone when nothing changed.
      execFileSync(
        "msgmerge",
        ["--quiet", "--update", "--backup=none", "--no-wrap", "--previous", po, pot],
        { stdio: ["ignore", "ignore", "inherit"] },
      );
    }
  }
}
