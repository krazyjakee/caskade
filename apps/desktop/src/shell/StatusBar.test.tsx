import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AppStatus, Snapshot } from "../api/bindings";
import type { JobSnapshot } from "../api/install";

const state = vi.hoisted(() => ({
  jobs: [] as JobSnapshot[],
  listener: undefined as ((event: { payload: Snapshot }) => void) | undefined,
  status: vi.fn<() => Promise<AppStatus>>(),
  stop: vi.fn(),
}));

vi.mock("../api/bindings", () => ({
  commands: { backgroundStatus: state.status },
  events: {
    backgroundChanged: {
      listen: vi.fn(async (listener) => {
        state.listener = listener;
        return state.stop;
      }),
    },
  },
}));
vi.mock("../views/activity/ActivityProvider", () => ({
  useJobs: () => ({ data: state.jobs }),
}));
// Rendered and tested on its own: it needs the router and the query client.
vi.mock("./AppUpdateLink", () => ({ AppUpdateLink: () => null }));
vi.mock("./SyncStatusLink", () => ({ SyncStatusLink: () => null }));

import { events } from "../api/bindings";
import { StatusBar } from "./StatusBar";
import { inDragRegion } from "./useWindowDrag";

beforeEach(() => {
  vi.clearAllMocks();
  state.jobs = [];
  state.listener = undefined;
  state.status.mockResolvedValue({ version: "1.2.3", background: { revision: 1, tasks: [] } });
  vi.stubGlobal("isTauri", true);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it("keeps the version visible while activity starts, queues and finishes", async () => {
  const { container } = render(<StatusBar />);
  await screen.findByText("Caskade v1.2.3");
  expect(screen.getByText("Ready")).toBeTruthy();
  const spinner = container.querySelector('[data-part="spinner"]');
  expect(spinner?.classList.contains("hidden")).toBe(true);
  act(() =>
    state.listener?.({
      payload: {
        revision: 2,
        tasks: [
          { id: 1, label: "Scanning library", queued: false },
          { id: 2, label: "Preparing artwork", queued: true },
        ],
      },
    }),
  );
  expect(screen.getByText("Scanning library · 1 queued")).toBeTruthy();
  expect(spinner?.classList.contains("animate-worker")).toBe(true);
  expect(screen.getByText("Caskade v1.2.3")).toBeTruthy();
  act(() => state.listener?.({ payload: { revision: 3, tasks: [] } }));
  expect(screen.getByText("Ready")).toBeTruthy();
  expect(spinner?.classList.contains("hidden")).toBe(true);
  expect(screen.getByText("Caskade v1.2.3")).toBeTruthy();
});

it("leaves the IGDB and Hasheous credits to Settings → About", async () => {
  const { container } = render(<StatusBar />);
  await screen.findByText("Caskade v1.2.3");
  expect(screen.queryByRole("link")).toBeNull();
  expect(container.textContent).not.toMatch(/igdb|hasheous/i);
});

it("does not let a late initial snapshot or older event hide newer work", async () => {
  let resolve: ((value: AppStatus) => void) | undefined;
  state.status.mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  render(<StatusBar />);
  await waitFor(() => expect(state.status).toHaveBeenCalled());
  act(() =>
    state.listener?.({
      payload: {
        revision: 5,
        tasks: [{ id: 2, label: "Updating metadata", queued: false }],
      },
    }),
  );
  await act(async () => resolve?.({ version: "2.0.0", background: { revision: 1, tasks: [] } }));
  act(() => state.listener?.({ payload: { revision: 3, tasks: [] } }));
  expect(screen.getByText("Updating metadata")).toBeTruthy();
  expect(screen.getByText("Caskade v2.0.0")).toBeTruthy();
});

it("shows unavailable status on IPC failure instead of claiming the app is idle", async () => {
  state.status.mockRejectedValue(new Error("disconnected"));
  render(<StatusBar />);
  await screen.findByText("Background status unavailable");
  expect(screen.getByText(/^Caskade v/)).toBeTruthy();
  act(() => state.listener?.({ payload: { revision: 2, tasks: [] } }));
  expect(screen.getByText("Ready")).toBeTruthy();
});

it("shows installs between worker stages and distinguishes a question from active work", async () => {
  state.jobs = [
    { id: "install", status: "running", title: "Example game", phase: "Installing" },
  ] as JobSnapshot[];
  const { rerender, container } = render(<StatusBar />);
  expect(screen.getByText("Installing: Example game")).toBeTruthy();
  state.jobs = [{ id: "install", status: "needs_user" }] as JobSnapshot[];
  await screen.findByText("Caskade v1.2.3");
  rerender(<StatusBar />);
  expect(screen.getByText("1 task needs attention")).toBeTruthy();
  expect(container.querySelector('[data-part="spinner"]')?.classList.contains("hidden")).toBe(true);
});

it("cleans up a listener that resolves after unmount", async () => {
  let resolve: ((stop: () => void) => void) | undefined;
  vi.mocked(events.backgroundChanged.listen).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const { unmount } = render(<StatusBar />);
  unmount();
  await act(async () => resolve?.(state.stop));
  expect(state.stop).toHaveBeenCalledOnce();
  expect(state.status).not.toHaveBeenCalled();
});

it("renders in browser preview without IPC", () => {
  vi.unstubAllGlobals();
  render(<StatusBar />);
  expect(screen.getByText("Ready")).toBeTruthy();
  expect(screen.getByText(/^Caskade v/)).toBeTruthy();
  expect(state.status).not.toHaveBeenCalled();
});

it("moves the window from the bar", async () => {
  render(<StatusBar />);
  await screen.findByText("Caskade v1.2.3");
  const at = (element: Element) => {
    const path: EventTarget[] = [];
    for (let node: Element | null = element; node; node = node.parentElement) path.push(node);
    return inDragRegion(path);
  };
  expect(at(screen.getByText("Ready"))).toBe(true);
  expect(at(screen.getByText("Caskade v1.2.3"))).toBe(true);
});
