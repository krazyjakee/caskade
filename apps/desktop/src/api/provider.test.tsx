import { onlineManager } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { createFixtureApi } from "./fixtures";
import { useGames, useLaunchGame } from "./hooks";
import { createLibraryQueryClient, LibraryProvider } from "./provider";

afterEach(() => {
  onlineManager.setOnline(true);
  cleanup();
});

describe("library query client", () => {
  it("keeps querying and launching while the OS reports no network", async () => {
    onlineManager.setOnline(false);
    const api = createFixtureApi();
    const client = createLibraryQueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <LibraryProvider api={api} client={client}>
        {children}
      </LibraryProvider>
    );
    const games = renderHook(() => useGames({ filter: "all", sort: "title" }), { wrapper });
    await waitFor(() => expect(games.result.current.isSuccess).toBe(true));
    expect(games.result.current.data?.pages[0]?.items.length).toBeGreaterThan(0);

    // A paused mutation never settles.
    const launch = renderHook(() => useLaunchGame(), { wrapper });
    await act(async () => {
      await expect(
        launch.result.current.mutateAsync({ entryId: "entry-1" }),
      ).resolves.toBeDefined();
    });
  });
});
