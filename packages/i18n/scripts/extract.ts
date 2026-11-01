/** `pnpm i18n:extract [--add <lang>…]`: writes the templates and merges them
 * into every language's PO files (`--add sv` starts a language).
 * `pnpm i18n:check` (`scripts/check.ts`) checks them. */
import { relative } from "node:path";
import { mergeCatalogs, writeTemplates } from "../src/extract.ts";
import { defaultRoot } from "../src/vite.ts";

const args = process.argv.slice(2);
const show = (file: string) => relative(defaultRoot, file);

const add = args.flatMap((arg, index) => (args[index - 1] === "--add" ? [arg] : []));
for (const file of await writeTemplates()) console.log(`Wrote ${show(file)}`);
mergeCatalogs(defaultRoot, add);
console.log("Merged the templates into the PO files.");
