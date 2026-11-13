/** Applies a theme (#139, #140): its compiled stylesheet, served by the core
 * on the `caskade-theme:` scheme, in one `<link>` that is swapped once the
 * new sheet has loaded, so the page never shows two themes or none. Without
 * the core (browser preview) the built-in default is inlined the way the
 * core compiles it. The theme's colour schemes set `<html data-color-scheme>`
 * next to the app's `data-mode`. */
import { isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";
import { commands } from "../api/bindings";
import { unwrap } from "../api/reply";
import { DEFAULT_THEME, type ResolvedTheme, type ThemesApi } from "./api";

export { DEFAULT_THEME };

/** The compiled default theme without the core: its theme.css in `cs.theme`.
 * Its tokens.json restates no tokens, so this equals the core's output. */
async function previewStylesheet(): Promise<string> {
  const { default: css } = await import("../../../../themes/default/theme.css?raw");
  return `@layer cs.theme {\n${css}}\n`;
}

function loaded(element: HTMLLinkElement | HTMLStyleElement, timeoutMs: number) {
  return new Promise<void>((resolve) => {
    element.addEventListener("load", () => resolve(), { once: true });
    element.addEventListener("error", () => resolve(), { once: true });
    setTimeout(resolve, timeoutMs);
  });
}

/** Bumped by every swap: a swap that finishes after a newer one started
 * removes its own sheet instead of the newer one. */
let latest = 0;

/** Swaps in the theme's sheet; false when a newer swap overtook it. */
async function swap(
  id: string,
  source: { href: string } | { css: string },
  timeoutMs: number,
): Promise<boolean> {
  const generation = ++latest;
  let element: HTMLLinkElement | HTMLStyleElement;
  if ("href" in source) {
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = source.href;
    element = link;
  } else {
    element = document.createElement("style");
    element.textContent = source.css;
  }
  element.dataset.csTheme = id;
  document.head.append(element);
  if (element instanceof HTMLLinkElement) await loaded(element, timeoutMs);
  if (generation !== latest) {
    element.remove();
    return false;
  }
  for (const old of document.head.querySelectorAll("[data-cs-theme]")) {
    if (old !== element) old.remove();
  }
  return true;
}

/** Swaps the active theme stylesheet; resolves once the new one applies. */
export async function applyTheme(id: string, { timeoutMs = 3000 } = {}): Promise<void> {
  if (isTauri()) {
    await swap(id, { href: await unwrap(commands.themeStylesheet(id)) }, timeoutMs);
    return;
  }
  if (id !== DEFAULT_THEME) throw new Error(`theme ${id} needs the Caskade core`);
  await swap(id, { css: await previewStylesheet() }, timeoutMs);
}

/** The scheme to show: the OS preference when the theme has it, otherwise
 * the theme's default (its first mode). */
export function colorScheme(modes: readonly string[], prefersLight: boolean): "dark" | "light" {
  const schemes = modes.filter(
    (mode): mode is "dark" | "light" => mode === "dark" || mode === "light",
  );
  const wanted = prefersLight ? "light" : "dark";
  return schemes.includes(wanted) ? wanted : (schemes[0] ?? "dark");
}

function prefersLight(): boolean {
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: light)").matches
  );
}

export function applyColorScheme(root: HTMLElement, modes: readonly string[]) {
  root.dataset.colorScheme = colorScheme(modes, prefersLight());
}

/** Dev reloads ask for the same URL again; a fresh query defeats caches. */
let reloads = 0;

/**
 * Applies a resolved theme: its stylesheet (unless it is already the one
 * applied; `reload` re-fetches it after a dev edit) and its colour scheme.
 */
export async function applyResolvedTheme(
  theme: ResolvedTheme,
  {
    root = document.documentElement,
    timeoutMs = 3000,
    reload = false,
  }: { root?: HTMLElement; timeoutMs?: number; reload?: boolean } = {},
): Promise<void> {
  const current = document.head.querySelectorAll<HTMLElement>("[data-cs-theme]");
  const same = current.length === 1 && current[0]?.dataset.csTheme === theme.id;
  if (!same || reload) {
    const source = theme.url
      ? { href: reload ? `${theme.url}?v=${++reloads}` : theme.url }
      : { css: await previewStylesheet() };
    // Overtaken by a newer theme: that one sets the scheme.
    if (!(await swap(theme.id, source, timeoutMs))) return;
  }
  applyColorScheme(root, theme.modes);
  setActiveTheme(theme);
}

let requests = 0;

/** Resolves `requested` (the settings' theme when null) and applies it. */
export async function applyActiveTheme(
  api: ThemesApi,
  requested: string | null,
  options?: Parameters<typeof applyResolvedTheme>[1],
): Promise<ResolvedTheme> {
  requests += 1;
  const request = requests;
  const theme = await api.resolve(requested);
  // A newer choice was made meanwhile: it applies, not this one.
  if (request !== requests) return theme;
  if (theme.notice) console.warn(theme.notice);
  await applyResolvedTheme(theme, options);
  return theme;
}

// The applied theme, for the Appearance panel's notice and safe mode.
let active: ResolvedTheme | null = null;
const listeners = new Set<() => void>();

export function setActiveTheme(theme: ResolvedTheme | null) {
  active = theme;
  for (const listener of listeners) listener();
}

export function getActiveTheme(): ResolvedTheme | null {
  return active;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The theme applied now, with the notice when it isn't the chosen one. */
export function useActiveTheme(): ResolvedTheme | null {
  return useSyncExternalStore(subscribe, getActiveTheme, getActiveTheme);
}
