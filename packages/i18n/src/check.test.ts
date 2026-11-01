import { describe, expect, it } from "vitest";
import { catalogProblems, entryProblems, rustPlaceholders, templateChanges } from "./check.ts";
import { completion, parsePo } from "./vite.ts";

const header = `msgid ""
msgstr ""
"Language: sv\\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\\n"
`;

const entries = (body: string) => parsePo(`${header}\n${body}`);
const problems = (body: string, syntax: "icu" | "rust" = "icu") =>
  entries(body).flatMap((entry) => entryProblems(entry, syntax));

describe("parsePo", () => {
  it("reads contexts, plurals, flags, continued strings and obsolete entries", () => {
    const [head, home, games, fuzzy, gone] = entries(`msgctxt "navigation"
msgid "Home"
msgstr "Hem"

msgid "One game"
msgid_plural "{n} games"
msgstr[0] "Ett spel"
msgstr[1] ""
"{n} spel"

#, fuzzy
#| msgid "Search games"
msgid "Search"
msgstr "Sök \\"spel\\""

#~ msgid "Gone"
#~ msgstr "Borta"
`);
    expect(head?.id).toBe("");
    expect(home).toMatchObject({
      line: 7,
      context: "navigation",
      id: "Home",
      translations: ["Hem"],
    });
    expect(games).toMatchObject({ plural: "{n} games", translations: ["Ett spel", "{n} spel"] });
    expect(fuzzy).toMatchObject({ flags: ["fuzzy"], translations: ['Sök "spel"'] });
    expect(gone).toMatchObject({ id: "Gone", obsolete: true });
  });
});

describe("ICU placeholders and tags (ui, app)", () => {
  it("passes translations that keep them, in any order", () => {
    expect(
      problems(`msgid "{latest} is out <0>· you have {installed}</0>"
msgstr "<0>Du har {installed} ·</0> {latest} finns"

msgid "Hello"
msgstr "Hej"

msgid "Missing {name}"
msgstr ""

#, fuzzy
msgid "Fuzzy {name}"
msgstr "Luddig"
`),
    ).toEqual([]);
  });

  it("fails a dropped, renamed or added placeholder", () => {
    expect(problems(`msgid "Couldn't start {game}"\nmsgstr "Kunde inte starta"\n`)).toEqual([
      "msgstr drops {game}",
    ]);
    expect(problems(`msgid "Open {title}"\nmsgstr "Öppna {titel}"\n`)).toEqual([
      "msgstr drops {title}",
      "msgstr has {titel}, which the English doesn't",
    ]);
    expect(problems(`msgid "Played {0}"\nmsgstr "Spelade {0} {1}"\n`)).toEqual([
      "msgstr has {1}, which the English doesn't",
    ]);
  });

  it("fails a dropped or changed tag", () => {
    expect(
      problems(`msgid "<0>{guide}</0> opens the menu"\nmsgstr "{guide} öppnar menyn"\n`),
    ).toEqual(["msgstr has the tags none, the English </0> <0>"]);
    expect(
      problems(`msgid "<0>{guide}</0> opens the menu"\nmsgstr "<1>{guide}</1> öppnar menyn"\n`),
    ).toHaveLength(1);
  });

  it("fails a translation that doesn't parse", () => {
    const [problem] = problems(`msgid "Hello {name}"\nmsgstr "Hej {name"\n`);
    expect(problem).toMatch(/^msgstr doesn't parse/);
  });

  it("checks each plural form: singular forms may leave the count out, the last may not", () => {
    expect(
      problems(`msgid "# file checked"
msgid_plural "# files checked"
msgstr[0] "En fil kontrollerad"
msgstr[1] "# filer kontrollerade"
`),
    ).toEqual([]);
    expect(
      problems(`msgid "# file checked"
msgid_plural "# files checked"
msgstr[0] "# fil kontrollerad"
msgstr[1] "Filer kontrollerade"
`),
    ).toEqual(["msgstr[1] drops #"]);
  });
});

describe("Rust placeholders (core)", () => {
  it("reads {name} and skips escaped braces", () => {
    expect([...rustPlaceholders("{n} of {game}, {{literal}} {n} {not a name}")]).toEqual([
      "n",
      "game",
    ]);
  });

  it("fails a dropped or renamed placeholder, and checks plural forms", () => {
    expect(problems(`msgid "Installed {game}"\nmsgstr "Installerade {spel}"\n`, "rust")).toEqual([
      "msgstr drops {game}",
      "msgstr has {spel}, which the English doesn't",
    ]);
    expect(
      problems(
        `msgid "One game in {folder}"
msgid_plural "{n} games in {folder}"
msgstr[0] "Ett spel"
msgstr[1] "{n} spel i {folder}"
`,
        "rust",
      ),
    ).toEqual(["msgstr[0] drops {folder}"]);
    expect(
      problems(
        `msgid "One game"
msgid_plural "{n} games"
msgstr[0] "Ett spel"
msgstr[1] "Flera spel"
`,
        "rust",
      ),
    ).toEqual(["msgstr[1] drops {n}"]);
  });

  it("doesn't read ICU tags in the core's messages", () => {
    expect(problems(`msgid "Path <0>"\nmsgstr "Sökväg"\n`, "rust")).toEqual([]);
  });
});

it("names the file, the line and the entry", () => {
  const text = `${header}\nmsgctxt "tray"\nmsgid "Quit {app}"\nmsgstr "Avsluta"\n`;
  expect(catalogProblems("locales/sv/core.po", text, "rust")).toEqual([
    'locales/sv/core.po:7: msgctxt "tray" msgid "Quit {app}": msgstr drops {app}',
  ]);
});

it("names the entries a stale template gains and loses", () => {
  const before = `${header}\nmsgid "Old"\nmsgstr ""\n\nmsgid "Kept"\nmsgstr ""\n`;
  const after = `${header}\nmsgid "Kept"\nmsgstr ""\n\nmsgid "New"\nmsgstr ""\n`;
  expect(templateChanges(before, after)).toEqual(['new: msgid "New"', 'gone: msgid "Old"']);
  expect(templateChanges(before, before.replace('msgid "Old"', '#: a.ts\nmsgid "Old"'))).toEqual([
    "comments or references changed",
  ]);
});

it("counts each shipped language's completion per domain", () => {
  const sv = completion().sv;
  expect(sv && Object.keys(sv).sort()).toEqual(["app", "core", "ui"]);
  for (const count of Object.values(sv ?? {})) {
    expect(count.translated).toBeLessThanOrEqual(count.total);
  }
});
