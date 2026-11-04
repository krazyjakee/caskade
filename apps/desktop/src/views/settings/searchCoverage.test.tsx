/** #56: every preference row Settings shows can be found by searching for
 * its label. A row a panel gains without a `searchIndex.ts` entry fails
 * here, named with its section. */
import { i18n } from "@lingui/core";
import { cleanup, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fakeLayout, renderApp } from "../../test/harness";
import { type SettingsTab, settingsTabs } from "../SettingsView";
import { settingsSearchIndex } from "./searchIndex";

vi.setConfig({ testTimeout: 30_000 });

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

/** Rows named by data rather than by the panel (the version running), and
 * the test fixture's own installer schema. */
const named: Partial<Record<SettingsTab, RegExp>> = {
  general: /^Caskade v/,
  installer: /^(Create desktop shortcuts|Parallel downloads)$/,
};
/** A row per store account: found through the section's words. */
const dataRows = '[data-part="store-account"]';

/** Waits until a section's rows have stopped arriving. */
async function rowsOf(tab: SettingsTab) {
  renderApp(`/settings?tab=${tab}`);
  await screen.findByRole("tab", { selected: true });
  let last = -1;
  let steady = 0;
  let labels: string[] = [];
  await waitFor(
    () => {
      labels = Array.from(
        document.querySelectorAll(
          '[role="tabpanel"] [data-cs="setting-row"] > div [data-part="label"]',
        ),
      )
        .filter((label) => !label.closest(dataRows))
        .map((label) => label.textContent?.trim() ?? "")
        .filter(Boolean);
      // The same rows on three looks in a row: loading is over.
      steady = labels.length === last ? steady + 1 : 0;
      last = labels.length;
      expect(steady).toBeGreaterThanOrEqual(2);
    },
    { interval: 300, timeout: 10_000 },
  );
  cleanup();
  return labels;
}

it("indexes every row Settings shows", async () => {
  const missing: string[] = [];
  for (const { id: tab } of settingsTabs) {
    const indexed = new Set(
      settingsSearchIndex.filter((entry) => entry.tab === tab).map((entry) => i18n._(entry.label)),
    );
    for (const label of await rowsOf(tab)) {
      if (!indexed.has(label) && !named[tab]?.test(label)) missing.push(`${tab}: ${label}`);
    }
  }
  expect(missing).toEqual([]);
});
