import { QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createLibraryQueryClient } from "../../api/provider";
import { InstallIngress, type InstallRequestApi, installationText } from "./InstallIngress";

afterEach(cleanup);
const request = {
  requestId: "request-1",
  kind: "url" as const,
  value: "https://example.invalid/game.zip",
};
function api(): InstallRequestApi {
  return {
    pending: vi.fn(async () => []),
    request: vi.fn(async () => request),
    requestDrop: vi.fn(async () => request),
    confirm: vi.fn(async () => "job-1"),
    dismiss: vi.fn(async () => {}),
    subscribe: vi.fn(async () => () => {}),
  };
}
it("accepts one HTTP URL or absolute path and leaves arbitrary clipboard text alone", () => {
  for (const text of [
    "hello",
    "relative/setup.exe",
    "javascript:alert(1)",
    "https://user:password@example.invalid/x",
    "/one\n/two",
    "https://example.invalid/one\nhttps://example.invalid/two",
    "magnet:?dn=no-hash",
    "magnet:?dn=Game#&xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
    "magnet:?dn=xt%3Durn%3Abtih%3A0123456789abcdef0123456789abcdef01234567",
  ])
    expect(installationText(text)).toBeNull();
  for (const text of [
    request.value,
    "/fixture/Games/setup.zip",
    "C:\\Games\\setup.exe",
    "\\\\server\\share\\game.7z",
    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Game",
    "magnet:?dn=Game&xt=urn%3Abtih%3A0123456789abcdef0123456789abcdef01234567",
    "magnet:?xt=urn:btmh:unsupported&xt=urn%3Abtih%3A0123456789abcdef0123456789abcdef01234567",
  ])
    expect(installationText(text)).toBe(text);
});
it.each([
  request.value,
  "magnet:?dn=Game&xt=urn%3Abtih%3A0123456789abcdef0123456789abcdef01234567",
])("pasting %s outside a text field waits for explicit confirmation", async (value) => {
  const backend = api();
  vi.mocked(backend.request).mockResolvedValue({ ...request, value });
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <InstallIngress api={backend} />
    </QueryClientProvider>,
  );
  fireEvent.paste(window, { clipboardData: { getData: () => value } });
  await screen.findByRole("dialog");
  expect(backend.request).toHaveBeenCalledExactlyOnceWith(value);
  expect(backend.confirm).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Confirm installation" }));
  await waitFor(() => expect(backend.confirm).toHaveBeenCalledExactlyOnceWith("request-1"));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});
it("does not intercept text-field pastes or turn browser File.name into an install path", () => {
  const backend = api();
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <input aria-label="URL text" />
      <InstallIngress api={backend} />
    </QueryClientProvider>,
  );
  fireEvent.paste(screen.getByRole("textbox"), { clipboardData: { getData: () => request.value } });
  fireEvent.drop(window, {
    dataTransfer: { files: [new File(["fixture"], "setup.exe")], getData: () => request.value },
  });
  expect(backend.request).not.toHaveBeenCalled();
});
it("recovers startup requests after registering the listener and dismisses without a download", async () => {
  const backend = api();
  vi.mocked(backend.pending).mockResolvedValue([request]);
  render(
    <QueryClientProvider client={createLibraryQueryClient()}>
      <InstallIngress api={backend} />
    </QueryClientProvider>,
  );
  const dialog = await screen.findByRole("dialog");
  expect(vi.mocked(backend.subscribe).mock.invocationCallOrder[0]).toBeLessThan(
    vi.mocked(backend.pending).mock.invocationCallOrder[0] ?? Infinity,
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(backend.dismiss).toHaveBeenCalledExactlyOnceWith(request.requestId));
  expect(backend.confirm).not.toHaveBeenCalled();
});
