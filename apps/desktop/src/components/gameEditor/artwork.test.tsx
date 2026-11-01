import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import { DropProvider } from "../../drop/DropProvider";
import { fakeDrops } from "../../drop/testing";
import { renderApp } from "../../test/harness";

afterEach(cleanup);

function games() {
  const list = createFixtureGames(2);
  const [first] = list;
  if (first) {
    first.coverGrid = "asset://cover-a";
    first.coverList = "asset://cover-a";
  }
  return list;
}

async function openArtwork() {
  fireEvent.click(await screen.findByRole("button", { name: "Edit details" }));
  const dialog = await screen.findByRole("dialog", { name: "Edit Fixture Game 0001" });
  fireEvent.click(within(dialog).getByRole("tab", { name: "Artwork" }));
  const cover = await within(dialog).findByRole("region", { name: "Cover" });
  return { dialog, cover };
}

it("hides, imports by drop, keeps through a refresh, resets and chooses a cover", async () => {
  let target: () => Element | null = () => null;
  const drops = fakeDrops(() => target());
  const app = renderApp("/games/fixture-0", {
    games: games(),
    wrap: (children) => (
      <DropProvider source={drops.source} hitTest={drops.hitTest}>
        {children}
      </DropProvider>
    ),
  });
  const { cover } = await openArtwork();
  expect(cover.dataset.choice).toBe("automatic");
  expect(within(cover).getByRole("figure").querySelector("img")?.getAttribute("src")).toBe(
    "asset://cover-a",
  );

  fireEvent.click(within(cover).getByRole("button", { name: "Show none" }));
  await waitFor(() => expect(cover.dataset.choice).toBe("removed"));
  expect((await app.library.getGame("fixture-0"))?.coverGrid).toBeNull();

  // A dropped image becomes the cover; one that isn't an image changes nothing.
  target = () => cover;
  drops.drop("/pictures/box-art.png");
  await waitFor(() => expect(cover.dataset.choice).toBe("chosen"));
  drops.drop("/pictures/notes.txt");
  expect((await within(cover).findByRole("alert")).textContent).toContain("isn't a PNG");
  expect(cover.dataset.choice).toBe("chosen");

  // A refresh brings a new automatic cover: the user's stays.
  act(() =>
    app.library.edits.addAutomaticArtwork("fixture-0", "cover", {
      id: "igdb-cover",
      source: "igdb:co1",
      url: "asset://cover-b",
      width: 600,
      height: 900,
    }),
  );
  await waitFor(() =>
    expect(within(cover).getAllByRole("button", { name: /^Use Cover/ })).toHaveLength(2),
  );
  expect(cover.dataset.choice).toBe("chosen");

  fireEvent.click(within(cover).getByRole("button", { name: "Reset Cover to automatic" }));
  await waitFor(() => expect(cover.dataset.choice).toBe("automatic"));
  expect((await app.library.getGame("fixture-0"))?.coverGrid).toBe("asset://cover-b");

  fireEvent.click(within(cover).getByRole("button", { name: "Use Cover 2 from fixture" }));
  await waitFor(() => expect(cover.dataset.choice).toBe("chosen"));
  expect((await app.library.getGame("fixture-0"))?.coverGrid).toBe("asset://cover-a");
});

it("chooses artwork by pad in console mode", async () => {
  const app = renderApp("/games/fixture-0", { games: games(), mode: "console" });
  const edit = await screen.findByRole("button", { name: "Edit details" });
  act(() => edit.focus());
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: "Edit Fixture Game 0001" });
  app.pad.press("tab.next");
  const cover = await within(dialog).findByRole("region", { name: "Cover" });
  const none = within(cover).getByRole("button", { name: "Show none" });
  act(() => none.focus());
  app.pad.press("confirm");
  await waitFor(() => expect(cover.dataset.choice).toBe("removed"));
  const reset = within(cover).getByRole("button", { name: "Reset Cover to automatic" });
  act(() => reset.focus());
  app.pad.press("confirm");
  await waitFor(() => expect(cover.dataset.choice).toBe("automatic"));
});
