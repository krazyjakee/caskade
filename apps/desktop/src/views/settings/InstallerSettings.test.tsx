import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { SettingsApi, SettingsSnapshot } from "../../api/settings";
import { InstallerSettings } from "./InstallerSettings";
import { SettingsProvider } from "./useSettings";

afterEach(cleanup);
const defaults = { shortcuts: false, language: "", downloads: 2, runner: "proton" };
const schema = {
  type: "object",
  properties: {
    shortcuts: {
      type: "boolean",
      title: "Shortcuts from schema",
      description: "Fixture schema description.",
    },
    language: { type: "string", title: "Language from schema", maxLength: 12 },
    downloads: { type: "integer", title: "Downloads from schema", minimum: 1, maximum: 4 },
    runner: { $ref: "#/$defs/Runner", title: "Runner from schema" },
  },
  $defs: {
    Runner: {
      oneOf: [
        { const: "proton", title: "Proton via umu" },
        { const: "system_wine", title: "System Wine" },
      ],
    },
  },
};
function backend() {
  let snapshot: SettingsSnapshot = {
    namespace: "core.installer",
    version: 2,
    revision: 0,
    value: structuredClone(defaults),
  };
  let reject = false;
  const api: SettingsApi = {
    sections: async () => [{ namespace: "core.installer", version: 2, schema, defaults }],
    get: async () => structuredClone(snapshot),
    set: async (_namespace, value, revision) => {
      if (reject) throw new Error("Save rejected");
      if (revision !== snapshot.revision) throw new Error("Revision conflict");
      snapshot = { ...snapshot, revision: revision + 1, value: structuredClone(value) };
      return structuredClone(snapshot);
    },
    reset: async (namespace, revision) => api.set(namespace, structuredClone(defaults), revision),
    subscribe: async () => () => {},
  };
  return {
    api,
    get value() {
      return snapshot.value;
    },
    reject: () => {
      reject = true;
    },
  };
}
function panel(api: SettingsApi) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
        })
      }
    >
      <SettingsProvider api={api}>
        <InstallerSettings />
      </SettingsProvider>
    </QueryClientProvider>,
  );
}

it("renders authoritative schema labels, references and numeric limits and persists changes", async () => {
  const fixture = backend();
  const view = panel(fixture.api);
  const shortcuts = await screen.findByRole("switch", { name: "Shortcuts from schema" });
  expect(screen.getByText("Fixture schema description.")).toBeTruthy();
  const slider = screen.getByRole("slider", { name: "Downloads from schema" });
  expect((slider as HTMLInputElement).min).toBe("1");
  expect((slider as HTMLInputElement).max).toBe("4");
  expect(screen.getByRole("button", { name: /Runner from schema/ }).textContent).toContain(
    "Proton via umu",
  );
  fireEvent.click(shortcuts);
  await waitFor(() => expect(fixture.value.shortcuts).toBe(true));
  fireEvent.change(screen.getByRole("textbox", { name: "Language from schema" }), {
    target: { value: "french" },
  });
  await waitFor(() => expect(fixture.value.language).toBe("french"));
  view.unmount();
  panel(fixture.api);
  expect(
    ((await screen.findByRole("switch", { name: "Shortcuts from schema" })) as HTMLInputElement)
      .checked,
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Reset installer settings" }));
  // Nothing changes until the reset is confirmed, which names what changes (#44).
  const dialog = await screen.findByRole("alertdialog");
  await within(dialog).findByText("Shortcuts from schema");
  expect(within(dialog).getByText("Language from schema")).toBeTruthy();
  expect(fixture.value.shortcuts).toBe(true);
  fireEvent.click(within(dialog).getByRole("button", { name: "Reset" }));
  await waitFor(() => expect(fixture.value).toEqual(defaults));
});
it("shows backend rejection and retains the committed preference", async () => {
  const fixture = backend();
  panel(fixture.api);
  const shortcuts = await screen.findByRole("switch", { name: "Shortcuts from schema" });
  fixture.reject();
  fireEvent.click(shortcuts);
  expect((await screen.findByRole("alert")).textContent).toContain("Save rejected");
  expect(fixture.value.shortcuts).toBe(false);
});
