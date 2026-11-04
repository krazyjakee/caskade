/** #61: Platforms and ROM scanners are one DOM in two layouts. jsdom has no
 * layout, so these check the classes that make them and what a card shows. */
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { createEmulationFixtureApi } from "../../../api/emulationFixtures";
import { EmulationPanel, type EmulationSection } from "./EmulationPanel";
import { EmulationProvider } from "./EmulationProvider";

afterEach(cleanup);

function Panel({ initial }: { initial: EmulationSection }) {
  const [section, setSection] = useState(initial);
  return <EmulationPanel section={section} setSection={setSection} />;
}

function mount(section: EmulationSection) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <EmulationProvider api={createEmulationFixtureApi()}>
        <Panel initial={section} />
      </EmulationProvider>
    </QueryClientProvider>,
  );
}

const classes = (element: Element | null | undefined) => [...(element?.classList ?? [])];

/** `row` stacks below the frame's `@6xl` and is a table row above it, and
 * its card names every column but `unlabelled`. */
function expectBothLayouts(row: HTMLTableRowElement, unlabelled: string[]) {
  const table = row.closest("table") as HTMLTableElement;
  expect(classes(table.parentElement)).toContain("@container");
  expect(classes(table)).toEqual(expect.arrayContaining(["block", "@6xl:table"]));
  expect(classes(table.tHead)).toEqual(
    expect.arrayContaining(["hidden", "@6xl:table-header-group"]),
  );
  expect(classes(row)).toEqual(expect.arrayContaining(["grid", "@6xl:table-row"]));
  for (const cell of row.cells)
    expect(classes(cell)).toEqual(expect.arrayContaining(["block", "@6xl:table-cell"]));
  const columns = [...(table.tHead?.rows[0]?.cells ?? [])].map((cell) => cell.textContent ?? "");
  const labels = [...row.querySelectorAll('[data-part="column-label"]')];
  expect(labels.map((label) => label.textContent)).toEqual(
    columns.filter((name) => !unlabelled.includes(name)),
  );
  // The labels are the card's: the table has its header.
  for (const label of labels) expect(classes(label)).toContain("@6xl:hidden");
}

it("lays Platforms out as cards when narrow, with the same labelled controls", async () => {
  mount("platforms");
  const row = (await screen.findByText("Super Nintendo")).closest("tr") as HTMLTableRowElement;
  expectBothLayouts(row, ["Platform"]);
  expect(row.cells).toHaveLength(5);
  expect(within(row).getByRole("button", { name: /Default emulator for/ })).toBeTruthy();
  expect(within(row).getByRole("textbox", { name: "BIOS folder for SNES" })).toBeTruthy();
  expect(within(row).getByRole("button", { name: "Browse for BIOS folder for SNES" })).toBeTruthy();
});

it("lays ROM scanners out as cards when narrow, with the same labelled controls", async () => {
  mount("scanners");
  const name = await screen.findByRole("button", { name: "SNES collection" });
  const row = name.closest("tr") as HTMLTableRowElement;
  expectBothLayouts(row, ["Scanner", "Actions"]);
  expect(row.cells).toHaveLength(9);
  expect(
    within(row).getByRole("switch", { name: "Include SNES collection in library update" }),
  ).toBeTruthy();
  expect(within(row).getByRole("button", { name: "Scan SNES collection" })).toBeTruthy();
});
