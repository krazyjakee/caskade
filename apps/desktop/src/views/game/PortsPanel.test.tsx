import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { createFixtureApi } from "../../api/fixtures";
import { createInstallFixtureApi } from "../../api/installFixtures";
import { PortsProvider } from "../../api/ports";
import { createPortsFixtureApi } from "../../api/portsFixtures";
import { createLibraryQueryClient, LibraryProvider } from "../../api/provider";
import { ActivityProvider } from "../activity/ActivityProvider";
import { PortsPanel } from "./PortsPanel";

afterEach(cleanup);

function renderPanel(ports = createPortsFixtureApi()) {
  render(
    <LibraryProvider api={createFixtureApi()} client={createLibraryQueryClient()}>
      <ActivityProvider api={createInstallFixtureApi([])}>
        <PortsProvider api={ports}>
          <PortsPanel gameId="fixture-1" />
        </PortsProvider>
      </ActivityProvider>
    </LibraryProvider>,
  );
  return ports;
}

it("lists the game's ports with what they need and credits the plans", async () => {
  renderPanel();
  await screen.findByRole("heading", { name: "Ports" });
  const rows = screen.getAllByRole("listitem").filter((row) => row.dataset.part === "port");
  expect(rows).toHaveLength(2);
  const zelda = rows[0] as HTMLElement;
  expect(within(zelda).getByRole("link", { name: /Zelda64Recomp\/Zelda64Recomp/ })).toBeTruthy();
  expect(within(zelda).getByText("The same game on IGDB")).toBeTruthy();
  expect(within(zelda).getByText(/runs on Linux, macOS, Windows/)).toBeTruthy();
  expect(within(zelda).getByText(/in your library/)).toBeTruthy();
  expect(within(rows[1] as HTMLElement).getByText("No release for this system")).toBeTruthy();
  expect(screen.getByRole("link", { name: /Install plans from Decomp Buddy/ })).toBeTruthy();
});

it("asks before installing, then installs into the game", async () => {
  const ports = renderPanel();
  fireEvent.click(await screen.findByRole("button", { name: "Install" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/runs code from that repository/)).toBeTruthy();
  expect(within(dialog).getByText(/checked against the checksum/)).toBeTruthy();
  expect(within(dialog).getByText(/Only the North American/)).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
  await waitFor(() => expect(ports.installed).toEqual(["github:zelda64recomp/zelda64recomp"]));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await screen.findByText("Installed");
});

it("shows nothing when the game has no ports", async () => {
  const ports = createPortsFixtureApi([]);
  renderPanel(ports);
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(screen.queryByRole("heading", { name: "Ports" })).toBeNull();
});
