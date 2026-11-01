/** `pnpm i18n:compile`: compiles every language's catalogs the way the build
 * does, prints how much of each is translated, and fails when a translation
 * doesn't compile. The build compiles them itself, through the Vite plugin. */
import { existsSync } from "node:fs";
import {
  compileDomain,
  defaultRoot,
  domains,
  readCatalog,
  templateFile,
  translatedLocales,
} from "../src/vite.ts";

let failed = false;
for (const domain of domains) {
  const pot = templateFile(defaultRoot, domain);
  const total = existsSync(pot) ? Object.keys(await readCatalog(pot)).length : 0;
  for (const locale of translatedLocales()) {
    const { messages, errors } = await compileDomain(locale, domain);
    for (const error of errors) console.error(`${domain}: ${error}`);
    failed ||= errors.length > 0;
    const done = Object.keys(messages).length;
    const percent = total ? Math.floor((done / total) * 100) : 100;
    console.log(`${locale}/${domain}: ${done} of ${total} messages translated (${percent} %)`);
  }
}
if (failed) process.exit(1);
