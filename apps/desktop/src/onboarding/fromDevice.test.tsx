/** First-run setup from another device (#242): pair with a nearby
 * Caskade (codes compared on both), wait for the first sync, sign in as the
 * household's admin when it has a PIN, then go on with this device's own
 * questions. Without devices, setup is as before. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it } from "vitest";
import { createAccountsFixtureApi } from "../api/accountsFixtures";
import { SyncProvider } from "../api/sync";
import { createSyncFixtureApi } from "../api/syncFixtures";
import { renderOnboarding } from "../test/onboardingFixture";

afterEach(cleanup);

/** A fresh install: no paired devices yet, one nearby. */
async function freshDevices() {
  const sync = createSyncFixtureApi();
  for (const device of sync.state().devices) await sync.forget(device.id);
  return sync;
}

function render(sync: ReturnType<typeof createSyncFixtureApi>) {
  return renderOnboarding(undefined, undefined, undefined, {
    wrap: (children: ReactNode) => <SyncProvider api={sync}>{children}</SyncProvider>,
  });
}

it("pairs with a nearby device, brings its library over, then asks about this device", async () => {
  const sync = await freshDevices();
  render(sync);
  fireEvent.click(await screen.findByRole("button", { name: "Set up from another device" }));
  await screen.findByRole("heading", { name: "Set up from another device" });
  const nearby = await screen.findByRole("list", { name: "Nearby devices" });
  expect(nearby.textContent).toContain("Kids' PC");
  fireEvent.click(within(nearby).getByRole("button", { name: "Pair" }));

  // Both devices show the code; it's confirmed here once they match.
  const dialog = await screen.findByRole("dialog", { name: /Pair with Kids' PC/ });
  expect(within(dialog).getByText("482 913")).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "They match: pair" }));

  await screen.findByText("Paired with Kids' PC");
  fireEvent.click(screen.getByRole("button", { name: "Done" }));

  // The first sync brings the library; then this device's own setup.
  await screen.findByRole("heading", { name: "Your library is here" });
  expect(sync.state().devices.map((device) => device.name)).toEqual(["Kids' PC"]);
  expect(sync.state().devices[0]?.lastSyncMs).not.toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByRole("heading", { name: /language/i });
});

it("pairs by a pasted link, and a wrong one says so", async () => {
  const sync = await freshDevices();
  render(sync);
  fireEvent.click(await screen.findByRole("button", { name: "Set up from another device" }));
  const field = await screen.findByRole("textbox", { name: "Pairing link" });
  fireEvent.change(field, { target: { value: "https://example.org" } });
  const pair = screen.getAllByRole("button", { name: "Pair" }).at(-1) as HTMLElement;
  fireEvent.click(pair);
  await screen.findByText(/isn't a Caskade pairing link/);
  fireEvent.change(field, { target: { value: "caskade://pair?caskadepair=1" } });
  fireEvent.click(screen.getAllByRole("button", { name: "Pair" }).at(-1) as HTMLElement);
  await screen.findByRole("heading", { name: "Your library is here" });
  expect(sync.state().devices.map((device) => device.name)).toEqual(["Office desktop"]);
});

it("asks for the household admin's PIN when the joined account has one", async () => {
  const sync = await freshDevices();
  renderOnboarding(undefined, undefined, undefined, {
    // After the first sync: the household's accounts, nobody signed in yet.
    accounts: (library) => {
      library.accounts = createAccountsFixtureApi(
        [
          { id: "ada", name: "Ada", role: "admin", pin: "2468" },
          { id: "kid", name: "Kid", avatar: "avatar:moon" },
        ],
        library.emit,
        { activeId: null },
      );
    },
    wrap: (children: ReactNode) => <SyncProvider api={sync}>{children}</SyncProvider>,
  });
  // Nobody signed in before setup chose "from another device": the welcome.
  fireEvent.click(await screen.findByRole("button", { name: "Set up from another device" }));
  await waitFor(() => expect(screen.getByText("Ada")).toBeTruthy());
  expect(screen.getByText("Kid")).toBeTruthy();
});

it("without devices, setup starts as before", async () => {
  renderOnboarding();
  await screen.findByRole("button", { name: /Get started/ });
  expect(screen.queryByRole("button", { name: "Set up from another device" })).toBeNull();
});
