import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  type RootSummary,
  rootBlockReason,
  type StorageApi,
  skipRootPicker,
} from "../../api/storage";
import { InstallRootPicker } from "./InstallRootPicker";

afterEach(cleanup);

function root(id: string, freeBytes = 10 * 1024 ** 3): RootSummary {
  return {
    root: { id, path: `/games/${id}`, label: id, isDefault: id === "fast" },
    status: "online",
    folderTemplate: "{title}",
    games: [],
    gamesBytes: 0,
    otherBytes: 0,
    capabilities: {
      writable: true,
      executable: true,
      symlinks: true,
      caseSensitive: true,
      freeBytes,
      totalBytes: 20 * 1024 ** 3,
      filesystem: "ext4",
      warnings: [],
    },
  };
}
function api(roots: RootSummary[], alwaysUseDefault = false): StorageApi {
  return {
    list: vi.fn(async () => roots),
    preferences: vi.fn(async () => ({ alwaysUseDefault })),
    add: vi.fn(),
    edit: vi.fn(),
    setDefault: vi.fn(),
    remove: vi.fn(),
    moveAndRemove: vi.fn(),
    chooseDirectory: vi.fn(),
    move: vi.fn(),
    uninstall: vi.fn(),
  };
}
function renderPicker(transport: StorageApi, onSelect = vi.fn(), estimatedBytes = 2 * 1024 ** 3) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <StrictMode>
      <QueryClientProvider client={client}>
        <InstallRootPicker
          api={transport}
          estimatedBytes={estimatedBytes}
          onSelect={onSelect}
          onCancel={vi.fn()}
        />
      </QueryClientProvider>
    </StrictMode>,
  );
  return onSelect;
}
describe("install root picker", () => {
  it("compares space, preselects default, and submits only the chosen root id", async () => {
    const onSelect = renderPicker(api([root("fast"), root("full", 1)]));
    expect(await screen.findByText("Install location")).toBeTruthy();
    expect(screen.getByText(/Estimated space: 2.0 GiB/)).toBeTruthy();
    const full = screen.getByRole("radio", { name: /full/ });
    expect((full as HTMLInputElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Install here" }));
    expect(onSelect).toHaveBeenCalledWith("fast");
  });
  it("skips a single eligible root once, including StrictMode", async () => {
    const select = renderPicker(api([root("fast")]));
    await waitFor(() => expect(select).toHaveBeenCalledTimes(1));
    expect(select).toHaveBeenCalledWith("fast");
  });
  it("honours always-default but never skips a full or offline default", async () => {
    const select = renderPicker(api([root("fast"), root("slow")], true));
    await waitFor(() => expect(select).toHaveBeenCalledWith("fast"));
    const full = root("fast", 1);
    expect(skipRootPicker([full, root("slow")], true, { estimatedBytes: 2 })).toBeNull();
    full.status = "offline";
    expect(skipRootPicker([full], true, {})).toBeNull();
    expect(rootBlockReason(full, {})).toBe("Location unavailable");
  });
  it("blocks symlink/noexec/read-only targets when required", () => {
    const candidate = root("fast");
    candidate.capabilities.symlinks = false;
    expect(rootBlockReason(candidate, { symlinks: true })).toMatch(/symbolic links/);
    candidate.capabilities.executable = false;
    expect(rootBlockReason(candidate, { executable: true })).toMatch(/executables/);
    candidate.status = "read_only";
    expect(rootBlockReason(candidate, {})).toMatch(/Read-only/);
  });
});
