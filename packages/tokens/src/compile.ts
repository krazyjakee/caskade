/** The design token compiler: W3C Design Tokens
 * (DTCG) JSON in three tiers → `--cs-*` CSS custom properties and a manifest
 * of the public token names. Pure: callers pass parsed JSON. */

export type Tier = "primitive" | "semantic" | "component";
export const tiers: readonly Tier[] = ["primitive", "semantic", "component"];

/** DTCG types, plus Caskade's `keyword` for CSS keywords and ratios. */
export const tokenTypes = [
  "color",
  "dimension",
  "duration",
  "number",
  "cubicBezier",
  "fontFamily",
  "keyword",
] as const;
export type TokenType = (typeof tokenTypes)[number];

export interface Token {
  /** Dotted path, `$root` omitted: `color.accent`, `color.accent.strong`. */
  path: string;
  /** `--cs-color-accent-strong`. */
  name: string;
  tier: Tier;
  type: TokenType;
  description?: string;
  /** As written: a literal or `{alias.path}`. */
  value: unknown;
  /** Where it was declared, for errors. */
  file: string;
}

/** A partial token tree applied under a condition (a selector or media query). */
export interface TokenSet {
  name: string;
  /** `:root` when not given. */
  selector?: string;
  media?: string;
  file: string;
  tree: unknown;
}

export class TokenError extends Error {
  readonly file: string;
  readonly path: string;
  constructor(file: string, path: string, message: string) {
    super(`${file}: ${path || "(root)"}: ${message}`);
    this.file = file;
    this.path = path;
  }
}

export interface ManifestEntry {
  name: string;
  path: string;
  tier: Tier;
  type: TokenType;
  description?: string;
  /** The CSS value with aliases resolved to literals. */
  resolved: string;
}

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const segment = /^(?:\$root|[a-z][a-z0-9]*(?:-[a-z0-9]+)*)$/;
const ALIAS = /^\{([a-z0-9$.-]+)\}$/;

export const cssName = (path: string) => `--cs-${path.replaceAll(".", "-")}`;

/** The tokens of one tier file: `$type` is inherited from groups. */
export function collect(tree: unknown, tier: Tier, file: string): Token[] {
  const tokens: Token[] = [];
  const walk = (node: unknown, path: string[], inherited?: string) => {
    if (!isObject(node)) throw new TokenError(file, path.join("."), "expected a group or token");
    const type = typeof node.$type === "string" ? node.$type : inherited;
    if ("$value" in node) {
      const dotted = path.filter((part) => part !== "$root").join(".");
      if (!type) throw new TokenError(file, dotted, "token has no $type");
      if (!(tokenTypes as readonly string[]).includes(type))
        throw new TokenError(file, dotted, `unknown $type "${type}"`);
      const extra = Object.keys(node).filter(
        (key) => !["$value", "$type", "$description", "$extensions"].includes(key),
      );
      if (extra.length)
        throw new TokenError(file, dotted, `a token can't have children (${extra[0]})`);
      tokens.push({
        path: dotted,
        name: cssName(dotted),
        tier,
        type: type as TokenType,
        description: typeof node.$description === "string" ? node.$description : undefined,
        value: node.$value,
        file,
      });
      return;
    }
    for (const [key, child] of Object.entries(node)) {
      if (key.startsWith("$") && key !== "$root") continue;
      if (!segment.test(key)) throw new TokenError(file, [...path, key].join("."), "invalid name");
      walk(child, [...path, key], type);
    }
  };
  walk(tree, []);
  return tokens;
}

/** The overrides of a set: paths and values, typed later from the base. */
function overrides(set: TokenSet): { path: string; value: unknown }[] {
  const found: { path: string; value: unknown }[] = [];
  const walk = (node: unknown, path: string[]) => {
    if (!isObject(node))
      throw new TokenError(set.file, path.join("."), "expected a group or token");
    if ("$value" in node) {
      found.push({ path: path.filter((p) => p !== "$root").join("."), value: node.$value });
      return;
    }
    for (const [key, child] of Object.entries(node))
      if (!key.startsWith("$") || key === "$root") walk(child, [...path, key]);
  };
  walk(set.tree, []);
  return found;
}

