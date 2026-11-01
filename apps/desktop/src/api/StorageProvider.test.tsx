import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  STORAGE_EVENT_DEBOUNCE_MS,
  STORAGE_EVENT_MAX_WAIT_MS,
  StorageProvider,
  useUnavailableStorageEntries,
} from "./StorageProvider";
import type { RootSummary, StorageApi } from "./storage";

function root(freeBytes: number, available: boolean): RootSummary {
  return {
    root: { id: "r1", path: "/games", isDefault: true, label: null },
    status: "online",
    capabilities: {
      writable: true,
      executable: true,
      symlinks: true,
      caseSensitive: true,
      freeBytes,
      totalBytes: 1000,
      filesystem: "ext4",
      warnings: [],
    },
    folderTemplate: "{title}",
    games: [
      {
        entryId: "e1",
        gameId: "g1",
        title: "One",
        relativeDir: "One",
        sizeBytes: 1,
        available,
        moveBlocked: null,
      },
    ],
    gamesBytes: 1,
    otherBytes: null,
  };
}

function Probe({ values }: { values: ReadonlySet<string>[] }) {
  values.push(useUnavailableStorageEntries());
  return null;
}

function setup(list: () => Promise<RootSummary[]>) {
  let emit: () => void = () => {};
  const api = {
    list: vi.fn(list),
    subscribe: async (listener: () => void) => {
      emit = listener;
      return () => {};
    },
  } as unknown as StorageApi;
  const values: ReadonlySet<string>[] = [];
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <StorageProvider api={api}>
        <Probe values={values} />
      </StorageProvider>
    </QueryClientProvider>,
  );
  return { api, client, values, emit: () => emit() };
}

afterEach(() => vi.useRealTimers());

describe("StorageProvider", () => {
  it("keeps the availability value while only free space changes", async () => {
    let free = 100;
    let available = false;
    const { api, client, values } = setup(async () => [root(free--, available)]);
    await waitFor(() => expect(values.at(-1)?.has("e1")).toBe(true));
    const first = values.at(-1);
    await act(() => client.refetchQueries({ queryKey: ["storage"] }));
    await act(() => client.refetchQueries({ queryKey: ["storage"] }));
    expect(api.list).toHaveBeenCalledTimes(3);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(values.at(-1)).toBe(first);
    available = true;
    await act(() => client.refetchQueries({ queryKey: ["storage"] }));
    await waitFor(() => expect(values.at(-1)?.size).toBe(0));
    expect(values.at(-1)).not.toBe(first);
  });

  it("refetches once per burst of change events", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { api, emit } = setup(async () => [root(1, true)]);
    await waitFor(() => expect(api.list).toHaveBeenCalledTimes(1));
    for (let i = 0; i < 20; i++) {
      emit();
      await act(() => vi.advanceTimersByTimeAsync(STORAGE_EVENT_DEBOUNCE_MS / 5));
    }
    expect(api.list).toHaveBeenCalledTimes(1);
    await act(() => vi.advanceTimersByTimeAsync(STORAGE_EVENT_DEBOUNCE_MS));
    expect(api.list).toHaveBeenCalledTimes(2);
  });

  it("still refetches during an endless stream of events", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { api, emit } = setup(async () => [root(1, true)]);
    await waitFor(() => expect(api.list).toHaveBeenCalledTimes(1));
    const step = STORAGE_EVENT_DEBOUNCE_MS / 5;
    for (let elapsed = 0; elapsed <= STORAGE_EVENT_MAX_WAIT_MS; elapsed += step) {
      emit();
      await act(() => vi.advanceTimersByTimeAsync(step));
    }
    expect(api.list).toHaveBeenCalledTimes(2);
  });
});
