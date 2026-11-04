import { Button, SegmentedControl, SettingRow, SettingsSection, Switch } from "@caskade/ui";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useRouter } from "@tanstack/react-router";
import {
  Activity,
  Captions,
  Code,
  Contrast,
  Droplet,
  Image,
  LayoutGrid,
  Palette,
  RectangleHorizontal,
  Rows3,
  Scaling,
  Sparkles,
} from "lucide-react";
import { useState } from "react";
import { deriveAccent } from "../../appearance/accent";
import { themeSurfaces } from "../../appearance/apply";
import { DEFAULT_THEME } from "../../themes/api";
import { useActiveTheme } from "../../themes/apply";
import { CustomCssEditor } from "./appearance/CustomCssEditor";
import { ThemePicker } from "./appearance/ThemePicker";
import { DraftSlider } from "./DraftSlider";
import { SectionSync } from "./devices/SectionSync";
import { ResetDefaults, type ResetPart } from "./ResetDefaults";
import { SettingsFeedback, SettingsLoading, useSettingsSection } from "./useSettings";

const densities = [
  { id: "compact", label: msg({ message: "Compact", context: "density" }), icon: Rows3 },
  {
    id: "comfortable",
    label: msg({ message: "Comfortable", context: "density" }),
    icon: LayoutGrid,
  },
] as const;

const motionOptions = [
  {
    id: "system",
    label: msg({
      message: "System",
      context: "animations setting",
      comment: "Follow the operating system's reduced-motion setting",
    }),
  },
  { id: "full", label: msg({ message: "Full", context: "animations setting" }) },
  { id: "reduced", label: msg({ message: "Reduced", context: "animations setting" }) },
] as const;

interface AppearancePreferences {
  /** An installed theme's id (#140). */
  theme: string;
  density: (typeof densities)[number]["id"];
  cardSize: number;
  uiScale: number;
  showArtTitles: boolean;
  motion: (typeof motionOptions)[number]["id"];
  reduceEffects: boolean;
  /** `#rrggbb`, or null for the theme's accent (#141). */
  accent?: string | null;
  /** User CSS in the `cs.user` layer (#142), and whether it applies. */
  customCss?: string;
  customCssEnabled?: boolean;
}

/** The theme's accent unless the user picked one; a pick that would be hard
 * to see on this theme's surfaces is adjusted (with a note) or refused. */
