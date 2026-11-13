/** The translator checks behind `pnpm i18n:check` (`just i18n-check`, CI):
 * every PO file passes `msgfmt --check`, and every translation keeps its
 * English's placeholders and tags. See `docs/translating.md`. */
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { devNull } from "node:os";
import { join } from "node:path";
import { type CompiledMessage, compileMessageOrThrow } from "@lingui/message-utils/compileMessage";
import { entryKey, isMessage, type PoEntry, parsePo } from "./vite.ts";

/** How a domain's messages mark what the code fills in: ICU (`{name}`,
 * `{0}`, Lingui's `<0>…</0>` tags) in the webview, the Rust runtime's
 * `{name}` (`{{` and `}}` are literal braces) in `core`. */
type Syntax = "icu" | "rust";

export const syntaxOf = (domain: string): Syntax => (domain === "core" ? "rust" : "icu");

/** What a text must keep: placeholder names (`#` for a gettext plural's
 * count in ICU) and tags, in any order. */
interface Markers {
  placeholders: Set<string>;
  tags: string[];
}

/** `{name}` placeholders, as `caskade_i18n::format::placeholders` reads them. */
export function rustPlaceholders(text: string): Set<string> {
  const names = new Set<string>();
  for (const match of text.matchAll(/\{\{|\}\}|\{([A-Za-z0-9_]+)\}/g)) {
    if (match[1]) names.add(match[1]);
  }
  return names;
}

function icuArguments(message: CompiledMessage, names: Set<string>, content: string[]): void {
  for (const token of message) {
    if (typeof token === "string") {
      content.push(token);
      continue;
    }
    const [name, , format] = token;
    names.add(name);
    if (format && typeof format === "object") {
      for (const [key, choice] of Object.entries(format)) {
        if (key !== "offset" && Array.isArray(choice)) icuArguments(choice, names, content);
      }
    }
  }
}

/** The placeholders and tags of `text`. Throws when ICU text doesn't parse. */
function markers(text: string, syntax: Syntax, plural: boolean): Markers {
  if (syntax === "rust") return { placeholders: rustPlaceholders(text), tags: [] };
  const placeholders = new Set<string>();
  const content: string[] = [];
  icuArguments(compileMessageOrThrow(text), placeholders, content);
  // A gettext plural's forms become one ICU plural, where `#` is the count.
  if (plural && content.some((part) => part.includes("#"))) placeholders.add("#");
  const tags = [...text.matchAll(/<\/?\d+\s*\/?>/g)].map((tag) => tag[0].replace(/\s/g, "")).sort();
  return { placeholders, tags };
}

const show = (name: string) => (name === "#" ? "#" : `{${name}}`);
const list = (names: Iterable<string>) => [...names].map(show).join(", ");
const sameTags = (a: string[], b: string[]) => a.join(" ") === b.join(" ");
const tagList = (tags: string[]) => (tags.length ? tags.join(" ") : "none");

/** What's wrong with an entry's translations, if anything. Each non-empty
 * form keeps the English's placeholders (none dropped, none added or
 * renamed) and its tags. A plural's forms are checked against `msgid` and
 * `msgid_plural`: each may use the placeholders of either and must keep
 * those both have, except the count, and the last (the language's most
 * general form) must keep every one `msgid_plural` has, the count included.
 * Fuzzy, obsolete and untranslated entries show English, so pass. */
