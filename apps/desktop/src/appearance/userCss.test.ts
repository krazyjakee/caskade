import { afterEach, expect, it } from "vitest";
import { readText } from "../../../../packages/create-plugin/testing.mjs";
import {
  applyUserCss,
  checkUserCss,
  isLocalUrl,
  LAYER_ORDER,
  problemText,
  userStylesheet,
} from "./userCss";

afterEach(() => {
  for (const element of document.head.querySelectorAll("style[data-cs-user]")) element.remove();
});

const messages = (css: string) => checkUserCss(css).map((problem) => problem.message);

it("refuses remote imports and URLs with a message", () => {
  expect(checkUserCss('@import url("https://example.com/look.css");')).toEqual([
    {
      line: 1,
      message:
        "@import of https://example.com/look.css is refused: custom CSS can't load anything from other origins.",
    },
  ]);
  expect(messages("\n\n@import 'http://evil.test/a.css';")).toEqual([
    "@import of http://evil.test/a.css is refused: custom CSS can't load anything from other origins.",
  ]);
  for (const css of [
    '[data-cs="hero"] { background: url(https://example.com/bg.png); }',
    '[data-cs="hero"] { background: url( "//example.com/bg.png" ); }',
    // `\\` is a backslash in CSS, which URLs read as a slash.
    '[data-cs="hero"] { background: URL(/\\\\example.com/bg.png); }',
    '[data-cs="hero"] { background: \\75 rl(https://example.com/bg.png); }',
    '[data-cs="hero"] { background: u\\rl("ht\\74 ps://example.com/bg.png"); }',
    '[data-cs="hero"] { background: image-set("https://example.com/a.png" 1x); }',
    '@font-face { font-family: X; src: url("ftp://example.com/x.woff2"); }',
  ])
    expect(messages(css), css).toEqual([
      expect.stringMatching(/^url\(.+\) is refused: custom CSS can't load anything/),
    ]);
  // Line numbers count from the top of the box.
  const css = "a {}\n\n[data-cs=hero] {\n  background: url(http://x.test/a.png);\n}";
  expect(checkUserCss(css)[0]?.line).toBe(4);
});

it("keeps local URLs, URL-looking text and comments", () => {
  expect(
    checkUserCss(`
      /* url(https://example.com/a.png) and @import "x"; are only a comment */
      [data-cs="hero"] {
        background: url(data:image/png;base64,AAAA), url("caskade-theme://localhost/x/a.png");
        mask: url(http://caskade-theme.localhost/x/m.svg);
        content: "https://example.com is just text";
        --cs-color-accent: #e0457b;
      }
      @media (min-width: 40rem) { [data-cs="game-card"] { color: red; } }
    `),
  ).toEqual([]);
  for (const url of [
    "assets/a.png",
    "#frag",
    "data:font/woff2;base64,AA",
    "http://localhost:1420/x",
  ])
    expect(isLocalUrl(url), url).toBe(true);
  for (const url of [
    "https://example.com/a.png",
    "//example.com",
    "\\\\example.com",
    "http://localhost.example.com/",
    "file:///etc/passwd",
    " h\ttps://example.com",
  ])
    expect(isLocalUrl(url), url).toBe(false);
});

it("refuses any @import and braces that would leave the user layer", () => {
  expect(messages('@import "local.css";')).toEqual([
    "@import isn't supported in custom CSS: paste the rules in instead.",
  ]);
  expect(messages('[data-cs="hero"] { color: red; } } [data-cs="hero"] { color: blue; }')).toEqual([
    "A closing brace closes nothing: custom CSS has to stay inside its layer.",
  ]);
  expect(messages('[data-cs="hero"] { color: red;')).toEqual(["An opening brace is never closed."]);
  expect(messages('[data-cs="hero"] { color: red; } /* open')).toEqual([
    "A comment is never closed.",
  ]);
  expect(messages('[data-cs="hero"] { content: "}"; }')).toEqual([]);
  // Unfinished input ends the scan instead of hanging it.
  expect(messages('[data-cs="hero"] { color: red; } \\')).toEqual([]);
  expect(messages('[data-cs="hero"] { background: url(https://example.com')).toEqual([
    "url(https://example.com) is refused: custom CSS can't load anything from other origins.",
    "An opening brace is never closed.",
  ]);
});

it("applies as one style in the user layer, and removes it when empty or refused", () => {
  const rule = '[data-cs="hero"] { color: red; }';
  expect(applyUserCss(rule)).toEqual([]);
  expect(applyUserCss(`${rule}\n[data-cs="sidebar"] { color: blue; }`)).toEqual([]);
  const styles = document.head.querySelectorAll("style[data-cs-user]");
  expect(styles).toHaveLength(1);
  expect(styles[0]?.textContent).toBe(
    `${LAYER_ORDER}\n@layer cs.user {\n${rule}\n[data-cs="sidebar"] { color: blue; }\n}\n`,
  );
  const refused = applyUserCss(`${rule}\n@import url(https://example.com/x.css);`);
  expect(problemText(refused)).toMatch(/^Line 2: @import of https:\/\/example\.com\/x\.css /);
  expect(document.head.querySelectorAll("style[data-cs-user]")).toHaveLength(0);
  applyUserCss(rule);
  applyUserCss(null);
  expect(document.head.querySelectorAll("style[data-cs-user]")).toHaveLength(0);
  applyUserCss(rule);
  applyUserCss("  \n");
  expect(document.head.querySelectorAll("style[data-cs-user]")).toHaveLength(0);
});

// Read from disk: Vitest's CSS handling empties `?raw` CSS imports.
const file = (relative: string) => readText("/", new URL(relative, import.meta.url).pathname);

/** The layers of a `@layer a, b, c;` statement, in order. */
function layers(css: string): string[] {
  const statement = /@layer\s+([^;{]+);/.exec(css)?.[1] ?? "";
  return statement.split(",").map((name) => name.trim());
}

/** The winning declaration among normal (not `!important`) declarations of
 * equal specificity: the later layer wins whatever the source order. */
function cascade(order: string[], declarations: { layer: string; value: string }[]) {
  const ranked = [...declarations].sort((a, b) => order.indexOf(a.layer) - order.indexOf(b.layer));
  return ranked.at(-1)?.value;
}

it("a custom rule beats the same property set by the theme and by a component", () => {
  const order = layers(file("../styles/index.css"));
  expect(order).toEqual([
    "cs.reset",
    "cs.tokens",
    "cs.base",
    "cs.components",
    "cs.plugins",
    "cs.theme",
    "cs.user",
  ]);
  // The user sheet restates the same order, so it holds even if it loads
  // first, and keeps its rules in cs.user.
  const user = userStylesheet('[data-cs="hero"] { color: hotpink; }');
  expect(layers(user)).toEqual(order);
  expect(user).toContain('@layer cs.user {\n[data-cs="hero"] { color: hotpink; }\n}');
  // The theme compiles into cs.theme (crates/caskade-theme), components into
  // cs.components (Tailwind's utilities, index.css).
  expect(file("../styles/index.css")).toContain(
    '@import "tailwindcss/utilities.css" layer(cs.components);',
  );
  expect(
    cascade(order, [
      // The user's <style> is in <head> before the theme's <link> here: source
      // order loses to the layer.
      { layer: "cs.user", value: "hotpink" },
      { layer: "cs.theme", value: "cyan" },
      { layer: "cs.components", value: "white" },
    ]),
  ).toBe("hotpink");
  // A stray } can't move rules out of the layer (unlayered CSS would beat it).
  expect(checkUserCss('} [data-cs="hero"] { color: hotpink; }')).toHaveLength(1);
});
