import contract from "@caskade/tokens/manifest.json";
import { describe, expect, it } from "vitest";
import {
  compileTheme,
  compileTokens,
  rewriteAssetUrls,
  ThemeError,
  type ThemeFolder,
  themeChain,
} from "./compile";
import { DEFAULT_THEME, themeStylesheet, themes } from "./index";

const folder = (id: string, rest: Partial<ThemeFolder> = {}): ThemeFolder => ({
  dir: id,
  manifest: { id, name: id, version: "1.0.0" },
  css: "",
  assets: {},
  ...rest,
});

describe("compileTokens", () => {
  it("compiles overrides, aliases and $root like caskade-theme", () => {
    const tree = {
      color: {
        accent: { $root: { $value: "#E0457B" }, strong: { $value: "{color.palette.cyan}" } },
      },
      radius: { card: { $value: { value: 4, unit: "px" } } },
    };
    expect(compileTokens(tree, contract)).toEqual([
      ["--cs-color-accent", "#e0457b"],
      ["--cs-color-accent-strong", "var(--cs-color-palette-cyan)"],
      ["--cs-radius-card", "4px"],
    ]);
  });

  it("names every problem", () => {
    const tree = { color: { nope: { $value: "#fff" }, accent: { $root: { $value: "red" } } } };
    expect(() => compileTokens(tree, contract)).toThrow(ThemeError);
    try {
      compileTokens(tree, contract);
    } catch (error) {
      expect((error as ThemeError).problems).toEqual([
        "tokens.json: color.nope: is not a token of the theming contract",
        'tokens.json: color.accent: "red" is not a color',
      ]);
    }
  });
});

describe("compileTheme", () => {
  it("puts the chain's tokens on :root and its CSS, base first, in cs.theme", () => {
    const base = folder("caskade.base", {
      tokens: { color: { accent: { $root: { $value: "#111111" } } } },
      css: '[data-cs="hero"] { opacity: 1; }\n',
    });
    const leaf = folder("dev.example.leaf", {
      manifest: { id: "dev.example.leaf", name: "Leaf", version: "2.0.0", extends: "caskade.base" },
      tokens: { color: { accent: { $root: { $value: "#222222" } } } },
      css: '[data-cs="pill"] { background: url("assets/bg.png"); }',
      assets: { "assets/bg.png": "/served/bg.png" },
    });
    const chain = themeChain("dev.example.leaf", [leaf, base]);
    expect(chain.map((theme) => theme.manifest.id)).toEqual(["caskade.base", "dev.example.leaf"]);
    expect(compileTheme(chain, contract)).toBe(
      [
        "/* dev.example.leaf 2.0.0, compiled by the playground. */",
        "@layer cs.theme {",
        ":root {",
        "  --cs-color-accent: #222222;",
        "}",
        "",
        "/* caskade.base */",
        '[data-cs="hero"] { opacity: 1; }',
        "",
        "/* dev.example.leaf */",
        '[data-cs="pill"] { background: url("/served/bg.png"); }',
        "}",
        "",
      ].join("\n"),
    );
  });

  it("refuses missing bases and cycles", () => {
    const extending = (base: string) =>
      folder("a.b", { manifest: { id: "a.b", name: "A", version: "1.0.0", extends: base } });
    expect(() => themeChain("a.b", [extending("c.d")])).toThrow("a.b extends c.d");
    expect(() => themeChain("a.b", [extending("a.b")])).toThrow("extends cycle");
  });

  it("leaves URLs outside assets/ alone", () => {
    const css = "url(x.png) url('assets/a.woff2#f')";
    expect(rewriteAssetUrls(css, { "assets/a.woff2": "/a" })).toBe('url(x.png) url("/a#f")');
  });
});

it("compiles every theme folder in themes/", () => {
  expect(themes.map((theme) => theme.manifest.id)).toContain(DEFAULT_THEME);
  for (const theme of themes) {
    // Vitest stubs `?raw` CSS imports, so only the layer wrapper is checked here.
    const css = themeStylesheet(theme.manifest.id);
    expect(css.startsWith(`/* ${theme.manifest.id} `)).toBe(true);
    expect(css).toContain("@layer cs.theme {\n");
  }
});