export function entryProblems(entry: PoEntry, syntax: Syntax): string[] {
  if (!isMessage(entry) || entry.flags.includes("fuzzy")) return [];
  const plural = entry.plural !== undefined;
  let one: Markers;
  let other: Markers;
  try {
    one = markers(entry.id, syntax, plural);
    other = plural ? markers(entry.plural ?? "", syntax, plural) : one;
  } catch {
    // English that doesn't parse is the code's to fix, not the translator's.
    return [];
  }
  const allowed = new Set([...one.placeholders, ...other.placeholders]);
  // The count (`#`, the core's `{n}`) may be left out of a singular form.
  const count = syntax === "rust" ? "n" : "#";
  const shared = new Set(
    [...one.placeholders].filter((name) => name !== count && other.placeholders.has(name)),
  );
  const problems: string[] = [];
  entry.translations.forEach((text, index) => {
    if (!text) return;
    const form = plural ? `msgstr[${index}]` : "msgstr";
    let found: Markers;
    try {
      found = markers(text, syntax, plural);
    } catch (error) {
      problems.push(`${form} doesn't parse: ${(error as Error).message.split("\n")[0]}`);
      return;
    }
    const last = index === entry.translations.length - 1;
    const required = plural && last ? other.placeholders : plural ? shared : one.placeholders;
    const dropped = [...required].filter((name) => !found.placeholders.has(name));
    const added = [...found.placeholders].filter((name) => !allowed.has(name));
    if (dropped.length) problems.push(`${form} drops ${list(dropped)}`);
    if (added.length) problems.push(`${form} has ${list(added)}, which the English doesn't`);
    if (!sameTags(found.tags, one.tags) && !sameTags(found.tags, other.tags)) {
      problems.push(
        `${form} has the tags ${tagList(found.tags)}, the English ${tagList(other.tags)}`,
      );
    }
  });
  return problems;
}

/** `msgctxt "…" msgid "…"`, shortened, to name an entry in a message. */
function describeEntry(entry: Pick<PoEntry, "context" | "id">): string {
  const short = (text: string) =>
    JSON.stringify(text.length > 60 ? `${text.slice(0, 57)}...` : text);
  return `${entry.context !== undefined ? `msgctxt ${short(entry.context)} ` : ""}msgid ${short(entry.id)}`;
}

/** The placeholder and tag problems in a PO file's text, one line each:
 * `<file>:<line>: <entry>: <problem>`. */
export function catalogProblems(file: string, text: string, syntax: Syntax): string[] {
  return parsePo(text).flatMap((entry) =>
    entryProblems(entry, syntax).map(
      (problem) => `${file}:${entry.line}: ${describeEntry(entry)}: ${problem}`,
    ),
  );
}

/** What changed between a template on disk and the one extraction writes,
 * by entry: `new` and `gone` messages, else that only comments or
 * references changed. */
export function templateChanges(before: string, after: string): string[] {
  const keys = (text: string) =>
    new Map(
      parsePo(text)
        .filter(isMessage)
        .map((entry) => [entryKey(entry), entry]),
    );
  const old = keys(before);
  const current = keys(after);
  const changes = [
    ...[...current]
      .filter(([key]) => !old.has(key))
      .map(([, entry]) => `new: ${describeEntry(entry)}`),
    ...[...old]
      .filter(([key]) => !current.has(key))
      .map(([, entry]) => `gone: ${describeEntry(entry)}`),
  ];
  return changes.length ? changes : ["comments or references changed"];
}

/** `msgfmt --check` on a PO file: what it reports (`<file>:<line>: …`),
 * nothing when the file is valid. Throws when GNU gettext isn't installed. */
export function msgfmtProblems(file: string, cwd: string): string[] {
  try {
    execFileSync("msgfmt", ["--check", `--output-file=${devNull}`, file], {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "ignore", "pipe"],
    });
    return [];
  } catch (error) {
    const { code, stderr } = error as { code?: string; stderr?: string };
    if (code === "ENOENT") {
      throw new Error("msgfmt not found: install GNU gettext (`gettext` on Linux and Homebrew)");
    }
    // Its warnings (header fields a hand-started file lacks) don't fail it.
    const lines = (stderr || String(error))
      .split("\n")
      .filter((line) => line.trim() && !line.includes("warning:"));
    return lines.length ? lines : [`${file}: msgfmt --check failed`];
  }
}

/** Plugins' and themes' catalogs (`<folder>/locales/<lang>.po`, #219),
 * relative to `root`. */
export function packageCatalogs(root: string): string[] {
  const found: string[] = [];
  const walk = (dir: string, depth: number) => {
    if (depth > 4 || !existsSync(join(root, dir))) return;
    for (const entry of readdirSync(join(root, dir), { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== "node_modules" && entry.name !== "target") walk(path, depth + 1);
      } else if (entry.name.endsWith(".po") && dir.endsWith("locales")) {
        found.push(path);
      }
    }
  };
  walk("plugins", 0);
  walk("themes", 0);
  return found.sort();
}
