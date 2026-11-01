import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { SettingsApi, SettingsChanged, SettingsSnapshot } from "../../api/settings";
import { SchemaSettings } from "./SchemaSettings";
import { SettingsProvider } from "./useSettings";

afterEach(cleanup);

const namespace = "plugins.dev.example.gamemode";
/** A settings service whose saves take a while, like the app's IPC. */
function slowBackend() {
  let snapshot: SettingsSnapshot = {
    namespace,
    version: 1,
    revision: 0,
    value: { path: "", nice: 0 },
  };
  const listeners = new Set<(event: SettingsChanged) => void>();
  const saves: unknown[] = [];
  const api: SettingsApi = {
    sections: async () => [
      {
        namespace,
        version: 1,
        schema: {
          type: "object",
          properties: {
            path: { type: "string", title: "gamemoderun path" },
            nice: { type: "integer", title: "Niceness" },
          },
        },
        defaults: { path: "", nice: 0 },
      },
    ],
    get: async () => snapshot,
    set: async (_namespace, value, revision) => {
      saves.push(value);
      await new Promise((resolve) => setTimeout(resolve, 40));
      if (revision !== snapshot.revision) throw new Error("Settings changed; try again.");
      snapshot = { ...snapshot, revision: snapshot.revision + 1, value };
      for (const listener of listeners) listener({ namespace, version: 1 });
      return snapshot;
    },
    reset: async () => snapshot,
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  return { api, saves, value: () => snapshot.value as Record<string, unknown> };
}

/** Types like a keyboard: each key adds to what the field shows now. */
async function type(input: HTMLInputElement, text: string) {
  for (const key of text) {
    fireEvent.change(input, { target: { value: input.value + key } });
    await act(() => new Promise((resolve) => setTimeout(resolve, 5)));
  }
}

it("keeps every character typed faster than a save, and saves once typing pauses", async () => {
  const backend = slowBackend();
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <SettingsProvider api={backend.api}>
        <SchemaSettings namespace={namespace} title="GameMode" />
      </SettingsProvider>
    </QueryClientProvider>,
  );
  const path = (await screen.findByRole("textbox", {
    name: "gamemoderun path",
  })) as HTMLInputElement;
  fireEvent.focus(path);
  await type(path, "/tmp/e4-m4/bin/gamemoderun");
  expect(path.value).toBe("/tmp/e4-m4/bin/gamemoderun");
  await waitFor(() => expect(backend.value().path).toBe("/tmp/e4-m4/bin/gamemoderun"), {
    timeout: 3000,
  });
  // One save for the burst, not one per key.
  expect(backend.saves).toHaveLength(1);

  // Numbers save on blur too; text that isn't a number isn't saved.
  const nice = screen.getByRole("textbox", { name: "Niceness" }) as HTMLInputElement;
  fireEvent.focus(nice);
  await type(nice, "5");
  fireEvent.blur(nice);
  await waitFor(() => expect(backend.value().nice).toBe(5));
  expect(path.value).toBe("/tmp/e4-m4/bin/gamemoderun");
});
