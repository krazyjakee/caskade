import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { StoreAccountsProvider } from "../../../api/storeAccounts";
import { createStoreAccountsFixtureApi, fixtureAccounts } from "../../../api/storeAccountsFixtures";
import { createSettingsFixture } from "../../../test/settingsFixture";
import { SettingsProvider } from "../useSettings";
import { StoreAccountsSettings } from "./StoreAccountsSettings";

afterEach(cleanup);

function renderPanel(api = createStoreAccountsFixtureApi()) {
  const settings = createSettingsFixture();
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SettingsProvider api={settings.api}>
        <StoreAccountsProvider api={api}>
          <StoreAccountsSettings />
        </StoreAccountsProvider>
      </SettingsProvider>
    </QueryClientProvider>,
  );
  return { api, settings };
}

/** The row of the account named `name`. */
async function row(name: string) {
  const label = await screen.findByText(name);
  return label.closest('[data-part="store-account"]') as HTMLElement;
}

it("shows every connection state with its trouble", async () => {
  renderPanel();
  for (const [name, state] of [
    ["Ada", "Connected"],
    ["Kid", "Refreshing…"],
    ["Ada Epic", "Offline"],
    ["ada-itch", "Sign-in expired"],
    ["Lutris here", "Disconnected"],
    ["Old Origin", "Not available"],
  ] as const) {
    expect(within(await row(name)).getByText(state)).toBeTruthy();
  }
  expect(within(await row("Ada Epic")).getByText(/can't be reached/)).toBeTruthy();
  expect(within(await row("ada-itch")).getByRole("button", { name: "Reconnect" })).toBeTruthy();
  expect(within(await row("Ada")).getByText(/lasts until Caskade closes/)).toBeTruthy();
  expect(within(await row("Kid")).getByRole("button", { name: "Stop" })).toBeTruthy();
  expect(
    within(await row("Old Origin")).getByRole("button", { name: "Refresh Old Origin" }),
  ).toHaveProperty("disabled", true);
});

it("connects with an API key, and a wrong key says why", async () => {
  const { api } = renderPanel();
  const section = (await screen.findByText("itch.io")).closest("section") as HTMLElement;
  fireEvent.click(within(section).getByRole("button", { name: "Connect another" }));
  const signIn = await screen.findByRole("region", { name: "Sign in to itch.io" });
  const key = within(signIn).getByLabelText("API key");
  fireEvent.change(key, { target: { value: "wrong" } });
  fireEvent.click(within(signIn).getByRole("button", { name: "Connect" }));
  expect(await within(signIn).findByText(/refused the sign-in/)).toBeTruthy();
  fireEvent.change(key, { target: { value: "new-key" } });
  fireEvent.click(within(signIn).getByRole("button", { name: "Connect" }));
  await waitFor(() =>
    expect(screen.queryByRole("region", { name: "Sign in to itch.io" })).toBeNull(),
  );
  expect(await screen.findByText("new-key")).toBeTruthy();
  expect(api.calls.some((call) => call.endsWith(" code"))).toBe(true);
});

it("a browser sign-in offers the page and can be cancelled", async () => {
  const { api } = renderPanel(createStoreAccountsFixtureApi([]));
  const section = (await screen.findByText("GOG")).closest("section") as HTMLElement;
  fireEvent.click(within(section).getByRole("button", { name: "Connect" }));
  const signIn = await screen.findByRole("region", { name: "Sign in to GOG" });
  expect(
    within(signIn).getByRole("link", { name: "Open the sign-in page" }).getAttribute("href"),
  ).toBe("https://auth.gog.com/auth");
  expect(within(signIn).getByText(/paste the address it ends on/)).toBeTruthy();
  expect(within(signIn).getByLabelText("Address or code")).toBeTruthy();
  fireEvent.click(within(signIn).getByRole("button", { name: "Cancel sign-in" }));
  await waitFor(() =>
    expect(screen.queryByRole("region", { name: "Sign in to Steam" })).toBeNull(),
  );
  expect(api.calls.some((call) => call.startsWith("cancel "))).toBe(true);
});

it("a device sign-in shows its code", async () => {
  renderPanel(createStoreAccountsFixtureApi([]));
  const section = (await screen.findByText("Epic Games")).closest("section") as HTMLElement;
  fireEvent.click(within(section).getByRole("button", { name: "Connect" }));
  const signIn = await screen.findByRole("region", { name: "Sign in to Epic Games" });
  expect(within(signIn).getByText("WDJB-MJHT")).toBeTruthy();
  expect(within(signIn).getByRole("link", { name: "Open the activation page" })).toBeTruthy();
});

it("disconnecting explains the library stays, and removing previews first", async () => {
  const { api } = renderPanel(createStoreAccountsFixtureApi(fixtureAccounts().slice(0, 1)));
  fireEvent.click(within(await row("Ada")).getByRole("button", { name: "Disconnect" }));
  const dialog = await screen.findByRole("alertdialog");
  expect(within(dialog).getByText(/Its games stay in your library/)).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Disconnect" }));
  await waitFor(() => expect(api.calls).toContain("disconnect a1"));
  expect(within(await row("Ada")).getByText("Disconnected")).toBeTruthy();

  fireEvent.click(within(await row("Ada")).getByRole("button", { name: "Remove…" }));
  const remove = await screen.findByRole("alertdialog");
  expect(await within(remove).findByText(/1 game only this account brought leaves/)).toBeTruthy();
  expect(
    within(remove).getByRole("list", { name: "Games that leave the library" }).textContent,
  ).toBe("Portal");
  fireEvent.click(within(remove).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(api.calls).toContain("remove a1"));
});

it("background refreshes are off until turned on", async () => {
  const { settings } = renderPanel();
  const toggle = await screen.findByRole("switch", { name: "Refresh in the background" });
  expect((toggle as HTMLInputElement).checked).toBe(false);
  fireEvent.click(toggle);
  await waitFor(() => expect(settings.values["core.storeAccounts"]?.backgroundRefresh).toBe(true));
  expect(screen.getByText("Import play time")).toBeTruthy();
  expect(screen.getByText(/counting time played through Caskade once/)).toBeTruthy();
});

it("Steam asks for a profile and a key, and explains privacy", async () => {
  const { api } = renderPanel(createStoreAccountsFixtureApi([]));
  const section = (await screen.findByText("Steam")).closest("section") as HTMLElement;
  fireEvent.click(within(section).getByRole("button", { name: "Connect" }));
  const signIn = await screen.findByRole("region", { name: "Sign in to Steam" });
  expect(within(signIn).getByText(/game details must be public/)).toBeTruthy();
  expect(within(signIn).getByRole("link", { name: "Get a key" })).toBeTruthy();
  const connect = within(signIn).getByRole("button", { name: "Connect" });
  expect(connect).toHaveProperty("disabled", true);
  fireEvent.change(within(signIn).getByLabelText("Steam profile"), {
    target: { value: "robinwalker" },
  });
  const key = within(signIn).getByLabelText("Steam Web API key") as HTMLInputElement;
  expect(key.type).toBe("password");
  fireEvent.change(key, { target: { value: "0123" } });
  fireEvent.click(within(signIn).getByRole("button", { name: "Connect" }));
  expect(await screen.findByText("robinwalker")).toBeTruthy();
  expect(api.calls.some((call) => call.endsWith(" fields"))).toBe(true);
});

it("says what each store brings in, and an achievements-only one brings no games", async () => {
  renderPanel();
  expect(
    await screen.findByText("Brings in every game your Steam account owns, installed or not."),
  ).toBeTruthy();
  expect(
    screen.getByText("Reads the games the Lutris launcher on this computer knows you own."),
  ).toBeTruthy();
  expect(
    screen.getByText("Shows your RetroAchievements achievements on the games in your library."),
  ).toBeTruthy();
  expect(screen.queryByText(/every game your RetroAchievements account owns/)).toBeNull();
});
