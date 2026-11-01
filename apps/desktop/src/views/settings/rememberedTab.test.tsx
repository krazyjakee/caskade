import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { renderApp } from "../../test/harness";
import { rememberedSettingsTab, resolveSettingsTab } from "./rememberedTab";

afterEach(cleanup);

it("opens on General, then comes back to the tab Settings was left on", async () => {
  const app = renderApp("/settings");
  await screen.findByRole("heading", { name: "Settings" });
  expect(app.router.state.location.search).toMatchObject({ tab: "general" });

  fireEvent.click(screen.getByRole("tab", { name: "Storage" }));
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "storage" }));

  // Away, and back through the sidebar.
  await app.router.navigate({ to: "/" });
  fireEvent.click(await screen.findByRole("link", { name: "Settings" }));
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "storage" }));
});

it("keeps the remembered tab across restarts and ignores what it can't read", () => {
  expect(rememberedSettingsTab()).toBe("general");
  localStorage.setItem("caskade.settings.tab", "emulators");
  expect(rememberedSettingsTab()).toBe("emulators");
  // The URL wins over the remembered tab.
  expect(resolveSettingsTab({ tab: "plugins" })).toBe("plugins");
  expect(resolveSettingsTab({ tab: "nowhere" })).toBe("emulators");
  localStorage.setItem("caskade.settings.tab", "nowhere");
  expect(rememberedSettingsTab()).toBe("general");
});
