/** A small TypeScript port of `caskade-theme`'s compiler (crates/caskade-theme
 * `compile`), so the playground can apply theme folders without the core:
 * the `extends` chain's tokens on `:root` (a later theme's value wins), then
 * each theme's CSS, base first, all inside `@layer cs.theme`. It checks
 * tokens against the contract; `cargo run -p caskade-theme --features cli --
 * lint <dir>` stays the full linter. */

export interface ThemeManifest {
  id: string;
  name: string;
  version: string;
  extends?: string;
  /** Colour schemes the theme provides, its default first. */
  modes?: ("dark" | "light")[];
  description?: string;
  author?: string;
}

export interface ThemeFolder {
  /** The folder name under `themes/`. */
  dir: string;
  manifest: ThemeManifest;
  /** Parsed `tokens.json`, when the theme has one. */
  tokens?: unknown;
  css: string;
  /** `assets/…` path → the URL it is served at. */
  assets: Readonly<Record<string, string>>;
}

/** A token of the contract (`@caskade/tokens/manifest.json`). */
interface ContractToken {
  name: string;
  path: string;
  type: string;
}

export class ThemeError extends Error {
  readonly problems: readonly string[];
  constructor(problems: readonly string[]) {
    super(problems.join("\n"));
    this.problems = problems;
  }
}

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const hex = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const keyword = /^[a-zA-Z0-9 ./%-]+$/;
const family = /^[^"'\\;{}<>]+$/;

function measured(value: unknown, units: readonly string[]): string | undefined {
  if (typeof value === "string") {
    if (value === "0") return value;
    const match = /^-?(?:\d+|\d*\.\d+)([a-z%]+)$/.exec(value);
    return match && units.includes(match[1] ?? "") ? value : undefined;
  }
  if (isObject(value) && typeof value.value === "number" && typeof value.unit === "string")
    return units.includes(value.unit) ? `${value.value}${value.unit}` : undefined;
  return undefined;
}

/** A literal as CSS, like `caskade-theme`'s `literal`. */
function literal(type: string, value: unknown): string | undefined {
  switch (type) {
    case "color": {
      const text = isObject(value) ? value.hex : value;
      return typeof text === "string" && hex.test(text) ? text.toLowerCase() : undefined;
    }
    case "dimension":
      return measured(value, ["px", "rem", "em", "%", "vh", "vw"]);
    case "duration":
      return measured(value, ["ms", "s"]);
    case "number":
      return typeof value === "number" && Number.isFinite(value) ? String(value) : undefined;
    case "cubicBezier":
      return Array.isArray(value) &&
        value.length === 4 &&
        value.every((n) => typeof n === "number" && Number.isFinite(n)) &&
        [value[0], value[2]].every((x) => x >= 0 && x <= 1)
        ? `cubic-bezier(${value.join(", ")})`
        : undefined;
    case "fontFamily": {
      const names = typeof value === "string" ? [value] : value;
      if (!Array.isArray(names) || names.length === 0) return undefined;
      if (!names.every((name) => typeof name === "string" && family.test(name))) return undefined;
      return names.map((name) => (/^[a-z-]+$/.test(name) ? name : `"${name}"`)).join(", ");
    }
    case "keyword":
      return typeof value === "string" && keyword.test(value) ? value : undefined;
    default:
      return undefined;
  }
}

/** `tokens.json` → `--cs-*` declarations, in file order. */
export function compileTokens(
  tree: unknown,
  contract: readonly ContractToken[],
): [name: string, value: string][] {
  const byPath = new Map(contract.map((token) => [token.path, token]));
  const out: [string, string][] = [];
  const problems: string[] = [];
  const walk = (node: unknown, path: string[], inherited?: string) => {
    const dotted = path.filter((part) => part !== "$root").join(".");
    if (!isObject(node)) {
      problems.push(`tokens.json: ${dotted || "(root)"}: expected a group or token`);
      return;
    }
    const declared = typeof node.$type === "string" ? node.$type : inherited;
    if ("$value" in node) {
      const token = byPath.get(dotted);
      if (!token) {
        problems.push(`tokens.json: ${dotted}: is not a token of the theming contract`);
        return;
      }
      if (declared && declared !== token.type) {
        problems.push(`tokens.json: ${dotted}: is a ${token.type}, not a ${declared}`);
        return;
      }
      const value = node.$value;
      const alias = typeof value === "string" ? /^\{(.+)\}$/.exec(value)?.[1] : undefined;
      if (alias !== undefined) {
        const target = byPath.get(alias.replace(/\.\$root$/, ""));
        if (!target) problems.push(`tokens.json: ${dotted}: unknown alias {${alias}}`);
        else if (target.type !== token.type)
          problems.push(`tokens.json: ${dotted}: alias {${alias}} is a ${target.type}`);
        else out.push([token.name, `var(${target.name})`]);
        return;
      }
      const css = literal(token.type, value);
      if (css === undefined)
        problems.push(`tokens.json: ${dotted}: ${JSON.stringify(value)} is not a ${token.type}`);
      else out.push([token.name, css]);
      return;
    }
    for (const [key, child] of Object.entries(node))
      if (!key.startsWith("$") || key === "$root") walk(child, [...path, key], declared);
  };
  walk(tree, []);
  if (problems.length) throw new ThemeError(problems);
  return out;
}

/** Rewrites `url(assets/…)` to the asset's served URL, as the core does for
 * `caskade-theme://`. Other URLs are left alone (the linter refuses them). */
export function rewriteAssetUrls(css: string, assets: Readonly<Record<string, string>>): string {
  return css.replace(/url\(\s*(["']?)([^"')]+)\1\s*\)/g, (match, _quote, url: string) => {
    const trimmed = url.trim();
    const path = trimmed.split(/[?#]/)[0] ?? "";
    const fragment = trimmed.includes("#") ? trimmed.slice(trimmed.indexOf("#")) : "";
    const served = assets[path.replace(/^\.\//, "")];
    return served ? `url("${served}${fragment}")` : match;
  });
}

/** The `extends` chain of `id`, base first. */
export function themeChain(id: string, themes: readonly ThemeFolder[]): ThemeFolder[] {
  const chain: ThemeFolder[] = [];
  let next: string | undefined = id;
  while (next !== undefined) {
    const current: string = next;
    if (chain.some((theme) => theme.manifest.id === current))
      throw new ThemeError([`extends cycle at ${current}`]);
    const theme = themes.find((candidate) => candidate.manifest.id === current);
    if (!theme)
      throw new ThemeError([
        chain.length
          ? `${chain.at(-1)?.manifest.id} extends ${current}, which isn't in themes/`
          : `theme ${current} isn't in themes/`,
      ]);
    chain.push(theme);
    next = theme.manifest.extends;
  }
  return chain.reverse();
}

/** One stylesheet for a chain from {@link themeChain}, shaped like the core's. */
export function compileTheme(chain: readonly ThemeFolder[], contract: readonly ContractToken[]) {
  const leaf = chain.at(-1);
  if (!leaf) return "";
  const tokens = new Map<string, string>();
  for (const theme of chain)
    if (theme.tokens !== undefined)
      for (const [name, value] of compileTokens(theme.tokens, contract)) tokens.set(name, value);
  const { id, version } = leaf.manifest;
  let out = `/* ${id} ${version}, compiled by the playground. */\n@layer cs.theme {\n`;
  if (tokens.size) {
    out += ":root {\n";
    for (const [name, value] of tokens) out += `  ${name}: ${value};\n`;
    out += "}\n";
  }
  for (const theme of chain) {
    if (!theme.css.trim()) continue;
    const css = rewriteAssetUrls(theme.css, theme.assets);
    out += `\n/* ${theme.manifest.id} */\n${css}${css.endsWith("\n") ? "" : "\n"}`;
  }
  return `${out}}\n`;
}
