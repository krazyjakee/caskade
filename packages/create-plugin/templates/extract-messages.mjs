// Extracts a Caskade plugin's messages into locales/<plugin id>.pot and
// merges them into each locales/<lang>.po (#219). No dependencies: Node's
// own modules, no gettext tools.
//
//   node scripts/extract-messages.mjs            # write the template, update catalogs
//   node scripts/extract-messages.mjs --add sv   # start a Swedish catalog
//   node scripts/extract-messages.mjs --check    # fail when anything is out of date
//
// The English text is the message id. It reads:
// - plugin.json: the name, the description, slot, view and command titles,
//   and the settings schema's titles and descriptions;
// - src/: `t("…")`, `t("…", values, { context: "…" })`,
//   `plural(count, "One …", "{n} …")`, `<Trans>…</Trans>` and
//   `<Trans message="…" context="…" />`;
// - backend/src/: the Rust SDK's `t("…")`, `tf("…", …)`, `tc("context",
//   "…")` and `plural("One …", "{count} …", n)` (plugin API 0.2), and
//   `tr("…")` or `host::message("…", …)` for a backend without the SDK.
// Only string literals count. Translations stay when a catalog is merged;
// new messages come in empty and removed ones go.
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const args = process.argv.slice(2);
const check = args.includes("--check");
const add = args.includes("--add") ? args[args.indexOf("--add") + 1] : undefined;
const root = resolve(
  args.find((arg, index) => !arg.startsWith("--") && args[index - 1] !== "--add") ?? ".",
);
const manifest = JSON.parse(readFileSync(join(root, "plugin.json"), "utf8"));
const locales = join(root, "locales");

/** Messages by key, in the order they were found. */
const found = new Map();
function message(id, { context, plural, where }) {
  if (!id) return;
  const key = `${context ?? ""}\u0004${id}`;
  const known = found.get(key) ?? { context, id, plural, references: new Set() };
  known.plural ??= plural;
  known.references.add(where);
  found.set(key, known);
}

// plugin.json
message(manifest.name, { where: "plugin.json" });
message(manifest.description, { where: "plugin.json" });
const contributes = manifest.contributes ?? {};
for (const item of [
  ...(contributes.slots ?? []),
  ...(contributes.views ?? []),
  ...(contributes.commands ?? []),
])
  message(item.title, { where: "plugin.json" });
(function schema(value) {
  if (Array.isArray(value)) value.forEach(schema);
  else if (value && typeof value === "object")
    for (const [key, child] of Object.entries(value)) {
      if ((key === "title" || key === "description") && typeof child === "string")
        message(child, { where: "plugin.json" });
      else if (!["default", "const", "enum", "examples"].includes(key)) schema(child);
    }
})(contributes.settings);

// Sources.
function files(folder, extensions) {
  if (!existsSync(folder)) return [];
  return readdirSync(folder)
    .sort()
    .flatMap((name) => {
      const path = join(folder, name);
      if (statSync(path).isDirectory())
        return name === "node_modules" || name === "target" ? [] : files(path, extensions);
      return extensions.some((extension) => name.endsWith(extension)) ? [path] : [];
    });
}
const literal = String.raw`"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)'`;
const unquote = (double, single) =>
  JSON.parse(`"${double ?? single.replaceAll("\\'", "'").replaceAll('"', '\\"')}"`);
