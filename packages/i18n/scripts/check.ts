/** `pnpm i18n:check` (with the core's `caskade-i18n extract --check`, `just
 * i18n-check`; CI runs both): fails, naming the file and the entry, on a
 * stale webview template, a PO file `msgfmt --check` rejects, or a
 * translation that drops or changes its English's placeholders or tags.
 * Then prints how much of each language is translated. Needs GNU gettext's
 * msgfmt. See `docs/translating.md`. */
import { existsSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import {
  catalogProblems,
  msgfmtProblems,
  packageCatalogs,
  syntaxOf,
  templateChanges,
} from "../src/check.ts";
import { extractTemplates } from "../src/extract.ts";
import {
  allDomains,
  catalogFile,
  completion,
  defaultRoot,
  translatedLocales,
} from "../src/vite.ts";

const show = (file: string) => relative(defaultRoot, file);
const problems: string[] = [];

for (const template of await extractTemplates()) {
  if (!template.stale) continue;
  const before = existsSync(template.file) ? readFileSync(template.file, "utf8") : "";
  problems.push(`${show(template.file)} is stale; run \`pnpm i18n:extract\`:`);
  for (const change of templateChanges(before, template.content)) problems.push(`  ${change}`);
}

const catalogs = translatedLocales().flatMap((locale) =>
  allDomains.flatMap((domain) => {
    const file = catalogFile(defaultRoot, locale, domain);
    return existsSync(file) ? [{ file: show(file), domain: domain as string }] : [];
  }),
);
for (const file of packageCatalogs(defaultRoot)) catalogs.push({ file, domain: "plugin" });

try {
  for (const { file, domain } of catalogs) {
    problems.push(...msgfmtProblems(file, defaultRoot));
    // Plugins' and themes' catalogs get the placeholder checks once their
    // domains' syntax is settled (#219); msgfmt covers them meanwhile.
    if (domain === "plugin") continue;
    const text = readFileSync(join(defaultRoot, file), "utf8");
    problems.push(...catalogProblems(file, text, syntaxOf(domain)));
  }
} catch (error) {
  console.error((error as Error).message);
  process.exit(2);
}

for (const [locale, byDomain] of Object.entries(completion())) {
  const counts = Object.entries(byDomain);
  const done = counts.reduce((sum, [, count]) => sum + count.translated, 0);
  const total = counts.reduce((sum, [, count]) => sum + count.total, 0);
  const percent = (translated: number, of: number) =>
    `${of ? Math.floor((translated / of) * 100) : 100} %`;
  const parts = counts.map(([domain, count]) => `${domain} ${count.translated}/${count.total}`);
  console.log(`${locale}: ${percent(done, total)} translated (${parts.join(", ")})`);
}

if (problems.length) {
  for (const problem of problems) console.error(problem);
  console.error("The translation catalogs need fixing (docs/translating.md).");
  process.exit(1);
}
console.log("The templates are current and every catalog passes.");
