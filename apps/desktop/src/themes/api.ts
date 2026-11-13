/** The installed themes and the active one (#140), from the core; the
 * browser preview has only the built-in default. */
import { t } from "@lingui/core/macro";
import { isTauri } from "@tauri-apps/api/core";
import { createContext } from "react";
import { commands, events } from "../api/bindings";
import { unwrap } from "../api/reply";

export const DEFAULT_THEME = "caskade.default";

interface ThemeProblem {
  file: string;
  line?: number | null;
  column?: number | null;
  message: string;
}

/** One installed theme, as the picker shows it. */
export interface InstalledTheme {
  id: string;
  name: string;
  version: string;
  description?: string | null;
  author?: string | null;
  extends?: string | null;
  /** Colour schemes it provides, its default first. */
  modes: string[];
  /** Preview image URLs. */
  previews: string[];
  /** `builtin`, `user`, `plugin` or `dev`. */
  source: string;
  folder?: string | null;
  /** Why it can't be applied; empty when it can. */
  problems: ThemeProblem[];
  /** Problems that don't stop it, such as a translation catalog in
   * `locales/` that can't be read, whose English shows instead (#219). */
  warnings?: ThemeProblem[];
}

/** The theme to apply for the one the settings name. */
export interface ResolvedTheme {
  /** The requested theme, or the default when it can't be applied. */
  id: string;
  name: string;
  /** The theme the settings name. */
  requested: string;
  modes: string[];
  /** The compiled stylesheet; none in the browser preview, which inlines
   * the default. */
  url: string | null;
  /** Why the requested theme isn't the one applied. */
  notice: string | null;
  /** Started with `--safe-mode`: the default theme and no custom CSS. */
  safeMode: boolean;
}

export interface ThemeReloaded {
  theme: string;
  /** Why the edited theme no longer compiles. */
  error: string | null;
}

export interface ThemesApi {
  list(): Promise<InstalledTheme[]>;
  /** Reads every theme folder again. */
  reload(): Promise<InstalledTheme[]>;
  /** The theme to apply for `id`; `null` asks for the one in the settings. */
  resolve(id: string | null): Promise<ResolvedTheme>;
  /** Dev themes recompiled after an edit. */
  onReloaded(listener: (event: ThemeReloaded) => void): Promise<() => void>;
  /** Development builds: load a theme folder in place and watch it. Without
   * `folder` the user picks one. Resolves to its id, or null when no folder
   * was picked. */
  loadUnpacked?(folder?: string): Promise<string | null>;
}

const NEXUS: InstalledTheme = {
  id: DEFAULT_THEME,
  name: "Nexus",
  version: "1.0.0",
  description:
    "Caskade's dark sci-fi look: indigo and cyan accents, glass panels, HUD frames and a drifting grid.",
  author: "Caskade",
  modes: ["dark"],
  previews: [],
  source: "builtin",
  problems: [],
};

const safeModeNotice = () =>
  t`Safe mode: Caskade started with the default theme and without your custom CSS.`;

/** The browser preview starts in safe mode with `?safe-mode`. */
function previewSafeMode() {
  return new URLSearchParams(window.location.search).has("safe-mode");
}

function previewResolve(id: string | null): ResolvedTheme {
  const requested = id ?? DEFAULT_THEME;
  const safeMode = previewSafeMode();
  return {
    id: DEFAULT_THEME,
    name: NEXUS.name,
    requested,
    modes: NEXUS.modes,
    url: null,
    notice: safeMode
      ? safeModeNotice()
      : requested === DEFAULT_THEME
        ? null
        : t`The theme ${{ theme: requested }} needs the Caskade core. The browser preview shows the default theme.`,
    safeMode,
  };
}

/** Development builds: a theme folder loaded in place and watched. */
async function loadUnpacked(dropped?: string): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const folder =
    dropped ?? (await open({ directory: true, multiple: false, title: t`Theme folder` }));
  if (typeof folder !== "string") return null;
  return unwrap(commands.themeLoadUnpacked(folder));
}

export const nativeThemesApi: ThemesApi = {
  async list() {
    if (!isTauri()) return [NEXUS];
    return unwrap(commands.themesList());
  },
  async reload() {
    if (!isTauri()) return [NEXUS];
    return unwrap(commands.themesReload());
  },
  async resolve(id) {
    if (!isTauri()) return previewResolve(id);
    return unwrap(commands.themeResolve(id));
  },
  async onReloaded(listener) {
    if (!isTauri()) return () => {};
    return events.themeReloaded.listen(({ payload }) =>
      listener({ theme: payload.theme, error: payload.error ?? null }),
    );
  },
  loadUnpacked: import.meta.env.DEV && isTauri() ? loadUnpacked : undefined,
};

/** The themes backend the Appearance panel uses; tests provide a fake. */
export const ThemesApiContext = createContext<ThemesApi>(nativeThemesApi);
