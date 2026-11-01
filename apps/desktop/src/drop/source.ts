import { isTauri } from "@tauri-apps/api/core";

/** A point in CSS pixels from the webview's top left. */
export interface DropPoint {
  x: number;
  y: number;
}

/** Files dragged from the desktop over the window. Only a drop carries
 * paths: they come from the OS, never from the page. */
export type NativeDrag =
  | { type: "enter" | "over"; position: DropPoint }
  | { type: "drop"; paths: string[]; position: DropPoint }
  | { type: "leave" };

export type DropSource = (listener: (event: NativeDrag) => void) => Promise<() => void>;

/** A link to the app's own pages or assets: the page's origin (the dev
 * server, `tauri://localhost`, `http://tauri.localhost`) or one of the
 * app's schemes (`caskade-asset://localhost`, or on Windows
 * `http://caskade-asset.localhost`). Dragging a card, link or image inside
 * the window reports one of these as a dropped path. */
export function isAppUrl(path: string) {
  let url: URL;
  try {
    url = new URL(path);
  } catch {
    return false;
  }
  if (url.origin !== "null" && url.origin === window.location.origin) return true;
  const scheme = url.protocol.slice(0, -1);
  const app = (name: string) => name === "tauri" || name.startsWith("caskade-");
  if (app(scheme)) return true;
  return (
    (scheme === "http" || scheme === "https") &&
    url.hostname.endsWith(".localhost") &&
    app(url.hostname.slice(0, -".localhost".length))
  );
}

/** Tauri's native drag and drop. The browser preview has none. Drags of
 * the app's own links ({@link isAppUrl}) are not drops: a drag of only
 * those is ignored, and they are left out of a drop. */
export const nativeDropSource: DropSource = async (listener) => {
  if (!isTauri()) return () => {};
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  let ignored = false;
  return getCurrentWebview().onDragDropEvent(({ payload }) => {
    if (payload.type === "enter")
      ignored = payload.paths.length > 0 && payload.paths.every(isAppUrl);
    const skip = ignored;
    if (payload.type === "leave" || payload.type === "drop") ignored = false;
    if (skip) return;
    if (payload.type === "leave") {
      listener({ type: "leave" });
      return;
    }
    // Tauri calls the position physical, but only WebView2 (Windows) reports
    // device pixels; WebKitGTK and WKWebView report CSS pixels.
    const ratio = /Windows/.test(navigator.userAgent) ? window.devicePixelRatio || 1 : 1;
    const position = { x: payload.position.x / ratio, y: payload.position.y / ratio };
    listener(
      payload.type === "drop"
        ? { type: "drop", paths: payload.paths.filter((p) => !isAppUrl(p)), position }
        : { type: payload.type, position },
    );
  });
};
