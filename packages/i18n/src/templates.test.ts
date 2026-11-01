import { relative } from "node:path";
import { expect, it } from "vitest";
import { extractTemplates } from "./extract.ts";
import { defaultRoot } from "./vite.ts";

// Like `committed_bindings_are_current`: marked strings changed without
// `pnpm i18n:extract`, so translators can't see them.
it("the committed templates are current", async () => {
  const stale = (await extractTemplates())
    .filter((template) => template.stale)
    .map((template) => relative(defaultRoot, template.file));
  expect(stale, "run `pnpm i18n:extract` and commit locales/").toEqual([]);
});
