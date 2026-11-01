import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { nativeSettingsApi, type SettingsApi } from "../api/settings";
import { effectSubscription } from "../api/subscription";
import { nativeThemesApi, type ThemesApi } from "../themes/api";
import {
  applyActiveTheme,
  applyColorScheme,
  getActiveTheme,
  useActiveTheme,
} from "../themes/apply";
import { type AppearancePreferences, applyAppearance } from "./apply";
import { applyUserCss, problemText } from "./userCss";

export const appearanceKey = ["settings", "core.appearance"] as const;

/** Keeps `<html>` in step with `core.appearance`, live and from startup:
 * the theme's stylesheet and colour scheme (#140), the user CSS (#142) and
 * the attributes and tokens of #141. */
export function AppearanceRoot({
  api = nativeSettingsApi,
  themes = nativeThemesApi,
  root = document.documentElement,
}: {
  api?: SettingsApi;
  themes?: ThemesApi;
  root?: HTMLElement;
}) {
  const client = useQueryClient();
  const settings = useQuery({ queryKey: appearanceKey, queryFn: () => api.get("core.appearance") });
  useEffect(
    () =>
      effectSubscription(api.subscribe, ({ namespace }) => {
        if (namespace === "core.appearance")
          void client.invalidateQueries({ queryKey: appearanceKey });
      }),
    [api, client],
  );
  const value = settings.data?.value as AppearancePreferences | undefined;
  const active = useActiveTheme();

  // The chosen theme, swapped without a reload. Startup applied the first.
  const theme = value?.theme;
  const wanted = useRef<string | null>(null);
  useEffect(() => {
    if (!theme) return;
    const previous = wanted.current ?? getActiveTheme()?.requested;
    wanted.current = theme;
    if (previous === theme) return;
    void applyActiveTheme(themes, theme, { root }).catch((error: unknown) =>
      console.warn("theme not applied", error),
    );
  }, [themes, theme, root]);

  // Dev themes (#140): re-apply the active one after each edit.
  useEffect(
    () =>
      effectSubscription(themes.onReloaded, ({ theme: edited, error }) => {
        const current = getActiveTheme();
        if (!current || (edited !== current.id && edited !== current.requested)) return;
        if (error) console.warn(`theme ${edited}: ${error}`);
        void applyActiveTheme(themes, current.requested, { root, reload: true }).catch(
          (failure: unknown) => console.warn("theme not applied", failure),
        );
      }),
    [themes, root],
  );

  // The OS switching between light and dark, for themes with both.
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia("(prefers-color-scheme: light)");
    const follow = () => {
      const current = getActiveTheme();
      if (current) applyColorScheme(root, current.modes);
    };
    query.addEventListener("change", follow);
    return () => query.removeEventListener("change", follow);
  }, [root]);

  // User CSS, unless it is switched off or Caskade started in safe mode.
  const css =
    value?.customCssEnabled !== false && !active?.safeMode ? (value?.customCss ?? null) : null;
  useEffect(() => {
    const problems = applyUserCss(css, root.ownerDocument);
    if (problems.length > 0) console.warn(`custom CSS not applied:\n${problemText(problems)}`);
  }, [css, root]);

  // The accent is checked against the theme's surfaces: apply it again
  // whenever another theme is applied.
  const themeId = active?.id ?? null;
  useEffect(() => {
    if (!value) return;
    const accent = applyAppearance(root, value);
    if (accent && !accent.ok) console.warn(`${themeId ?? "theme"}: ${accent.message}`);
  }, [root, value, themeId]);
  return null;
}