function AccentPicker({
  value,
  onChange,
}: {
  value: string | null;
  onChange: (accent: string | null) => void;
}) {
  const { t } = useLingui();
  const [note, setNote] = useState<{ text: string; refused: boolean } | null>(null);
  const shown =
    value ??
    (getComputedStyle(document.documentElement).getPropertyValue("--cs-color-accent").trim() ||
      "#5b5ff7");
  return (
    <div className="flex flex-col items-end gap-1.5">
      <div className="flex items-center gap-2">
        <input
          type="color"
          aria-label={t`Accent colour`}
          data-part="accent-input"
          value={/^#[0-9a-f]{6}$/i.test(shown) ? shown : "#5b5ff7"}
          onChange={(event) => {
            const result = deriveAccent(
              event.target.value,
              themeSurfaces(document.documentElement),
            );
            if (!result.ok) {
              setNote({ text: result.message, refused: true });
              return;
            }
            setNote(result.message ? { text: result.message, refused: false } : null);
            onChange(result.accent);
          }}
          className="size-9 cursor-pointer rounded-control border border-border bg-transparent p-0.5"
        />
        <Button
          variant="ghost"
          size="sm"
          isDisabled={value === null}
          onPress={() => {
            setNote(null);
            onChange(null);
          }}
        >
          <Trans>Theme accent</Trans>
        </Button>
      </div>
      {note && (
        <p
          role={note.refused ? "alert" : "status"}
          className={`max-w-64 text-right text-xs ${note.refused ? "text-danger" : "text-fg-muted"}`}
        >
          {note.text}
        </p>
      )}
    </div>
  );
}

export function AppearanceSettings() {
  const { t } = useLingui();
  const { settings, setSettings, saving, error, reload } =
    useSettingsSection<AppearancePreferences>("core.appearance");
  const set = <K extends keyof AppearancePreferences>(key: K, value: AppearancePreferences[K]) =>
    setSettings((s) => ({ ...s, [key]: value }));
  const active = useActiveTheme();
  if (!settings) return <SettingsLoading error={error} reload={reload} />;

  // The page shows the whole section, custom CSS included (#44).
  const resetParts: ResetPart[] = [
    {
      namespace: "core.appearance",
      whole: true,
      fields: [
        { key: "theme", label: t`Theme` },
        { key: "density", label: t`Density` },
        { key: "uiScale", label: t`Interface scale` },
        { key: "accent", label: t`Accent colour` },
        { key: "cardSize", label: t`Card size` },
        { key: "showArtTitles", label: t`Show titles on artwork` },
        { key: "motion", label: t`Animations` },
        { key: "reduceEffects", label: t`Reduce effects` },
        { key: "customCss", label: t`Custom CSS` },
        { key: "customCssEnabled", label: t`Disable custom CSS` },
      ],
    },
  ];

  // Two columns only where the panel itself has room for them (#41).
  return (
    <div data-settings="appearance" className="@container">
      <div className="grid items-start gap-4 @6xl:grid-cols-2">
        <div className="col-span-full flex items-center justify-between gap-3">
          <SettingsFeedback error={error} saving={saving} />
          <ResetDefaults
            group={t({ message: "Appearance", context: "settings tab" })}
            label={t`Reset appearance settings`}
            parts={resetParts}
            isDisabled={saving}
          />
        </div>
        <SettingsSection
          icon={Palette}
          title={t`Theme`}
          description={t`How Caskade looks. Themes restyle every view through design tokens.`}
          className="col-span-full"
        >
          <ThemePicker value={settings.theme || DEFAULT_THEME} onChange={(v) => set("theme", v)} />
          <MoreThemes />
        </SettingsSection>

        <SettingsSection
          icon={Sparkles}
          title={t({ message: "Look", context: "settings section", comment: "The appearance" })}
          description={t`Adjust the theme to taste.`}
        >
          <SettingRow icon={LayoutGrid} label={t`Density`}>
            <SegmentedControl
              options={densities.map((option) => ({ ...option, label: t(option.label) }))}
              value={settings.density}
              onChange={(v) => set("density", v)}
            />
          </SettingRow>
          <SettingRow icon={Scaling} label={t`Interface scale`}>
            <DraftSlider
              minValue={80}
              maxValue={150}
              step={5}
              value={settings.uiScale}
              commit={(v) => set("uiScale", v)}
              format={(v) => `${v}%`}
              className="w-56"
            />
          </SettingRow>
          <SettingRow
            icon={Droplet}
            label={t`Accent colour`}
            description={t`Buttons, selection and glows; kept readable on the theme.`}
          >
            <AccentPicker value={settings.accent ?? null} onChange={(v) => set("accent", v)} />
          </SettingRow>
          {/* The interface scale suits each screen and always stays here. */}
          <SectionSync namespace="core.appearance" />
        </SettingsSection>

        <SettingsSection
          icon={Image}
          title={t`Library`}
          description={t`How games are shown in grids.`}
        >
          <SettingRow
            icon={RectangleHorizontal}
            label={t`Card size`}
            description={t`Minimum width of a game card.`}
          >
            <DraftSlider
              minValue={128}
              maxValue={288}
              step={8}
              value={settings.cardSize}
              commit={(v) => set("cardSize", v)}
              format={(v) => `${v}px`}
              className="w-56"
            />
          </SettingRow>
          <SettingRow
            icon={Captions}
            label={t`Show titles on artwork`}
            description={t`Overlay the game title on cover art.`}
          >
            <Switch isSelected={settings.showArtTitles} onChange={(v) => set("showArtTitles", v)} />
          </SettingRow>
        </SettingsSection>

        <SettingsSection
          icon={Activity}
          title={t`Motion & Effects`}
          description={t`Turn these down on slower machines.`}
        >
          <SettingRow
            icon={Activity}
            label={t`Animations`}
            description={t`System follows your OS reduced-motion setting.`}
          >
            <SegmentedControl
              options={motionOptions.map((option) => ({ ...option, label: t(option.label) }))}
              value={settings.motion}
              onChange={(v) => set("motion", v)}
            />
          </SettingRow>
          <SettingRow
            icon={Contrast}
            label={t`Reduce effects`}
            description={t`Turn off blur, glow, scanlines and the backdrop grid.`}
          >
            <Switch isSelected={settings.reduceEffects} onChange={(v) => set("reduceEffects", v)} />
          </SettingRow>
        </SettingsSection>

        <SettingsSection
          icon={Code}
          title={t`Custom CSS`}
          description={t`Your own rules, applied over the theme without a reload.`}
          className="col-span-full"
        >
          <CustomCssEditor
            css={settings.customCss ?? ""}
            enabled={settings.customCssEnabled !== false}
            safeMode={active?.safeMode ?? false}
            saving={saving}
            onSave={(css) => set("customCss", css)}
            onEnabledChange={(enabled) => set("customCssEnabled", enabled)}
          />
        </SettingsSection>
      </div>
    </div>
  );
}

/** Links to the catalogue's themes; only where there is a router to follow it. */
function MoreThemes() {
  const router = useRouter({ warn: false }) as ReturnType<typeof useRouter> | undefined;
  if (!router) return null;
  return (
    <Link
      data-part="more-themes"
      to="/settings"
      search={{ tab: "plugins" }}
      className="mt-3 inline-block text-sm text-accent-alt"
    >
      <Trans>More themes in the plugin and theme catalogue</Trans>
    </Link>
  );
}
