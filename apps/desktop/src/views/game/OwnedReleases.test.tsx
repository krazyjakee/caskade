import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { StoreAccountsProvider } from "../../api/storeAccounts";
import { createStoreAccountsFixtureApi } from "../../api/storeAccountsFixtures";
import { fakeLayout, renderApp } from "../../test/harness";
import { OwnedReleases } from "./OwnedReleases";

afterEach(cleanup);

function renderOwned(gameId: string, primary = true) {
  const api = createStoreAccountsFixtureApi([]);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <StoreAccountsProvider api={api}>
        <OwnedReleases gameId={gameId} status="not-installed" primary={primary} />
      </StoreAccountsProvider>
    </QueryClientProvider>,
  );
  return api;
}

it("installs through the launcher and waits for it, with a way out", async () => {
  const api = renderOwned("g1");
  expect(await screen.findByText("Owned on Steam (Ada)")).toBeTruthy();
  expect(screen.getByText(/Library refreshed/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Install with Steam" }));
  expect(await screen.findByText("Waiting for Steam to finish installing it.")).toBeTruthy();
  expect(api.calls).toContain("install g1-owned");
  fireEvent.click(screen.getByRole("button", { name: "Stop waiting" }));
  await screen.findByRole("button", { name: "Install with Steam" });
});

it("a missing launcher and another system are said, without an Install", async () => {
  renderOwned("g2");
  expect(await screen.findByText("GOG Galaxy isn't installed on this computer.")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Install/ })).toBeNull();
  cleanup();
  renderOwned("g3");
  expect(await screen.findByText(/Runs on Windows, not on this computer's system/)).toBeTruthy();
});

it("a pad installs an owned game from its page", async () => {
  const undo = fakeLayout();
  const api = createStoreAccountsFixtureApi(
    [],
    [
      {
        entryId: "fixture-1-owned",
        storeAccountId: "a1",
        provider: "steam",
        providerName: "Steam",
        accountName: "Ada",
        productId: "620",
        title: "Portal 2",
        state: { type: "available", launcher: "Steam" },
        refreshedAtMs: null,
      },
    ],
  );
  const app = renderApp("/games/fixture-1", {
    mode: "console",
    wrap: (children) => <StoreAccountsProvider api={api}>{children}</StoreAccountsProvider>,
  });
  const install = await screen.findByRole("button", { name: "Install with Steam" });
  for (let i = 0; i < 40 && document.activeElement !== install; i++) {
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  expect(document.activeElement).toBe(install);
  app.pad.press("confirm");
  await waitFor(() => expect(api.calls).toContain("install fixture-1-owned"));
  undo();
});