const contextOf = (rest) => {
  const match = new RegExp(String.raw`^[^)]*?context:\s*(?:${literal})`).exec(rest);
  return match ? unquote(match[1], match[2]) : undefined;
};
for (const path of files(join(root, "src"), [".ts", ".tsx", ".js", ".jsx", ".mts"])) {
  const text = readFileSync(path, "utf8");
  const where = relative(root, path);
  for (const match of text.matchAll(new RegExp(String.raw`\bt\(\s*(?:${literal})`, "g")))
    message(unquote(match[1], match[2]), {
      where,
      context: contextOf(text.slice(match.index + match[0].length)),
    });
  for (const match of text.matchAll(
    new RegExp(String.raw`\bplural\(\s*[^,]+,\s*(?:${literal})\s*,\s*(?:${literal})`, "g"),
  ))
    message(unquote(match[1], match[2]), {
      where,
      plural: unquote(match[3], match[4]),
      context: contextOf(text.slice(match.index + match[0].length)),
    });
  for (const match of text.matchAll(/<Trans\b/g)) {
    // The tag up to its end: `message="…"` and `context="…"`, or `>text`.
    const rest = text.slice(match.index + match[0].length);
    const end = rest.search(/\/>|<\/Trans>/);
    const tag = end < 0 ? "" : rest.slice(0, end);
    const attribute = (name) => new RegExp(String.raw`\b${name}="([^"]*)"`).exec(tag)?.[1];
    const children = /^[^>]*>([^<{]*)$/.exec(tag)?.[1]?.trim();
    message(attribute("message") ?? children, { where, context: attribute("context") });
  }
}
const rust = String.raw`"((?:[^"\\]|\\.)*)"`;
for (const path of files(join(root, "backend/src"), [".rs"])) {
  const text = readFileSync(path, "utf8");
  const where = relative(root, path);
  for (const match of text.matchAll(new RegExp(String.raw`\b(?:t|tf|tr|message)\(\s*${rust}`, "g")))
    message(JSON.parse(`"${match[1]}"`), { where });
  for (const match of text.matchAll(new RegExp(String.raw`\btc\(\s*${rust}\s*,\s*${rust}`, "g")))
    message(JSON.parse(`"${match[2]}"`), { where, context: JSON.parse(`"${match[1]}"`) });
  for (const match of text.matchAll(
    new RegExp(String.raw`\bplural\(\s*${rust}\s*,\s*${rust}`, "g"),
  ))
    message(JSON.parse(`"${match[1]}"`), { where, plural: JSON.parse(`"${match[2]}"`) });
}

// PO text.
const quote = (text) =>
  `"${text.replaceAll("\\", "\\\\").replaceAll('"', '\\"').replaceAll("\n", "\\n").replaceAll("\t", "\\t")}"`;
function entry({ context, id, plural, references, comments = [], flags = [], translations }) {
  const lines = [...comments];
  if (references?.size) lines.push(`#: ${[...references].join(" ")}`);
  if (flags.length) lines.push(`#, ${flags.join(", ")}`);
  if (context !== undefined) lines.push(`msgctxt ${quote(context)}`);
  lines.push(`msgid ${quote(id)}`);
  if (plural !== undefined) {
    lines.push(`msgid_plural ${quote(plural)}`);
    const forms = translations?.length ? translations : ["", ""];
    for (const [index, form] of forms.entries()) lines.push(`msgstr[${index}] ${quote(form)}`);
  } else lines.push(`msgstr ${quote(translations?.[0] ?? "")}`);
  return lines.join("\n");
}
/** Reads PO text: the header and each message; obsolete `#~` entries go. */
function parse(text) {
  const messages = [];
  let current = null;
  let field = null;
  const fresh = () => {
    current = { comments: [], flags: [], translations: [] };
    messages.push(current);
  };
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#~")) {
      current = null;
      continue;
    }
    if (line.startsWith("#")) {
      if (!current || current.id !== undefined) fresh();
      if (line.startsWith("#,"))
        current.flags.push(
          ...line
            .slice(2)
            .split(",")
            .map((f) => f.trim()),
        );
      // References are written again from the template.
      else if (!line.startsWith("#:")) current.comments.push(line);
      continue;
    }
    const keyword = /^(msgctxt|msgid_plural|msgid|msgstr(?:\[(\d+)\])?)\s+(".*")$/.exec(line);
    if (keyword) {
      const name = keyword[1];
      if ((name === "msgctxt" || name === "msgid") && (!current || current.id !== undefined))
        fresh();
      field = name.startsWith("msgstr") ? Number(keyword[2] ?? 0) : name;
    } else if (!line.startsWith('"') || !current)
      throw new Error(`can't read ${JSON.stringify(raw)}`);
    const string = JSON.parse(keyword ? keyword[3] : line);
    if (field === "msgctxt") current.context = (current.context ?? "") + string;
    else if (field === "msgid") current.id = (current.id ?? "") + string;
    else if (field === "msgid_plural") current.plural = (current.plural ?? "") + string;
    else current.translations[field] = (current.translations[field] ?? "") + string;
  }
  const header = messages.find((item) => item.id === "" && item.context === undefined);
  return { header, messages: messages.filter((item) => item.id) };
}

const pot = `${[
  entry({
    id: "",
    comments: [`# Messages of the Caskade plugin ${manifest.id}.`],
    translations: [
      `Project-Id-Version: ${manifest.id}\nMIME-Version: 1.0\n` +
        "Content-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n",
    ],
  }),
  ...[...found.values()].map((item) => entry(item)),
].join("\n\n")}\n`;

/** A catalog with the template's messages and its own translations. */
function merge(text) {
  const { header, messages } = parse(text);
  const translated = new Map(
    messages.map((item) => [`${item.context ?? ""}\u0004${item.id}`, item]),
  );
  return `${[
    entry({ ...header, id: "" }),
    ...[...found.entries()].map(([key, item]) => {
      const old = translated.get(key);
      const changedPlural = old && (old.plural ?? null) !== (item.plural ?? null);
      return entry({
        ...item,
        comments: old?.comments ?? [],
        // A message whose English plural changed needs a look.
        flags: changedPlural ? [...new Set([...old.flags, "fuzzy"])] : (old?.flags ?? []),
        translations: old?.translations,
      });
    }),
  ].join("\n\n")}\n`;
}

const writes = new Map([[join(locales, `${manifest.id}.pot`), pot]]);
if (add) {
  const file = join(locales, `${add}.po`);
  if (existsSync(file)) throw new Error(`${relative(root, file)} already exists`);
  writes.set(
    file,
    merge(
      `${entry({
        id: "",
        translations: [
          `Project-Id-Version: ${manifest.id}\nLanguage: ${add}\nMIME-Version: 1.0\n` +
            "Content-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n" +
            "Plural-Forms: nplurals=2; plural=(n != 1);\n",
        ],
        comments: ["# Check Plural-Forms for this language."],
      })}\n`,
    ),
  );
}
if (existsSync(locales))
  for (const name of readdirSync(locales)
    .filter((name) => name.endsWith(".po"))
    .sort()) {
    const file = join(locales, name);
    writes.set(file, merge(readFileSync(file, "utf8")));
  }

let stale = 0;
for (const [file, text] of writes) {
  const before = existsSync(file) ? readFileSync(file, "utf8") : undefined;
  if (before === text) continue;
  stale++;
  if (check) console.error(`${relative(root, file)} is out of date`);
  else {
    mkdirSync(locales, { recursive: true });
    writeFileSync(file, text);
    console.log(`wrote ${relative(root, file)}`);
  }
}
console.log(`${found.size} messages`);
if (check && stale > 0) process.exit(1);
