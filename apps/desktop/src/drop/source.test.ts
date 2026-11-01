import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { afterEach, expect, it, vi } from "vitest";
import { isAppUrl, type NativeDrag, nativeDropSource } from "./source";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});

async function received(userAgent: string) {
  vi.stubGlobal("isTauri", true);
  vi.stubGlobal("devicePixelRatio", 2);
  vi.stubGlobal("navigator", { userAgent });
  mockWindows("main");
  mockIPC(() => null, { shouldMockEvents: true });
  const events: NativeDrag[] = [];
  const stop = await nativeDropSource((event) => events.push(event));
  await emit("tauri://drag-over", { position: { x: 200, y: 100 } });
  await emit("tauri://drag-drop", { paths: ["/games/Celeste"], position: { x: 200, y: 100 } });
  await emit("tauri://drag-leave", null);
  stop();
  await emit("tauri://drag-leave", null);
  return events;
}

it("passes native drags on in CSS pixels; only WebView2 reports device pixels", async () => {
  expect(await received("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15")).toEqual([
    { type: "over", position: { x: 200, y: 100 } },
    { type: "drop", paths: ["/games/Celeste"], position: { x: 200, y: 100 } },
    { type: "leave" },
  ]);
  expect(await received("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/130.0")).toEqual([
    { type: "over", position: { x: 100, y: 50 } },
    { type: "drop", paths: ["/games/Celeste"], position: { x: 100, y: 50 } },
    { type: "leave" },
  ]);
});

it("knows the app's own links", () => {
  const page = `${window.location.origin}/library?filter=installed`;
  for (const url of [
    page,
    "tauri://localhost/game/1",
    "http://tauri.localhost/game/1",
    "caskade-asset://localhost/%2Fcache%2Fart.png",
    "http://caskade-asset.localhost/%2Fcache%2Fart.png",
    "caskade-theme://localhost/caskade.default/theme.css",
  ])
    expect(isAppUrl(url), url).toBe(true);
  for (const path of [
    "/games/Celeste",
    "C:\\Games\\Celeste",
    "https://example.com/game.zip",
    "http://localhost.example.com/game.zip",
    "http://other.localhost/game.zip",
  ])
    expect(isAppUrl(path), path).toBe(false);
});

it("ignores drags of the app's own links and leaves them out of drops", async () => {
  vi.stubGlobal("isTauri", true);
  mockWindows("main");
  mockIPC(() => null, { shouldMockEvents: true });
  const events: NativeDrag[] = [];
  const stop = await nativeDropSource((event) => events.push(event));
  const position = { x: 10, y: 10 };
  const page = `${window.location.origin}/game/1`;
  await emit("tauri://drag-enter", { paths: [page], position });
  await emit("tauri://drag-over", { position });
  await emit("tauri://drag-drop", { paths: [page], position });
  expect(events).toEqual([]);
  await emit("tauri://drag-enter", { paths: [page, "/games/Celeste"], position });
  await emit("tauri://drag-drop", { paths: [page, "/games/Celeste"], position });
  stop();
  expect(events).toEqual([
    { type: "enter", position },
    { type: "drop", paths: ["/games/Celeste"], position },
  ]);
});
