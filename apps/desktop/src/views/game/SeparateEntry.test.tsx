import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { type GameIgdbApi, GameIgdbProvider } from "../../api/igdb";
import { SeparateEntry } from "./SeparateEntry";

afterEach(cleanup);

function show(separate: GameIgdbApi["separate"], isRunning = false) {
  const api = { separate } as GameIgdbApi;
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <GameIgdbProvider api={api}>
        <SeparateEntry entryId="genesis" isRunning={isRunning} />
      </GameIgdbProvider>
    </QueryClientProvider>,
  );
  return invalidate;
}

it("moves the entry into a game of its own and refreshes the library", async () => {
  const separate = vi.fn(async () => "apart");
  const invalidate = show(separate);
  fireEvent.click(screen.getByRole("button", { name: "Separate" }));
  await waitFor(() => expect(invalidate).toHaveBeenCalled());
  expect(separate).toHaveBeenCalledWith("genesis");
});

it("shows why it failed, and waits for a running game", async () => {
  show(async () => {
    throw new Error("entry genesis is its game's only entry");
  });
  fireEvent.click(screen.getByRole("button", { name: "Separate" }));
  expect((await screen.findByRole("alert")).textContent).toContain("only entry");
  cleanup();
  show(vi.fn(), true);
  expect(screen.getByRole("button", { name: "Separate" }).hasAttribute("disabled")).toBe(true);
});
