import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { StoreAccountsProvider } from "../../api/storeAccounts";
import { createStoreAccountsFixtureApi } from "../../api/storeAccountsFixtures";
import { PlaytimeSources } from "./PlaytimeSources";

afterEach(cleanup);

function renderSources(gameId: string) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <StoreAccountsProvider api={createStoreAccountsFixtureApi([])}>
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
