import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { AppUpdateApi, AppUpdateStatus } from "../../api/appUpdate";
import { AppUpdateProvider } from "../../api/appUpdate";
import { createSettingsFixture } from "../../test/settingsFixture";
import { AppUpdateSection } from "./AppUpdateSection";
import { SettingsProvider } from "./useSettings";

afterEach(cleanup);

const idle: AppUpdateStatus = {
  currentVersion: "0.1.0",
  unavailable: null,
  phase: "idle",
  release: null,
  checkedAt: null,
  downloaded: 0,
  total: null,
  error: null,
};
const release = { version: "0.2.0", notes: "Faster library scans.", date: null };

/** A core whose check finds 0.2.0 and whose install reports through events. */
function fakeCore(initial: AppUpdateStatus = idle) {
  let status = initial;
  const listeners = new Set<(status: AppUpdateStatus) => void>();
  const calls: string[] = [];
  const emit = (next: AppUpdateStatus) => {
    status = next;
    for (const listener of listeners) listener(next);
  };
  const api: AppUpdateApi = {
    status: async () => status,
    check: async () => {
      calls.push("check");
      status = { ...status, phase: "available", release, checkedAt: Date.now() };
      return status;
    },
    install: async () => {
      calls.push("install");
      status = { ...status, phase: "downloading" };
      return status;
    },
    restart: async () => {
      calls.push("restart");
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  return { api, calls, emit, current: () => status };
}

function renderSection(api: AppUpdateApi) {
  const settings = createSettingsFixture();
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SettingsProvider api={settings.api}>
        <AppUpdateProvider api={api}>
          <AppUpdateSection />
        </AppUpdateProvider>
      </SettingsProvider>
    </QueryClientProvider>,
  );
  return settings;
}

it("checks, installs with progress and offers the restart", async () => {
  const core = fakeCore();
  renderSection(core.api);
  expect(await screen.findByText("Caskade v0.1.0")).toBeTruthy();
  expect(screen.getByText("Not checked yet.")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Check now" }));
  expect(await screen.findByText("Version 0.2.0 is available.")).toBeTruthy();
  expect(screen.getByText("Faster library scans.")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: /Download and install/ }));
  await waitFor(() => expect(core.calls).toEqual(["check", "install"]));
  await screen.findByText("Downloading version 0.2.0…");
  act(() => core.emit({ ...core.current(), downloaded: 25, total: 100 }));
  await waitFor(() =>
    expect(
      screen.getByRole("progressbar", { name: "Update download" }).getAttribute("aria-valuenow"),
    ).toBe("25"),
  );
  act(() => core.emit({ ...core.current(), phase: "ready" }));
  expect(
    await screen.findByText("Version 0.2.0 is installed. Restart Caskade to use it."),
  ).toBeTruthy();
  expect(screen.queryByRole("progressbar")).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: /Restart now/ }));
  await waitFor(() => expect(core.calls).toEqual(["check", "install", "restart"]));
});

it("offers the release again after a failed download", async () => {
  const core = fakeCore({
    ...idle,
    phase: "failed",
    release,
    error: "Couldn't download the update: connection reset",
  });
  renderSection(core.api);
  expect(await screen.findByText("Couldn't download the update: connection reset")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /Try again/ }));
  await waitFor(() => expect(core.calls).toEqual(["install"]));
});

it("says why a copy can't update itself and hides the automatic checks", async () => {
  const core = fakeCore({
    ...idle,
    unavailable: "Development builds don't update themselves.",
  });
  renderSection(core.api);
  expect(await screen.findByText(/Development builds don't update themselves/)).toBeTruthy();
  expect(screen.getByRole("button", { name: "Check now" }).hasAttribute("disabled")).toBe(true);
  expect(screen.queryByText("Check automatically")).toBeNull();
});

it("turns the automatic checks off", async () => {
  const settings = renderSection(fakeCore().api);
  fireEvent.click(await screen.findByRole("switch", { name: /Check automatically/ }));
  await waitFor(() => expect(settings.values["core.appUpdates"]).toEqual({ check: false }));
});