const hex = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const units = /^-?(?:\d+|\d*\.\d+)(px|rem|em|%|vh|vw)$/;
const keyword = /^[a-zA-Z0-9 ./%-]+$/;
const family = /^[^"'\\;{}<>]+$/;

/** A literal value as CSS, or an error naming the problem. */
function literal(type: TokenType, value: unknown): string {
  const fail = (what: string): never => {
    throw new Error(`expected ${what}, got ${JSON.stringify(value)}`);
  };
  const measured = (allowed: string[]) => {
    if (typeof value === "string") {
      const unit = units.exec(value)?.[1] ?? /^-?(?:\d+|\d*\.\d+)(ms|s)$/.exec(value)?.[1];
      if (unit && allowed.includes(unit)) return value;
      if (value === "0") return value;
    }
    if (isObject(value) && typeof value.value === "number" && typeof value.unit === "string") {
      if (allowed.includes(value.unit)) return `${value.value}${value.unit}`;
    }
    return fail(`a ${type} in ${allowed.join(", ")}`);
  };
  switch (type) {
    case "color":
      if (typeof value === "string" && hex.test(value)) return value.toLowerCase();
      if (isObject(value) && typeof value.hex === "string" && hex.test(value.hex))
        return value.hex.toLowerCase();
      return fail("a hex colour");
    case "dimension":
      return measured(["px", "rem", "em", "%", "vh", "vw"]);
    case "duration":
      return measured(["ms", "s"]);
    case "number":
      if (typeof value === "number" && Number.isFinite(value)) return String(value);
      return fail("a number");
    case "cubicBezier":
      if (
        Array.isArray(value) &&
        value.length === 4 &&
        value.every((n) => typeof n === "number" && Number.isFinite(n)) &&
        [value[0], value[2]].every((n) => n >= 0 && n <= 1)
      )
        return `cubic-bezier(${value.join(", ")})`;
      return fail("four numbers with x in 0..1");
    case "fontFamily": {
      const names = typeof value === "string" ? [value] : value;
      if (!Array.isArray(names) || names.length === 0) return fail("a font family or list");
      return names
        .map((name) => {
          if (typeof name !== "string" || !family.test(name)) return fail("font family names");
          // Generic families and single words stay bare; the rest is quoted.
          return /^[a-z-]+$/.test(name) ? name : `"${name}"`;
        })
        .join(", ");
    }
    case "keyword":
      if (typeof value === "string" && keyword.test(value)) return value;
      return fail("a CSS keyword");
  }
}

export interface Compiled {
  tokens: Token[];
  manifest: ManifestEntry[];
  css: string;
}

const MAX_ALIAS_DEPTH = 16;

/** Compiles the tiers and sets. Aliases stay live as `var()` so a theme that
 * changes a primitive restyles every semantic token built on it. */
export function compile(
  files: { tier: Tier; file: string; tree: unknown }[],
  sets: TokenSet[] = [],
): Compiled {
  const tokens = files.flatMap((input) => collect(input.tree, input.tier, input.file));
  const byPath = new Map<string, Token>();
  for (const token of tokens) {
    const other = byPath.get(token.path);
    if (other) throw new TokenError(token.file, token.path, `already declared in ${other.file}`);
    byPath.set(token.path, token);
  }
  const resolve = (token: Token, value: unknown, file: string, seen: string[] = []): string => {
    const alias = typeof value === "string" ? ALIAS.exec(value)?.[1] : undefined;
    if (alias !== undefined) {
      const target = byPath.get(alias.replace(/\.\$root$/, ""));
      if (!target) throw new TokenError(file, token.path, `unknown alias {${alias}}`);
      if (seen.includes(target.path) || seen.length > MAX_ALIAS_DEPTH)
        throw new TokenError(
          file,
          token.path,
          `alias cycle: ${[...seen, target.path].join(" → ")}`,
        );
      if (target.type !== token.type)
        throw new TokenError(
          file,
          token.path,
          `alias {${alias}} is a ${target.type}, not a ${token.type}`,
        );
      return resolve(target, target.value, target.file, [...seen, target.path]);
    }
    try {
      return literal(token.type, value);
    } catch (error) {
      throw new TokenError(file, token.path, (error as Error).message);
    }
  };
  const css = (token: Token, value: unknown, file: string) => {
    resolve(token, value, file, [token.path]);
    const alias = typeof value === "string" ? ALIAS.exec(value)?.[1] : undefined;
    return alias === undefined
      ? literal(token.type, value)
      : `var(${cssName(alias.replace(/\.\$root$/, ""))})`;
  };
  const manifest: ManifestEntry[] = tokens.map((token) => ({
    name: token.name,
    path: token.path,
    tier: token.tier,
    type: token.type,
    ...(token.description && { description: token.description }),
    resolved: resolve(token, token.value, token.file, [token.path]),
  }));
  const lines = ["/* Generated by @caskade/tokens from tokens/*.json. Do not edit. */", ":root {"];
  for (const tier of tiers) {
    const inTier = tokens.filter((token) => token.tier === tier);
    if (!inTier.length) continue;
    lines.push(`  /* ${tier} */`);
    for (const token of inTier)
      lines.push(`  ${token.name}: ${css(token, token.value, token.file)};`);
  }
  lines.push("}");
  for (const set of sets) {
    const body = overrides(set).map(({ path, value }) => {
      const token = byPath.get(path);
      if (!token) throw new TokenError(set.file, path, "a set can only override declared tokens");
      return `  ${token.name}: ${css(token, value, set.file)};`;
    });
    const block = [`${set.selector ?? ":root"} {`, ...body, "}"];
    lines.push("", `/* ${set.name} */`);
    if (set.media) lines.push(`@media ${set.media} {`, ...block.map((line) => `  ${line}`), "}");
    else lines.push(...block);
  }
  return { tokens, manifest, css: `${lines.join("\n")}\n` };
}

/** The set's condition from `$extensions["dev.caskade.condition"]`. */
export function setFromJson(name: string, file: string, tree: unknown): TokenSet {
  const condition =
    isObject(tree) && isObject(tree.$extensions)
      ? tree.$extensions["dev.caskade.condition"]
      : undefined;
  const selector =
    isObject(condition) && typeof condition.selector === "string" ? condition.selector : undefined;
  const media =
    isObject(condition) && typeof condition.media === "string" ? condition.media : undefined;
  if (!selector && !media)
    throw new TokenError(
      file,
      "",
      'a set needs $extensions["dev.caskade.condition"] with a selector or media query',
    );
  if (
    media !== undefined &&
    !/^\([a-z-]+: ?[a-z0-9-]+\)(?: and \([a-z-]+: ?[a-z0-9-]+\))*$/.test(media)
  )
    throw new TokenError(file, "", `unsupported media query ${JSON.stringify(media)}`);
  if (
    selector !== undefined &&
    !/^(?::root|\[data-[a-z-]+(?:="[a-z0-9-]+")?\]|:not\(\[data-[a-z-]+(?:="[a-z0-9-]+")?\]\))+$/.test(
      selector,
    )
  )
    throw new TokenError(file, "", `unsupported selector ${JSON.stringify(selector)}`);
  return { name, file, selector, media, tree };
}
