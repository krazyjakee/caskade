import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it, vi } from "vitest";
import { events } from "./bindings";
import { nativeInstallRequestApi } from "./installRequestsNative";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});
const request = {
  requestId: "request-1",
  kind: "url" as const,
  value: "https://example.invalid/game.zip",
};
it("uses the shared pending queue; ingress and dismiss never invoke install or confirmation", async () => {
  vi.stubGlobal("isTauri", true);
  const invoke = vi.fn((command: string) =>
    command === "pending_install_requests"
      ? [request]
      : command === "request_install_input"
        ? request
        : command === "confirm_install_request"
          ? { id: "job-1" }
          : null,
  );
  mockIPC(invoke);
  expect(await nativeInstallRequestApi.pending()).toEqual([request]);
  expect(await nativeInstallRequestApi.request(request.value)).toEqual(request);
  await nativeInstallRequestApi.dismiss(request.requestId);
  expect(invoke.mock.calls.map(([command]) => command)).toEqual([
    "pending_install_requests",
    "request_install_input",
    "dismiss_install_request",
  ]);
  expect(await nativeInstallRequestApi.confirm(request.requestId, "chosen-root")).toBe("job-1");
  expect(invoke).toHaveBeenLastCalledWith("confirm_install_request", {
    requestId: "request-1",
    rootId: "chosen-root",
  });
});
it("listens for typed external requests and cleans up", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => null, { shouldMockEvents: true });
  const listener = vi.fn();
  const stop = await nativeInstallRequestApi.subscribe(listener);
  await events.installRequested.emit(request);
  expect(listener).toHaveBeenCalledExactlyOnceWith(request);
  stop();
  await events.installRequested.emit(request);
  expect(listener).toHaveBeenCalledOnce();
});
