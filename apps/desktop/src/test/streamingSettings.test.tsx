/** #265: Settings → Streaming over fixture APIs, by mouse and by pad: hosting
 * on (and someone else's streaming), a device asking to pair, pairing this
 * device with another (a wrong code, then the right one), adding a device
 * by address, the stream quality and a standard account's view. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import type { FixtureAccount } from "../api/accountsFixtures";
import { StreamingApiProvider } from "../api/streaming";
import { createStreamingFixtureApi, type StreamingFixtureOptions } from "../api/streamingFixtures";
import { fakeLayout, fakeViewport, focused, renderApp, until } from "./harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

function open(
  path: string,
  options: StreamingFixtureOptions & {
    mode?: "desktop" | "console";
    accounts?: FixtureAccount[];
    activeAccount?: string;
  } = {},
) {
  const { mode, accounts, activeAccount, ...fixture } = options;
  const streaming = createStreamingFixtureApi(fixture);
  const app = renderApp(path, {
    mode,
    accounts,
    activeAccount,
    wrap: (app) => <StreamingApiProvider api={streaming}>{app}</StreamingApiProvider>,
  });
  return { app, streaming };
}

const hostingSwitch = () =>
  screen.findByRole("switch", { name: "Let other devices stream games from this one" });

it("an admin lets other devices play, and pairs one that asks", async () => {
  const { streaming } = open("/settings?tab=streaming");
  fireEvent.click(await hostingSwitch());
  await screen.findByText("Ready. Devices on this network can pair and play.");
  expect(streaming.hostingCalls).toEqual([true]);
  expect(
    screen.getByText(
      "None yet. On another device, open Settings → Streaming in Caskade and choose Pair with this one.",
    ),
  ).toBeTruthy();

  // A device asks: its request waits in the tab, for the code it shows.
  streaming.request({ id: "r1", name: "Steam Deck", address: "192.168.1.40" });
  const requests = await screen.findByText(
    "Steam Deck wants to stream games from this device.",
    {},
    { timeout: 5_000 },
  );
  const form = requests.closest("form") as HTMLElement;
  const pair = within(form).getByRole("button", { name: "Pair" });
  expect(pair.hasAttribute("disabled") || pair.dataset.disabled !== undefined).toBe(true);
  fireEvent.change(within(form).getByRole("textbox", { name: "Code shown on Steam Deck" }), {
    target: { value: "48a31" },
  });
  fireEvent.change(within(form).getByRole("textbox", { name: "Call it" }), {
    target: { value: "Deck" },
  });
  fireEvent.click(within(form).getByRole("button", { name: "Pair" }));
  await waitFor(() =>
    expect(streaming.accepted).toEqual([{ id: "r1", code: "4831", name: "Deck" }]),
  );
  const paired = await screen.findByText("Deck");
  // Forgetting it asks first (#49), then ends its pairing.
  const row = paired.closest("li") as HTMLElement;
  fireEvent.click(within(row).getByRole("button", { name: "Forget Deck" }));
  within(row).getByText("It needs a new code to play from this device again.");
  fireEvent.click(within(row).getByRole("button", { name: "Keep" }));
  expect(screen.getByText("Deck")).toBeTruthy();
  fireEvent.click(within(row).getByRole("button", { name: "Forget Deck" }));
  fireEvent.click(await within(row).findByRole("button", { name: "Forget Deck" }));
  await waitFor(() => expect(screen.queryByText("Deck")).toBeNull());
});

it("streaming someone already set up is used with its sign-in", async () => {
  const { streaming } = open("/settings?tab=streaming", { existing: true });
  fireEvent.click(await hostingSwitch());
  const dialog = await screen.findByRole("dialog", { name: "Streaming is already set up" });
  fireEvent.change(within(dialog).getByRole("textbox", { name: "User name" }), {
    target: { value: "admin" },
  });
  fireEvent.change(within(dialog).getByLabelText("Password"), { target: { value: "wrong" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Use it" }));
  await within(dialog).findByText("That user name or password isn't right");
  fireEvent.change(within(dialog).getByLabelText("Password"), { target: { value: "secret" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Use it" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await screen.findByText("Ready. Devices on this network can pair and play.");
  expect(streaming.hostingCalls).toEqual([true]);
});

it("pairs this device with another: a wrong code, then the right one", async () => {
  const { streaming } = open("/settings?tab=streaming");
  const study = (await screen.findByText("Study")).closest("li") as HTMLElement;
  expect(study.textContent).toContain("Ready · Not paired");
  const den = (await screen.findByText("Living room")).closest("li") as HTMLElement;
  expect(den.textContent).toContain("Ready · Paired · 12 games");

  fireEvent.click(within(study).getByRole("button", { name: "Pair…" }));
  const dialog = await screen.findByRole("dialog", { name: "Pair with Study" });
  await within(dialog).findByText("4831");
  expect(dialog.textContent).toContain(
    "On Study, open Settings → Streaming in Caskade and enter this code:",
  );
  streaming.finishPairing("fixture-study", "wrongCode");
  await within(dialog).findByText(
    "The code entered on Study didn't match. Try again with a new code.",
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Try again" }));
  await within(dialog).findByText("4831");
  streaming.finishPairing("fixture-study", "paired");
  await within(dialog).findByText(
    "Paired. Study's games show in your library as Stream from Study.",
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Done" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() =>
    expect((screen.getByText("Study").closest("li") as HTMLElement).textContent).toContain(
      "Paired",
    ),
  );
});

it("adds a device by its address, and saves the stream quality", async () => {
  const { app } = open("/settings?tab=streaming");
  fireEvent.change(await screen.findByRole("textbox", { name: "Add a device by its address" }), {
    target: { value: "10.0.0.7" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  const added = await screen.findByText("10.0.0.7");
  expect((added.closest("li") as HTMLElement).textContent).toContain("Can't be reached");

  const resolution = screen.getByRole("button", { name: /Resolution/ });
  fireEvent.click(resolution);
  fireEvent.click(await screen.findByRole("option", { name: "1920 × 1080" }));
  await waitFor(async () => {
    const saved = await app.settings.api.get("core.streaming");
    expect((saved.value as { quality: { resolution: string } }).quality.resolution).toBe(
      "1920x1080",
    );
  });
});

it("a standard account pairs to play but can't host or change quality", async () => {
  open("/settings?tab=streaming", {
    accounts: [
      { id: "player", name: "Player", role: "admin" },
      { id: "kid", name: "Kid", role: "standard" },
    ],
    activeAccount: "kid",
  });
  await screen.findByText(/Only an admin can let other devices stream from this one/);
  expect((await hostingSwitch()).hasAttribute("disabled")).toBe(true);
  expect(screen.getByRole("button", { name: "Pair…" })).toBeTruthy();
});

it("a pad turns hosting on and pairs with a device in console mode", async () => {
  const { app, streaming } = open("/settings?tab=streaming", { mode: "console" });
  await hostingSwitch();
  // Down from the tabs to the switch, then A.
  const toSwitch = async () => {
    for (let i = 0; i < 20; i++) {
      if (focused()?.getAttribute("role") === "switch" || focused()?.closest('[role="switch"]'))
        return;
      app.pad.press("nav.down");
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
  };
  await toSwitch();
  app.pad.press("confirm");
  await screen.findByText("Ready. Devices on this network can pair and play.");
  expect(streaming.hostingCalls).toEqual([true]);

  // On to Pair… for the study, A opens the code.
  for (let i = 0; i < 30 && focused()?.textContent !== "Pair…"; i++) {
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  expect(focused()?.textContent).toBe("Pair…");
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: "Pair with Study" });
  await within(dialog).findByText("4831");
  streaming.finishPairing("fixture-study", "paired");
  await until(() => focused()?.textContent === "Done", "Done");
  app.pad.press("confirm");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("a device asking to pair prompts over any view, and a pad enters its code", async () => {
  const { app, streaming } = open("/library", {
    mode: "console",
    hosting: { state: "ready", version: "1", problem: null },
  });
  await screen.findByRole("heading", { name: "Library" });
  streaming.request({ id: "r2", name: "Bedroom TV", address: "192.168.1.50" });
  const prompt = await screen.findByRole(
    "dialog",
    { name: "Bedroom TV wants to play from this device" },
    { timeout: 5_000 },
  );
  // The code field has focus; the pad's keyboard types into it.
  const code = within(prompt).getByRole("textbox", { name: "Code shown on Bedroom TV" });
  await waitFor(() => expect(focused()).toBe(code));
  fireEvent.change(code, { target: { value: "1234" } });
  await until(() => {
    if (focused()?.textContent === "Pair") return true;
    app.pad.press("nav.down");
    return false;
  }, "Pair");
  app.pad.press("confirm");
  await waitFor(() =>
    expect(streaming.accepted).toEqual([{ id: "r2", code: "1234", name: "Bedroom TV" }]),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

const programs = () => [
  {
    part: "host" as const,
    label: "Game streaming from this device",
    installed: "v1.0",
    latest: "v1.1",
    update: true,
  },
  {
    part: "client" as const,
    label: "Game streaming to this device",
    installed: "v6.2.0",
    latest: "v6.3.0",
    update: true,
  },
];

it("updates streaming when asked, never during a stream, and skips a version (#266)", async () => {
  open("/settings?tab=streaming", { programs: programs(), busy: true });
  const host = (await screen.findByText("Game streaming from this device")).closest(
    "li",
  ) as HTMLElement;
  expect(host.textContent).toContain("A newer version, v1.1, is ready.");
  fireEvent.click(within(host).getByRole("button", { name: "Update" }));
  await screen.findByText("A device is streaming from this one. Update once the game ends.");

  const client = screen.getByText("Game streaming to this device").closest("li") as HTMLElement;
  fireEvent.click(within(client).getByRole("button", { name: "Update" }));
  await waitFor(() => expect(client.textContent).toContain("Up to date."));
  // Skipped: no longer offered.
  fireEvent.click(within(host).getByRole("button", { name: "Skip this version" }));
  await waitFor(() => expect(host.textContent).toContain("Up to date."));
});

it("repairs hosting and forgets every paired device (#266)", async () => {
  const { streaming } = open("/settings?tab=streaming", {
    hosting: {
      state: "needsAttention",
      version: null,
      problem: "Streaming isn't working on this device",
    },
    paired: [
      { uuid: "u1", name: "Deck" },
      { uuid: "u2", name: "Bedroom TV" },
    ],
  });
  await screen.findByText("Streaming isn't working on this device");
  fireEvent.click(screen.getByRole("button", { name: "Repair" }));
  await screen.findByText("Ready. Devices on this network can pair and play.");
  expect(streaming.repairs).toBe(1);

  await screen.findByText("Deck");
  fireEvent.click(screen.getByRole("button", { name: "Forget all paired devices…" }));
  fireEvent.click(await screen.findByRole("button", { name: "Forget them all" }));
  await waitFor(() => expect(screen.queryByText("Deck")).toBeNull());
  expect(screen.queryByText("Bedroom TV")).toBeNull();
});

it("a device paired through sync pairs with no code (#267)", async () => {
  const { streaming } = open("/settings?tab=streaming", { linked: ["fixture-study"] });
  const study = (await screen.findByText("Study")).closest("li") as HTMLElement;
  fireEvent.click(within(study).getByRole("button", { name: "Pair…" }));
  const dialog = await screen.findByRole("dialog", { name: "Pair with Study" });
  await within(dialog).findByText(
    "Study is one of your devices, so it's pairing without a code. This takes a moment.",
  );
  expect(within(dialog).queryByText("4831")).toBeNull();
  streaming.finishPairing("fixture-study", "paired");
  await within(dialog).findByText(
    "Paired. Study's games show in your library as Stream from Study.",
  );
});

it("says when controllers or a screen won't work on this host (#260)", async () => {
  open("/settings?tab=streaming", {
    hosting: {
      state: "ready",
      version: "1",
      problem: null,
      warning: "Controllers on other devices won't work here until Repair sets them up.",
    },
  });
  expect(
    await screen.findByText(
      "Controllers on other devices won't work here until Repair sets them up.",
    ),
  ).toBeTruthy();
  expect(screen.getByRole("button", { name: "Repair" })).toBeTruthy();
});
