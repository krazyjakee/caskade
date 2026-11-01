/** The user's CSS (#142): one `<style>` in `@layer cs.user`, which beats
 * every theme and component style without `!important` (TECH-PLAN §5.2).
 * Like themes, it can't load anything from other origins: a remote
 * `@import` or `url()` refuses the whole sheet, with a message. */

import { t } from "@lingui/core/macro";

/** The most custom CSS kept, in characters (`core.appearance.customCss`). */
export const USER_CSS_MAX = 32 * 1024;

/** The contract's layer order (`styles/index.css`), restated so the user
 * layer stays last even if this sheet loads first. */
export const LAYER_ORDER =
  "@layer cs.reset, cs.tokens, cs.base, cs.components, cs.plugins, cs.theme, cs.user;";

export interface CssProblem {
  /** 1-based. */
  line: number;
  message: string;
}

const LOCAL_SCHEMES = new Set([
  "data",
  "blob",
  "caskade-theme",
  "caskade-asset",
  "caskade-plugin",
  "asset",
]);

/** Whether a URL stays inside Caskade: relative, `data:`, the app's own
 * schemes, or a `localhost` host (how Windows serves them). */
export function isLocalUrl(raw: string): boolean {
  // Browsers drop tabs and newlines in URLs and read `\` as `/`.
  const url = raw.replace(/[\t\n\r]/g, "").trim();
  if (/^[\\/]{2}/.test(url)) return false;
  const scheme = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(url)?.[1]?.toLowerCase();
  if (!scheme) return true;
  if (LOCAL_SCHEMES.has(scheme)) return true;
  if (scheme !== "http" && scheme !== "https") return false;
  try {
    const host = new URL(url.replace(/\\/g, "/")).hostname;
    return host === "localhost" || host.endsWith(".localhost");
  } catch {
    return false;
  }
}

/** Functions whose string arguments are URLs. */
const URL_FUNCTIONS = new Set(["image-set", "-webkit-image-set", "src"]);

function isNameStart(c: string | undefined): c is string {
  return c !== undefined && /[A-Za-z_\u0080-\uffff]/.test(c);
}
function isName(c: string | undefined): c is string {
  return c !== undefined && /[\w\u0080-\uffff-]/.test(c);
}
function isHex(c: string | undefined): c is string {
  return c !== undefined && /[0-9a-fA-F]/.test(c);
}

/**
 * What stops `css` from being applied: remote `@import`s and `url()`s (any
 * `@import`, in fact: it can't work inside the user layer), and braces that
 * would close the layer early. Follows CSS tokenization, so comments,
 * strings and escapes (`\75 rl(`) can't hide a URL.
 */
