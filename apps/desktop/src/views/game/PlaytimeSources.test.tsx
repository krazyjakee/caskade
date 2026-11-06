import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { StoreAccountsProvider } from "../../api/storeAccounts";
import { createStoreAccountsFixtureApi } from "../../api/storeAccountsFixtures";
import { PlaytimeSources } from "./PlaytimeSources";

afterEach(cleanup);

function renderSources(gameId: string, api = createStoreAccountsFixtureApi([])) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <StoreAccountsProvider api={api}>
        <PlaytimeSources gameId={gameId} totalSeconds={12 * 3600} />
      </StoreAccountsProvider>
    </QueryClientProvider>,
  );
}

it("says what stores reported beyond Caskade's own time", async () => {
  renderSources("g1");
  const line = await screen.findByText(/more reported by Steam \(Ada\)/);
  expect(line.getAttribute("title")).toMatch(/^Steam: /);
});

it("says nothing when only Caskade tracked the game", async () => {
  renderSources("g2");
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(screen.queryByText(/reported by/)).toBeNull();
});

it("says nothing when a store reported no time beyond Caskade's own", async () => {
  const api = createStoreAccountsFixtureApi([]);
  const seen: string[] = [];
  api.playtime = async (gameId) => {
    seen.push(gameId);
    return {
      totalMs: 0,
      trackedMs: 0,
      reportedMs: 0,
      lastPlayedMs: null,
      sources: [
        {
          provider: "steam",
          accountName: null,
          totalMs: 0,
          countedMs: 0,
          lastPlayedMs: null,
          observedAtMs: 1_700_000_000_000,
        },
      ],
    };
  };
  renderSources("g2", api);
  await waitFor(() => expect(seen).toEqual(["g2"]));
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(screen.queryByText(/reported by/)).toBeNull();
});
