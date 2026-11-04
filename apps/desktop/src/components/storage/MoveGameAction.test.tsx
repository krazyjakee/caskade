import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  StorageProvider,
  useStorageAvailability,
  useUnavailableStorageEntries,
} from "../../api/StorageProvider";
import type { RootSummary, StorageApi } from "../../api/storage";
import { MoveGameAction } from "./MoveGameAction";

afterEach(cleanup);
function roots(): RootSummary[] {
  return ["one", "two"].map((id) => ({
    root: { id, path: `/games/${id}`, label: id, isDefault: id === "one" },
    status: "online",
    capabilities: {
      writable: true,
      executable: true,
      symlinks: true,
      caseSensitive: true,
      freeBytes: 10000,
      totalBytes: 20000,
      filesystem: "ext4",
      warnings: [],
    },
    folderTemplate: "{title}",
    games:
      id === "one"
        ? [
            {
              entryId: "entry",
              gameId: "game",
              title: "Fixture",
              relativeDir: "Fixture",
              sizeBytes: 100,
              available: true,
              moveBlocked: null,
            },
          ]
        : [],
    gamesBytes: 100,
    otherBytes: 100,
  }));
}
function api(data: RootSummary[]): StorageApi {
  return {
    list: vi.fn(async () => structuredClone(data)),
    preferences: async () => ({ alwaysUseDefault: false }),
    add: async () => {
      throw new Error("not used");
    },
    edit: async () => {
      throw new Error("not used");
    },
    setDefault: async () => {
      throw new Error("not used");
    },
    remove: async () => {},
    moveAndRemove: async () => {},
    chooseDirectory: async () => null,
    move: vi.fn(async () => {}),
    uninstall: async () => {},
  };
}
const client = () => new QueryClient({ defaultOptions: { queries: { retry: false } } });
function State() {
  const unavailable = useStorageAvailability("game");
  const external = useStorageAvailability("external");
  const entries = useUnavailableStorageEntries();
  return <p>{`${unavailable}:${external}:${entries.has("entry")}`}</p>;
}

describe("managed move action and availability", () => {
  it("loads roots lazily and submits managed entry and target ids", async () => {
    const storage = api(roots());
    render(
      <QueryClientProvider client={client()}>
        <MoveGameAction api={storage} gameId="game" entryId="entry" />
      </QueryClientProvider>,
    );
    expect(storage.list).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Move installed files…" }));
    await screen.findByText(/Fixture · 100 B/);
    fireEvent.click(screen.getByRole("button", { name: "Move game" }));
    await waitFor(() => expect(storage.move).toHaveBeenCalledWith("entry", "two"));
  });
  it("gives external games a reason and blocks running game actions", async () => {
    const storage = api([]);
    render(
      <QueryClientProvider client={client()}>
        <MoveGameAction api={storage} gameId="external" />
        <MoveGameAction api={storage} gameId="running" isRunning />
      </QueryClientProvider>,
    );
    const actions = screen.getAllByRole("button", { name: "Move installed files…" });
    expect((actions[1] as HTMLButtonElement).disabled).toBe(true);
    const externalAction = actions[0];
    if (!externalAction) throw new Error("missing external action");
    fireEvent.click(externalAction);
    await screen.findByText("Only Caskade-managed installations can be moved.");
    expect((screen.getByRole("button", { name: "Move game" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });
  it("updates only managed-copy availability when a volume disappears and returns", async () => {
    const data = roots();
    const storage = api(data);
    let notify = () => {};
    const stop = vi.fn();
    storage.subscribe = async (listener) => {
      notify = listener;
      return stop;
    };
    const view = render(
      <QueryClientProvider client={client()}>
        <StorageProvider api={storage}>
          <State />
        </StorageProvider>
      </QueryClientProvider>,
    );
    await waitFor(() => expect(storage.list).toHaveBeenCalled());
    const managed = data[0]?.games[0];
    if (!managed) throw new Error("missing managed fixture");
    managed.available = false;
    notify();
    await screen.findByText("true:false:true");
    managed.available = true;
    notify();
    await screen.findByText("false:false:false");
    view.unmount();
    expect(stop).toHaveBeenCalledTimes(1);
  });
});