export function checkUserCss(source: string): CssProblem[] {
  const css = source.replace(/\r\n?|\f/g, "\n").replaceAll("\0", "\uFFFD");
  const problems: CssProblem[] = [];
  let i = 0;
  let line = 1;
  let depth = 0;
  const stack: string[] = [];
  let pendingImport: { line: number; target: string | null } | null = null;

  const readEscape = (): string => {
    i++; // the backslash
    const c = css[i];
    if (c === undefined) return "\uFFFD";
    if (c === "\n") {
      line++;
      i++;
      return "";
    }
    if (isHex(c)) {
      let hex = "";
      while (hex.length < 6 && isHex(css[i])) hex += css[i++] ?? "";
      if (css[i] === " " || css[i] === "\t" || css[i] === "\n") {
        if (css[i] === "\n") line++;
        i++;
      }
      const code = Number.parseInt(hex, 16);
      return code === 0 || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)
        ? "\uFFFD"
        : String.fromCodePoint(code);
    }
    i++;
    return c;
  };
  const name = (): string => {
    let text = "";
    while (i < css.length) {
      const c = css[i];
      if (c === "\\" && css[i + 1] !== undefined && css[i + 1] !== "\n") text += readEscape();
      else if (isName(c)) {
        text += c;
        i++;
      } else break;
    }
    return text;
  };
  const string = (quote: string): string => {
    i++;
    let text = "";
    while (i < css.length) {
      const c = css[i];
      if (c === quote) {
        i++;
        break;
      }
      if (c === "\n") break; // a bad string ends at the line
      if (c === "\\") text += readEscape();
      else {
        text += c;
        i++;
      }
    }
    return text;
  };
  const skipSpace = () => {
    while (css[i] === " " || css[i] === "\t" || css[i] === "\n") {
      if (css[i] === "\n") line++;
      i++;
    }
  };
  /** After `url(`: the URL, through the closing parenthesis. */
  const url = (): string => {
    skipSpace();
    let text = "";
    const quote = css[i];
    if (quote === '"' || quote === "'") text = string(quote);
    else {
      while (i < css.length && css[i] !== ")" && !/[ \t\n]/.test(css[i] ?? "")) {
        if (css[i] === "\\") text += readEscape();
        else text += css[i++] ?? "";
      }
    }
    while (i < css.length && css[i] !== ")") {
      if (css[i] === "\n") line++;
      i++;
    }
    i++;
    return text;
  };
  const target = (value: string, at: number) => {
    if (pendingImport) pendingImport.target ??= value;
    else if (!isLocalUrl(value))
      problems.push({
        line: at,
        message: t`url(${{ url: value }}) is refused: custom CSS can't load anything from other origins.`,
      });
  };
  const endImport = () => {
    if (!pendingImport) return;
    const { line: at, target: value } = pendingImport;
    pendingImport = null;
    problems.push({
      line: at,
      message:
        value !== null && !isLocalUrl(value)
          ? t`@import of ${{ url: value }} is refused: custom CSS can't load anything from other origins.`
          : t`@import isn't supported in custom CSS: paste the rules in instead.`,
    });
  };

  while (i < css.length) {
    const c = css[i];
    const at = line;
    if (c === "\n") {
      line++;
      i++;
    } else if (c === "/" && css[i + 1] === "*") {
      const end = css.indexOf("*/", i + 2);
      const stop = end < 0 ? css.length : end + 2;
      line += (css.slice(i, stop).match(/\n/g) ?? []).length;
      if (end < 0) problems.push({ line: at, message: t`A comment is never closed.` });
      i = stop;
    } else if (c === '"' || c === "'") {
      const value = string(c);
      if (pendingImport || URL_FUNCTIONS.has(stack.at(-1) ?? "")) target(value, at);
    } else if (c === "@") {
      i++;
      if (name().toLowerCase() === "import") {
        endImport();
        pendingImport = { line: at, target: null };
      }
    } else if (
      isNameStart(c) ||
      (c === "\\" && css[i + 1] !== "\n") ||
      (c === "-" && (isNameStart(css[i + 1]) || css[i + 1] === "-" || css[i + 1] === "\\"))
    ) {
      const start = i;
      const ident = name().toLowerCase();
      if (i === start)
        i++; // a lone backslash at the end
      else if (css[i] === "(") {
        i++;
        if (ident === "url") target(url(), at);
        else stack.push(ident);
      }
    } else if (c === "(") {
      stack.push("");
      i++;
    } else if (c === ")") {
      stack.pop();
      i++;
    } else if (c === "{") {
      endImport();
      depth++;
      i++;
    } else if (c === "}") {
      depth--;
      if (depth < 0) {
        problems.push({
          line: at,
          message: t`A closing brace closes nothing: custom CSS has to stay inside its layer.`,
        });
        depth = 0;
      }
      i++;
    } else if (c === ";") {
      endImport();
      i++;
    } else i++;
  }
  endImport();
  if (depth > 0) problems.push({ line, message: t`An opening brace is never closed.` });
  return problems;
}

/** The stylesheet for `css`: the layer order, then `css` in `cs.user`. */
export function userStylesheet(css: string): string {
  return `${LAYER_ORDER}\n@layer cs.user {\n${css}\n}\n`;
}

export function problemText(problems: readonly CssProblem[]): string {
  return problems
    .map((problem) => t`Line ${{ line: problem.line }}: ${{ problem: problem.message }}`)
    .join("\n");
}

/**
 * Applies `css` as the one user `<style>`, or removes it for `null`, empty
 * CSS or CSS with problems. Returns the problems; when there are any,
 * nothing is applied.
 */
export function applyUserCss(css: string | null, doc: Document = document): CssProblem[] {
  const problems = css ? checkUserCss(css) : [];
  const existing = doc.head.querySelector<HTMLStyleElement>("style[data-cs-user]");
  if (!css?.trim() || problems.length > 0) {
    existing?.remove();
    return problems;
  }
  const style = existing ?? doc.createElement("style");
  style.dataset.csUser = "";
  style.textContent = userStylesheet(css);
  // Last in <head>; the layer order decides the cascade either way.
  doc.head.append(style);
  return problems;
}
