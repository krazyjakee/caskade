/** Appearance's theme picker while it has no themes to show (#65): loading,
 * a failed read or rescan, and none installed. */
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { type InstalledTheme, type ThemesApi, ThemesApiContext } from "../../../themes/api";
import { ThemePicker } from "./ThemePicker";

afterEach(cleanup);

const nexus: InstalledTheme = {
  id: "caskade.default",
  name: "Nexus",
  version: "1.0.0",
  modes: ["dark"],
  previews: [],
  source: "builtin",
  problems: [],
};

function show(list: ThemesApi["list"], reload: ThemesApi["reload"] = list) {
  const themes: ThemesApi = {
    list,
    reload,
    resolve: async () => {
      throw new Error("not used");
    },
    onReloaded: async () => () => {},
  };
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ThemesApiContext.Provider value={themes}>
        <ThemePicker value={nexus.id} onChange={() => {}} />
      </ThemesApiContext.Provider>
    </QueryClientProvider>,
  );
}

const alertWith = async (text: string) =>
  (await screen.findByText(text)).closest('[role="alert"]') as HTMLElement;

it("says it's loading until the themes arrive", async () => {
  let finish = (_: InstalledTheme[]) => {};
  show(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  expect((await screen.findByRole("status")).textContent).toBe("Loading…");
  expect(screen.queryByRole("radiogroup")).toBeNull();
  finish([nexus]);
  expect(await screen.findByRole("radio", { name: /Nexus/ })).toBeTruthy();
  expect(screen.queryByRole("status")).toBeNull();
});

it("a failed read says so in words, with Try again", async () => {
  let failing = true;
  show(async () => {
    if (failing) throw new Error("the themes folder can't be read");
    return [nexus];
  });
  const notice = await alertWith("the themes folder can't be read");
  expect(notice.textContent).toContain("Themes couldn't be loaded");
  expect(notice.textContent).not.toContain("Error:");
  failing = false;
  fireEvent.click(within(notice).getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("radio", { name: /Nexus/ })).toBeTruthy();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("says when there are no themes, and a rescan can find some", async () => {
  show(
    async () => [],
    async () => [nexus],
  );
  expect(await screen.findByText(/No themes found/)).toBeTruthy();
  expect(screen.queryByRole("radiogroup")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Rescan themes" }));
  expect(await screen.findByRole("radio", { name: /Nexus/ })).toBeTruthy();
  expect(screen.queryByText(/No themes found/)).toBeNull();
});

it("a failed rescan keeps the themes and offers Try again", async () => {
  let failing = true;
  show(
    async () => [nexus],
    async () => {
      if (failing) throw new Error("the rescan stopped");
      return [nexus];
    },
  );
  fireEvent.click(await screen.findByRole("button", { name: "Rescan themes" }));
  const notice = await alertWith("the rescan stopped");
  expect(screen.getByRole("radio", { name: /Nexus/ })).toBeTruthy();
  failing = false;
  fireEvent.click(within(notice).getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
});
