import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import manifest from "../manifest.json" with { type: "json" };
import { build } from "../scripts/build.ts";
import { compile, setFromJson, type Tier, TokenError } from "./compile.ts";
import m0 from "./fixtures/m0-tokens.json" with { type: "json" };

const root = fileURLToPath(new URL("../../..", import.meta.url));

describe("the base token set", () => {
  it("resolves to exactly the values the app had before tokens became data", () => {
    const resolved = Object.fromEntries(
      manifest.filter((entry) => entry.name in m0).map((entry) => [entry.name, entry.resolved]),
    );
    expect(resolved).toEqual(m0);
  });

  it("compiles to the committed CSS (snapshot)", () => {
    const css = Object.entries(build()).find(([file]) => file.endsWith("tokens.css"))?.[1];
    expect(css).toMatchSnapshot();
  });

  it("draws console focus inside the element, with glows only there (#161)", () => {
    const value = (name: string) => manifest.find((entry) => entry.name === name)?.resolved;
    expect(value("--cs-focus-ring-offset")).toBe("2px");
    expect(value("--cs-focus-ring-inner")).toBe("0px");
    expect(value("--cs-focus-ring-halo")).toBe("0px");
    const set = setFromJson(
      "mode-console",
      "mode-console.json",
      JSON.parse(readFileSync(join(root, "packages/tokens/tokens/sets/mode-console.json"), "utf8")),
    );
    const ring = (set.tree as { focus: { ring: Record<string, { $value: string }> } }).focus.ring;
    expect(ring.offset?.$value).toBe(`-${ring.width?.$value}`);
    expect(Number.parseFloat(ring.inner?.$value ?? "0")).toBeGreaterThan(0);
  });
});

const tier = (tree: unknown, name: Tier = "semantic") => ({
  tier: name,
  file: `${name}.json`,
  tree,
});
const error = (run: () => unknown) => {
  try {
    run();
  } catch (problem) {
    expect(problem).toBeInstanceOf(TokenError);
    return (problem as Error).message;
  }
  throw new Error("expected a TokenError");
};

describe("errors name the file and the token", () => {
  it("rejects unknown aliases, cycles and aliases of another type", () => {
    expect(
      error(() => compile([tier({ color: { $type: "color", a: { $value: "{color.b}" } } })])),
    ).toBe("semantic.json: color.a: unknown alias {color.b}");
    expect(
      error(() =>
        compile([
          tier({
            color: { $type: "color", a: { $value: "{color.b}" }, b: { $value: "{color.a}" } },
          }),
        ]),
      ),
    ).toMatch(/^semantic\.json: color\.[ab]: alias cycle: color\.a → color\.b → color\.a$/);
    expect(
      error(() =>
        compile([
          tier({
            size: { $type: "dimension", $value: "1px" },
            color: { $type: "color", a: { $value: "{size}" } },
          }),
        ]),
      ),
    ).toBe("semantic.json: color.a: alias {size} is a dimension, not a color");
  });

  it("rejects unknown types and bad values", () => {
    expect(error(() => compile([tier({ a: { $type: "gradient", $value: "x" } })]))).toBe(
      'semantic.json: a: unknown $type "gradient"',
    );
    expect(error(() => compile([tier({ a: { $value: "#fff" } })]))).toBe(
      "semantic.json: a: token has no $type",
    );
    const bad: [string, unknown][] = [
      ["color", "red"],
      ["color", "#12345"],
      ["dimension", "10"],
      ["dimension", "10ms"],
      ["duration", "1px"],
      ["number", "1"],
      ["cubicBezier", [2, 0, 0, 1]],
      ["fontFamily", ['Bad"; } body { x']],
      ["keyword", "x; color: red"],
    ];
    for (const [type, value] of bad)
      expect(error(() => compile([tier({ a: { $type: type, $value: value } })]))).toMatch(
        /^semantic\.json: a: expected /,
      );
  });

  it("rejects duplicates across tiers, children of tokens and bad names", () => {
    const a = { a: { $type: "number", $value: 1 } };
    expect(error(() => compile([tier(a, "primitive"), tier(a, "semantic")]))).toBe(
      "semantic.json: a: already declared in primitive.json",
    );
    expect(
      error(() => compile([tier({ a: { $type: "number", $value: 1, b: { $value: 2 } } })])),
    ).toMatch(/a token can't have children/);
    expect(error(() => compile([tier({ Bad_Name: { $type: "number", $value: 1 } })]))).toMatch(
      /invalid name/,
    );
  });

  it("checks override sets against the base", () => {
    const base = [tier({ a: { $type: "duration", $value: "1s" } })];
    const set = (tree: unknown) =>
      setFromJson("s", "sets/s.json", {
        $extensions: { "dev.caskade.condition": { selector: '[data-x="y"]' } },
        ...(tree as object),
      });
    expect(compile(base, [set({ a: { $value: "0ms" } })]).css).toContain(
      '[data-x="y"] {\n  --cs-a: 0ms;\n}',
    );
    expect(error(() => compile(base, [set({ b: { $value: "0ms" } })]))).toBe(
      "sets/s.json: b: a set can only override declared tokens",
    );
    expect(error(() => compile(base, [set({ a: { $value: "1px" } })]))).toMatch(
      /sets\/s\.json: a: expected a duration/,
    );
    expect(error(() => setFromJson("s", "sets/s.json", {}))).toMatch(/needs \$extensions/);
    expect(
      error(() =>
        setFromJson("s", "sets/s.json", {
          $extensions: { "dev.caskade.condition": { media: "screen{}" } },
        }),
      ),
    ).toMatch(/unsupported media query/);
  });
});

/** Every `--cs-*` the app and packages read must be a declared token. */
it("the app reads no undeclared --cs-* token", () => {
  const declared = new Set(manifest.map((entry) => entry.name));
  const sources = (dir: string): string[] =>
    readdirSync(dir).flatMap((name) => {
      const path = join(dir, name);
      if (name === "node_modules" || name === "bindings.ts") return [];
      if (statSync(path).isDirectory()) return sources(path);
      return /\.(css|rs|tsx?)$/.test(name) && !/\.test\.tsx?$/.test(name) ? [path] : [];
    });
  const roots = [
    join(root, "apps/native/src"),
    join(root, "apps/native/styles"),
    ...readdirSync(join(root, "packages")).map((p) => join(root, "packages", p, "src")),
  ];
  const undeclared = new Set<string>();
  let read = 0;
  for (const dir of roots) {
    try {
      statSync(dir);
    } catch {
      continue;
    }
    for (const file of sources(dir)) {
      if (file.includes(join("packages", "tokens"))) continue;
      for (const [name] of readFileSync(file, "utf8").matchAll(
        /--cs-[a-z0-9]+(?:-[a-z0-9]+)*(?![a-z0-9-])/g,
      )) {
        read++;
        if (!declared.has(name)) undeclared.add(`${name} (${file.replace(root, "")})`);
      }
    }
  }
  expect(read).toBeGreaterThan(50);
  expect([...undeclared]).toEqual([]);
});
