import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setupI18n } from "@lingui/core";
import { generateMessageId } from "@lingui/message-utils/generateMessageId";
import { afterEach, beforeEach, expect, it } from "vitest";
import { mergeCatalogs } from "./extract.ts";
import { compileDomain, compilePoFile, completeness, pseudoLocale } from "./vite.ts";

const plural = "{count, plural, one {# game} other {# games}}";
const pluralComment = `#. js-lingui:${new URLSearchParams({ icu: plural, pluralize_on: "count" })}`;

const template = `msgid ""
msgstr ""
"POT-Creation-Date: 2026-10-29 00:00+0000\\n"
"Content-Type: text/plain; charset=utf-8\\n"
"Language: \\n"

${pluralComment}
msgid "# game"
msgid_plural "# games"
msgstr[0] ""
msgstr[1] ""

msgid "Library"
msgstr ""

msgctxt "navigation"
msgid "Home"
msgstr ""

msgid "Missing"
msgstr ""

msgid "Search"
msgstr ""

msgid "Hello {name}"
msgstr ""
`;

const swedish = `msgid ""
msgstr ""
"Language: sv\\n"
"Content-Type: text/plain; charset=UTF-8\\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\\n"

${pluralComment}
msgid "# game"
msgid_plural "# games"
msgstr[0] "# spel"
msgstr[1] "# spelen"

msgid "Library"
msgstr "Bibliotek"

msgctxt "navigation"
msgid "Home"
msgstr "Hem"

msgid "Missing"
msgstr ""

#, fuzzy
#| msgid "Search games"
msgid "Search"
msgstr "Sök spel"

msgid "Hello {name}"
msgstr "Hej {name"

#~ msgid "Gone"
#~ msgstr "Borta"
`;

let root: string;
beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "caskade-i18n-"));
  mkdirSync(join(root, "locales", "sv"), { recursive: true });
  writeFileSync(join(root, "locales", "app.pot"), template);
  writeFileSync(join(root, "locales", "sv", "app.po"), swedish);
});
afterEach(() => rmSync(root, { recursive: true, force: true }));

const id = (message: string, context?: string) => generateMessageId(message, context);

async function swedishI18n() {
  const { messages, errors } = await compileDomain("sv", "app", root);
  const i18n = setupI18n({ locale: "sv", messages: { sv: messages as never } });
  return { i18n, messages, errors };
}

it("shows a translated entry, with its context", async () => {
  const { i18n } = await swedishI18n();
  expect(i18n._({ id: id("Library"), message: "Library" })).toBe("Bibliotek");
  expect(i18n._({ id: id("Home", "navigation"), message: "Home" })).toBe("Hem");
});

it("shows English for missing, fuzzy and broken entries, and leaves them out", async () => {
  const { i18n, messages, errors } = await swedishI18n();
  expect(i18n._({ id: id("Missing"), message: "Missing" })).toBe("Missing");
  expect(i18n._({ id: id("Search"), message: "Search" })).toBe("Search");
  expect(i18n._({ id: id("Hello {name}"), message: "Hello {name}", values: { name: "Ada" } })).toBe(
    "Hello Ada",
  );
  expect(Object.keys(messages)).not.toContain(id("Search"));
  expect(Object.keys(messages)).not.toContain(id("Gone"));
  expect(errors).toHaveLength(1);
});

it("turns gettext plurals into the language's plural forms", async () => {
  const { i18n } = await swedishI18n();
  const games = (count: number) => i18n._({ id: id(plural), message: plural, values: { count } });
  expect(games(1)).toBe("1 spel");
  expect(games(3)).toBe("3 spelen");
});

it("makes the pseudo-locale from the template", async () => {
  const { messages } = await compileDomain(pseudoLocale.locale, "app", root);
  const i18n = setupI18n({ locale: "en-XA", messages: { "en-XA": messages as never } });
  const library = i18n._({ id: id("Library"), message: "Library" });
  expect(library).toMatch(/^\[.*\]$/);
  expect(library).not.toContain("Library");
  expect(library.length).toBeGreaterThan("[Library]".length);
  expect(i18n._({ id: id("Hello {name}"), values: { name: "Ada" } })).toContain("Ada");
});

it("counts how much of a language is translated, leaving out fuzzy and obsolete entries", async () => {
  // Translated: the plural, "Library", "Home" and "Hello {name}" of six.
  expect(await completeness(root)).toEqual({ sv: 4 / 6 });
});

it("compiles a PO file on its own", async () => {
  const file = join(root, "locales", "sv", "app.po");
  const { messages } = await compilePoFile(file);
  expect(Object.keys(messages).sort()).toEqual(
    [id("Home", "navigation"), id("Library"), id(plural)].sort(),
  );
});

const hasGettext = (() => {
  try {
    execFileSync("msgmerge", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
})();

it.skipIf(!hasGettext)("merges a template into the PO files, the same each time", () => {
  // "Library" became "Library view": its translation comes along, fuzzy.
  writeFileSync(
    join(root, "locales", "app.pot"),
    template.replace('msgid "Library"', 'msgid "Library view"'),
  );
  mergeCatalogs(root, ["de"]);
  const po = join(root, "locales", "sv", "app.po");
  const merged = readFileSync(po, "utf8");
  expect(merged).toMatch(/#, fuzzy\n#\| msgid "Library"\nmsgid "Library view"\nmsgstr "Bibliotek"/);
  expect(merged).toContain('#~ msgid "Gone"');
  expect(merged).toContain('msgstr "Hem"');
  const started = readFileSync(join(root, "locales", "de", "app.po"), "utf8");
  expect(started).toContain('"Language: de\\n"');
  mergeCatalogs(root);
  expect(readFileSync(po, "utf8")).toBe(merged);
  expect(readFileSync(join(root, "locales", "de", "app.po"), "utf8")).toBe(started);
});
