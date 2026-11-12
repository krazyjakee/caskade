// #130, #109: the create-plugin templates write plugin API 0.2 plugins: a
// Rust backend that handles a game event and, with a view, draws it with
// Caskade's components. The Rust side builds them, loads them and draws the
// view (crates/caskade-plugin-host/tests/scaffold.rs).
import { afterEach, expect, it } from "vitest";
import { generate } from "../../../../packages/create-plugin/generate.mjs";
import {
  joinPath as join,
  listFolder,
  readText,
  removeFolder,
  tempFolder,
} from "../../../../packages/create-plugin/testing.mjs";

const folders: string[] = [];
afterEach(() => {
  for (const folder of folders.splice(0)) removeFolder(folder);
});

function scaffold(ui: boolean, backend?: boolean) {
  const dir = tempFolder();
  folders.push(dir);
  const files = generate({ id: "dev.example.hello", name: "Hello", ui, backend, dir });
  return { dir, files };
}

it("the view template's backend draws the view and asks for nothing", () => {
  const { dir, files } = scaffold(true);
  expect(files.sort()).toEqual(
    [
      ".gitignore",
      "README.md",
      "backend/Cargo.toml",
      "backend/src/lib.rs",
      "package.json",
      "plugin.json",
      "scripts/build-backend.mjs",
      "scripts/extract-messages.mjs",
    ].sort(),
  );
  const manifest = JSON.parse(readText(dir, "plugin.json"));
  expect(manifest).toMatchObject({
    api: "^0.2",
    backend: "dist/backend.wasm",
    permissions: {},
    activation: ["onView:main", "onEvent:game.exited"],
    contributes: {
      views: [{ id: "main", title: "Hello", modes: ["desktop", "console"] }],
      hooks: ["game.exited"],
    },
  });
  expect(manifest.ui).toBeUndefined();
  const backend = readText(dir, "backend/src/lib.rs");
  expect(backend).toContain("fn render(");
  expect(backend).toContain('ui::button(t("Click"), "click")');
  expect(readText(dir, "backend/Cargo.toml")).toContain("caskade:plugin@0.2.0");
  expect(JSON.parse(readText(dir, "package.json")).scripts.build).toBe(
    "node scripts/build-backend.mjs",
  );
  expect(listFolder(dir, "locales")).toEqual([]);
});

it("the template without a view only handles events, and refuses a non-empty folder", () => {
  const { dir } = scaffold(false);
  const manifest = JSON.parse(readText(dir, "plugin.json"));
  expect(manifest.activation).toEqual(["onEvent:game.exited"]);
  expect(manifest.contributes.views).toBeUndefined();
  expect(readText(dir, "backend/src/lib.rs")).not.toContain("fn render(");
  expect(() => generate({ id: "dev.example.hello", name: "Hello", ui: true, dir })).toThrow(
    /not empty/,
  );
  // 0.1's UI-only plugins are gone: the backend draws the UI.
  expect(() =>
    generate({ id: "dev.example.hi", name: "Hi", ui: true, backend: false, dir: join(dir, "x") }),
  ).toThrow(/drawn by its backend/);
  expect(() => generate({ id: "Hello", name: "Hello", ui: true, dir: join(dir, "y") })).toThrow(
    /not a plugin id/,
  );
});
