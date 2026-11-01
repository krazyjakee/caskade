/** The look before the first render: the chosen theme (so the default never
 * flashes in first, #140), the user CSS (#142) and the Appearance
 * attributes (#141). `AppearanceRoot` keeps them live afterwards. */
import { nativeSettingsApi, type SettingsApi } from "../api/settings";
import { nativeThemesApi, type ThemesApi } from "../themes/api";
import { applyActiveTheme, applyTheme, DEFAULT_THEME } from "../themes/apply";
import { type AppearancePreferences, applyAppearance } from "./apply";
import { applyUserCss, problemText } from "./userCss";

export async function startAppearance({
  settings = nativeSettingsApi,
  themes = nativeThemesApi,
  root = document.documentElement,
}: {
  settings?: SettingsApi;
  themes?: ThemesApi;
  root?: HTMLElement;
} = {}): Promise<void> {
  const [theme, appearance] = await Promise.all([
    // The core reads the choice from the settings itself.
    applyActiveTheme(themes, null, { root }).catch(async (error: unknown) => {
      console.warn("theme not applied", error);
      await applyTheme(DEFAULT_THEME).catch(() => {});
      return null;
    }),
    settings
      .get("core.appearance")
      .then((snapshot) => snapshot.value as unknown as AppearancePreferences)
      .catch(() => null),
  ]);
  if (!appearance) return;
  if (appearance.customCssEnabled !== false && !theme?.safeMode) {
    const problems = applyUserCss(appearance.customCss ?? null, root.ownerDocument);
    if (problems.length > 0) console.warn(`custom CSS not applied:\n${problemText(problems)}`);
  }
  applyAppearance(root, appearance);
}
