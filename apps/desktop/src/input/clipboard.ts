/** The clipboard, for what a pad presses (#247): the on-screen keyboard's
 * Paste and the Copy buttons. A webview only allows the clipboard in answer
 * to a click or a key, so the app goes through the core; the browser
 * preview and tests use the page's own. */
import { isTauri } from "@tauri-apps/api/core";
import { commands } from "../api/bindings";

/** The clipboard's text; empty when there's none or it can't be read. */
export async function readClipboard(): Promise<string> {
  if (isTauri()) return commands.clipboardReadText();
  return (await navigator.clipboard?.readText().catch(() => "")) ?? "";
}

/** Puts `text` on the clipboard; whether it worked. */
export async function writeClipboard(text: string): Promise<boolean> {
  if (isTauri()) return (await commands.clipboardWriteText(text)).status === "ok";
  return (
    (await navigator.clipboard?.writeText(text).then(
      () => true,
      () => false,
    )) ?? false
  );
}
