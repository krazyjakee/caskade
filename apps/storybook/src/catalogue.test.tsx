import { anatomy, anatomyRoots, checkAnatomy } from "@caskade/ui";
import { cleanup, render } from "@testing-library/react";
import { beforeAll, expect, it } from "vitest";
import { type UiName, uiNames } from "./catalogue/fixtures";
import { uiStates } from "./catalogue/stories";
import { allStories } from "./test/stories";

/** Settable interaction attributes (hover, press and focus need real input). */
const settable = [
  "data-disabled",
  "data-selected",
  "data-indeterminate",
  "data-invalid",
  "data-readonly",
  "data-required",
  "data-open",
  "data-orientation",
  "data-pending",
];

/** Catalogued root → the attribute=value pairs it was rendered with, across every story. */
const rendered = new Map<string, Set<string>>();
const problems: string[] = [];

beforeAll(() => {
  for (const [title, Story] of allStories) {
    render(<Story />);
    for (const root of anatomyRoots(document.body)) {
      const name = root.getAttribute("data-cs") ?? "";
      const seen = rendered.get(name) ?? new Set<string>();
      for (const attribute of root.getAttributeNames())
        seen.add(`${attribute}=${root.getAttribute(attribute)}`);
      rendered.set(name, seen);
      for (const problem of checkAnatomy(root)) problems.push(`${title}: ${problem}`);
    }
    cleanup();
  }
});

it("has a story for every @caskade/ui component in the anatomy catalogue", () => {
  // Shell stories also render the app's own roots (About, Legal): apps/desktop checks those.
  const app = new Set(
    Object.entries(anatomy)
      .filter(([, entry]) => entry.source === "app")
      .map(([name]) => name),
  );
  const ui = [...rendered.keys()].filter((name) => !app.has(name));
  expect(ui.sort()).toEqual([...uiNames].sort());
});

it("renders only catalogued parts and state values", () => {
  expect(problems).toEqual([]);
});

it.each(uiNames)("shows every documented state of %s", (name: UiName) => {
  const seen = rendered.get(name) ?? new Set<string>();
  const missing = uiStates(name)
    .map(([attribute, value]) => `${attribute}=${value}`)
    .filter((pair) => !seen.has(pair));
  const attributes = new Set([...seen].map((pair) => pair.slice(0, pair.indexOf("="))));
  const interactions = (anatomy[name].interaction as readonly string[]).filter(
    (attribute) => settable.includes(attribute) && !attributes.has(attribute),
  );
  expect([...missing, ...interactions]).toEqual([]);
});
