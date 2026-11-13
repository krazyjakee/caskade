import { sharedSpecifiers } from "@caskade/vite-plugin";

/** Runtime exports of the app modules UI plugins import. The Vite shim
 * generator (`pluginShims.ts`) reads this; a test keeps it in sync. */
export const appExports: Record<string, readonly string[]> = {
  "@caskade/sdk": [
    "API_VERSION",
    "FocusSection",
    "HostScope",
    "PluginScope",
    "Trans",
    "createPluginI18n",
    "definePlugin",
    "pluginSlots",
    "useBackendQuery",
    "useFocusable",
    "useGame",
    "useGames",
    "useInputAction",
    "useMode",
    "usePlugin",
    "useSettings",
    "useTranslation",
  ],
  "@caskade/ui": [
    "Avatar",
    "AvatarPicker",
    "Button",
    "Checkbox",
    "ContextMenu",
    "DEFAULT_AVATAR",
    "ErrorNotice",
    "GameArt",
    "Menu",
    "MenuItem",
    "MenuPopover",
    "MenuSection",
    "MenuSeparator",
    "Pill",
    "SegmentedControl",
    "Select",
    "SettingRow",
    "SettingsSection",
    "Slider",
    "StatusBadge",
    "StatusDot",
    "Switch",
    "TextField",
    "anatomy",
    "anatomyNames",
    "anatomyRoots",
    "artBackground",
    "avatarInfo",
    "avatars",
    "checkAnatomy",
    "contextMenuTarget",
    "initials",
    "ownedParts",
    "useContextMenu",
  ],
};

export function shimPath(specifier: string): string {
  return `/plugin-shims/${specifier.replace(/^@/, "").replaceAll("/", "-")}.js`;
}

/** The import map the shell document declares before any module loads. */
export function importMap(): { imports: Record<string, string> } {
  return { imports: Object.fromEntries(sharedSpecifiers.map((s) => [s, shimPath(s)])) };
}

/** A shim module re-exporting one shared module from `__caskadeShared`. */
export function shimSource(specifier: string, names: readonly string[]): string {
  const valid = names.filter((name) => name !== "default" && /^[A-Za-z_$][\w$]*$/.test(name));
  return [
    `const m = globalThis.__caskadeShared?.[${JSON.stringify(specifier)}];`,
    `if (!m) throw new Error(${JSON.stringify(`Caskade has not shared ${specifier} with plugins`)});`,
    "export default m.default ?? m;",
    valid.length ? `export const { ${valid.join(", ")} } = m;` : "",
    "",
  ].join("\n");
}
