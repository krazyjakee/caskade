/** Appearance settings → root attributes and token overrides (#141). Themes
 * keep working: each choice is a token set selected by an attribute on
 * `<html>` (`data-effects`, `data-motion`, `data-density`) or an inline token. */
import { type AccentResult, deriveAccent, parseSurface } from "./accent";

export interface AppearancePreferences {
  density: "compact" | "comfortable";
  uiScale: number;
  motion: "system" | "full" | "reduced";
  reduceEffects: boolean;
  /** `#rrggbb`, or null for the theme's accent. */
  accent?: string | null;
  /** The chosen theme's id (#140). */
  theme?: string;
  /** User CSS (#142) and whether it applies. */
  customCss?: string;
  customCssEnabled?: boolean;
}

const ACCENT_TOKENS = [
  "--cs-color-accent",
  "--cs-color-accent-strong",
  "--cs-color-accent-fg",
  "--cs-color-glow",
] as const;

/** The theme's opaque surfaces, read from its tokens (translucent ones are
 * composited over the base). */
export function themeSurfaces(root: HTMLElement) {
  const style = getComputedStyle(root);
  const read = (name: string) => style.getPropertyValue(name).trim();
  const base = parseSurface(read("--cs-color-surface-base"));
  if (!base) return [];
  const raised = parseSurface(read("--cs-color-surface-raised"), base);
  return raised ? [base, raised] : [base];
}

/**
 * Applies the preferences to `root` and returns what happened to the
 * accent (adjusted or refused, with a message) when one is set.
 * `system` motion is decided in CSS by `prefers-reduced-motion`.
 */
export function applyAppearance(
  root: HTMLElement,
  prefs: AppearancePreferences,
): AccentResult | null {
  root.dataset.density = prefs.density;
  root.dataset.motion = prefs.motion;
  if (prefs.reduceEffects) root.dataset.effects = "reduced";
  else delete root.dataset.effects;
  root.style.setProperty("--cs-ui-scale", String(Math.min(150, Math.max(80, prefs.uiScale)) / 100));
  for (const token of ACCENT_TOKENS) root.style.removeProperty(token);
  if (!prefs.accent) return null;
  const result = deriveAccent(prefs.accent, themeSurfaces(root));
  if (result.ok) {
    root.style.setProperty("--cs-color-accent", result.accent);
    root.style.setProperty("--cs-color-accent-strong", result.strong);
    root.style.setProperty("--cs-color-accent-fg", result.fg);
    root.style.setProperty("--cs-color-glow", result.glow);
  }
  return result;
}
