import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  type TorrentLinks,
  type TorrentLinksApi,
  TorrentLinksProvider,
} from "../../api/torrentLinks";
import { TorrentLinksSettings } from "./TorrentLinksSettings";

afterEach(cleanup);

function fixture(initial: TorrentLinks) {
  let status = initial;
  const api: TorrentLinksApi = {
    status: async () => status,
    set: vi.fn(async (open: boolean) => {
      status =
        status.support === "direct"
          ? { ...status, registered: open, magnet: open, torrentFiles: open }
          : { ...status, registered: open };
      return status;
    }),
    choose: vi.fn(async () => {}),
  };
  render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
        })
      }
    >
      <TorrentLinksProvider api={api}>
        <TorrentLinksSettings />
      </TorrentLinksProvider>
    </QueryClientProvider>,
  );
  return api;
}

const off = { registered: false, magnet: false, torrentFiles: false };

it("makes Caskade the app for magnet links and .torrent files on Linux, and hands them back", async () => {
  const api = fixture({ support: "direct", ...off });
  expect(screen.getByText(/Paste a magnet link anywhere in Caskade/)).toBeTruthy();
  const open = await screen.findByRole("switch", { name: "Open them with Caskade" });
  expect((open as HTMLInputElement).checked).toBe(false);
  fireEvent.click(open);
  await waitFor(() => expect((open as HTMLInputElement).checked).toBe(true));
  expect(api.set).toHaveBeenLastCalledWith(true);
  fireEvent.click(open);
  await waitFor(() => expect((open as HTMLInputElement).checked).toBe(false));
  expect(api.set).toHaveBeenLastCalledWith(false);
});

it("offers Caskade on Windows and opens Default apps until the user chooses it", async () => {
  const api = fixture({ support: "chooser", ...off });
  const offer = await screen.findByRole("switch", { name: "Offer Caskade for them" });
  expect(screen.queryByRole("button", { name: "Open Default apps" })).toBeNull();
  fireEvent.click(offer);
  fireEvent.click(await screen.findByRole("button", { name: "Open Default apps" }));
  await waitFor(() => expect(api.choose).toHaveBeenCalled());
});

it("shows a failure and leaves the switch as it was", async () => {
  const api = fixture({ support: "direct", ...off });
  api.set = vi.fn(async () => {
    throw new Error("xdg-mime failed");
  });
  const open = await screen.findByRole("switch", { name: "Open them with Caskade" });
  fireEvent.click(open);
  expect((await screen.findByRole("alert")).textContent).toContain("xdg-mime failed");
  expect((open as HTMLInputElement).checked).toBe(false);
});

it("says when the system can't send them to Caskade", async () => {
  fixture({ support: "unsupported", ...off });
  expect(await screen.findByText(/can't send magnet links/)).toBeTruthy();
  expect(screen.queryByRole("switch")).toBeNull();
});
