/** Extraction: marked strings into the templates (`locales/<domain>.pot`),
 * then the templates merged into every language's PO file by GNU gettext's
 * msgmerge, which keeps translations, marks changed entries fuzzy and keeps
 * removed ones as obsolete `#~` entries. */
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { getCatalogs } from "@lingui/cli/api";
import type { CatalogType, ExtractedCatalogType } from "@lingui/conf";
import {
  catalogFile,
  type Domain,
  defaultRoot,
  domains,
  linguiConfig,
  templateFile,
  translatedLocales,
} from "./vite.ts";

export interface Template {
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

/** Extracts every domain's template without writing it. */
export async function extractTemplates(root = defaultRoot): Promise<Template[]> {
  const catalogs = await getCatalogs(linguiConfig(root));
  return await Promise.all(
    catalogs.map(async (catalog) => {
      const domain = catalog.name as Domain;
      const messages = await catalog.collect();
      if (!messages) throw new Error(`Extracting the ${domain} domain failed`);
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
