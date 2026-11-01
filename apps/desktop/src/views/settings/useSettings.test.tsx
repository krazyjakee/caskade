import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { SettingsApi, SettingsChanged, SettingsSnapshot } from "../../api/settings";
import { AppearanceSettings } from "./AppearanceSettings";
import { SettingsProvider, useSettingsSection } from "./useSettings";

afterEach(cleanup);
const appearance = {
  theme: "nexus",
  density: "comfortable",
  cardSize: 176,
  uiScale: 100,
  showArtTitles: true,
  motion: "system",
  reduceEffects: false,
};
function fixture(namespace: string, defaults: Record<string, unknown>) {
  let snapshot: SettingsSnapshot = {
    namespace,
    version: 1,
    revision: 0,
    value: structuredClone(defaults),
  };
  let fail = false;
  const listeners = new Set<(event: SettingsChanged) => void>();
  const writes: number[] = [];
  const api: SettingsApi = {
    sections: async () => [{ namespace, version: 1, schema: {}, defaults }],
    get: async () => structuredClone(snapshot),
    set: async (_namespace, value, revision) => {
      await Promise.resolve();
      if (fail) {
        fail = false;
        throw new Error("Save rejected");
      }
      if (revision !== snapshot.revision) throw new Error("Revision conflict");
      writes.push(revision);
      snapshot = { ...snapshot, revision: revision + 1, value: structuredClone(value) };
      for (const listener of listeners) listener({ namespace, version: 1 });
      return structuredClone(snapshot);
    },
    reset: async (_namespace, revision) => api.set(namespace, structuredClone(defaults), revision),
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
  return {
    api,
    writes,
    get value() {
      return snapshot.value;
    },
    rejectNext: () => {
      fail = true;
    },
  };
}
function client() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
}
function panel(api: SettingsApi) {
  return render(
    <QueryClientProvider client={client()}>
      <SettingsProvider api={api}>
        <AppearanceSettings />
      </SettingsProvider>
    </QueryClientProvider>,
  );
}
it("the appearance panel saves through the backend, survives remount and resets", async () => {
  const backend = fixture("core.appearance", appearance);
  const view = panel(backend.api);
  const toggle = await screen.findByRole("switch", { name: "Reduce effects" });
  expect((toggle as HTMLInputElement).checked).toBe(false);
  fireEvent.click(toggle);
  await waitFor(() => expect(backend.value.reduceEffects).toBe(true));
  await waitFor(() => expect((toggle as HTMLInputElement).checked).toBe(true));
  view.unmount();
  panel(backend.api);
  expect(
    ((await screen.findByRole("switch", { name: "Reduce effects" })) as HTMLInputElement).checked,
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Reset appearance settings" }));
  await waitFor(() => expect(backend.value.reduceEffects).toBe(false));
  await waitFor(() =>
    expect(
      (screen.getByRole("switch", { name: "Reduce effects" }) as HTMLInputElement).checked,
    ).toBe(false),
  );
});
it("a rejected save shows an error and retains the persisted value", async () => {
  const backend = fixture("core.appearance", appearance);
  panel(backend.api);
  const toggle = await screen.findByRole("switch", { name: "Reduce effects" });
  backend.rejectNext();
  fireEvent.click(toggle);
  expect((await screen.findByRole("alert")).textContent).toContain("Save rejected");
  expect(backend.value.reduceEffects).toBe(false);
  expect((toggle as HTMLInputElement).checked).toBe(false);
});
function Probe() {
  const { settings, setSettings } = useSettingsSection<{ one: boolean; two: boolean }>(
    "test.options",
  );
  if (!settings) return <span>Loading</span>;
  return (
    <>
      <button type="button" onClick={() => setSettings((s) => ({ ...s, one: true }))}>
        First
      </button>
      <button type="button" onClick={() => setSettings((s) => ({ ...s, two: true }))}>
        Second
      </button>
    </>
  );
}

it("rapid changes serialize against fresh revisions instead of losing another control", async () => {
  const backend = fixture("test.options", { one: false, two: false });

  render(
    <QueryClientProvider client={client()}>
      <SettingsProvider api={backend.api}>
        <Probe />
      </SettingsProvider>
    </QueryClientProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "First" }));
  fireEvent.click(screen.getByRole("button", { name: "Second" }));
  await waitFor(() => expect(backend.value).toEqual({ one: true, two: true }));
  expect(backend.writes).toEqual([0, 1]);
});
it("delayed native subscriptions clean up safely under StrictMode", async () => {
  const backend = fixture("test.options", {});
  const pending: ((cleanup: () => void) => void)[] = [];
  backend.api.subscribe = () =>
    new Promise((resolve) => {
      pending.push(resolve);
    });
  const one = vi.fn();
  const two = vi.fn();
  const view = render(
    <StrictMode>
      <QueryClientProvider client={client()}>
        <SettingsProvider api={backend.api}>
          <span>Fixture</span>
        </SettingsProvider>
      </QueryClientProvider>
    </StrictMode>,
  );
  expect(pending).toHaveLength(2);
  pending[0]?.(one);
  await waitFor(() => expect(one).toHaveBeenCalledOnce());
  view.unmount();
  pending[1]?.(two);
  await waitFor(() => expect(two).toHaveBeenCalledOnce());
});
