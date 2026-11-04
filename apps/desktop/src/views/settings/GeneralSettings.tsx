import {
  Button,
  SegmentedControl,
  Select,
  SettingRow,
  SettingsSection,
  Switch,
  TextField,
} from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useNavigate } from "@tanstack/react-router";
import {
  Clock,
  Cloud,
  FolderOpen,
  Gamepad2,
  Globe,
  HardDrive,
  House,
  ImageDown,
  Languages,
  LogOut,
  MessageSquareWarning,
  MonitorPlay,
  MonitorUp,
  Play,
  Power,
  Rocket,
  RotateCcw,
  Sparkles,
} from "lucide-react";
import { StorageSummary } from "../../components/storage/StorageSummary";
import { LanguageSelect } from "../../locale/LanguageSelect";
import { AppUpdateSection } from "./AppUpdateSection";
import { ResetDefaults, type ResetPart } from "./ResetDefaults";
import { UpdateSettings } from "./UpdateSettings";
import {
  SettingsFeedback,
  SettingsLoading,
  SettingsStatus,
  useSettingsSection,
} from "./useSettings";

/** A choice's label, translated where it is shown (`useOptions`). */
interface Option<K extends string> {
  id: K;
  label: MessageDescriptor;
}

/** Options with their labels in the current language. */
function useOptions<K extends string>(options: readonly Option<K>[]) {
  const { t } = useLingui();
  return options.map(({ id, label }) => ({ id, label: t(label) }));
}

const startViews = [
  { id: "home", label: msg({ message: "Home", context: "navigation" }) },
  { id: "library", label: msg`Library` },
  { id: "collections", label: msg`Collections` },
  { id: "last", label: msg`Last opened view` },
] as const satisfies readonly Option<string>[];

const onLaunchOptions = [
  { id: "stay", label: msg`Stay open` },
  { id: "minimize", label: msg({ message: "Minimize", context: "window" }) },
  { id: "tray", label: msg`Hide to tray` },
] as const satisfies readonly Option<string>[];

const closeOptions = [
  { id: "quit", label: msg({ message: "Quit", context: "close button setting" }) },
  { id: "tray", label: msg`Hide to tray` },
] as const satisfies readonly Option<string>[];

const timeFormats = [
  {
    id: "system",
    label: msg({
      message: "By language",
      comment: "Time format: 12 or 24 hours, whichever the app's language uses",
    }),
  },
  { id: "12h", label: msg`12-hour` },
  { id: "24h", label: msg`24-hour` },
] as const satisfies readonly Option<string>[];

const consoleStarts = [
  { id: "auto", label: msg({ message: "Automatic", context: "start mode" }) },
  { id: "desktop", label: msg({ message: "Desktop", context: "mode" }) },
  { id: "console", label: msg`Console mode` },
] as const satisfies readonly Option<string>[];

interface ConsolePreferences {
  start: (typeof consoleStarts)[number]["id"];
  active: boolean;
}

/** `core.console` (#148): which mode Caskade opens in. */
function ConsoleStartRow() {
  const { t } = useLingui();
  const options = useOptions(consoleStarts);
  const section = useSettingsSection<ConsolePreferences>("core.console");
  const { settings, setSettings } = section;
  if (!settings) return <SettingsStatus section={section} what={t`Start in`} />;
  return (
    <>
      <SettingRow
        icon={MonitorPlay}
        label={t`Start in`}
        description={t`Automatic opens console mode in Steam Deck game mode (gamescope), otherwise the mode you last used.`}
      >
        <Select
          options={options}
          value={settings.start}
          onChange={(start) => setSettings((current) => ({ ...current, start }))}
          className="w-44"
        />
      </SettingRow>
      <SettingsStatus section={section} what={t`Start in`} />
    </>
  );
}

interface GeneralPreferences {
  launchOnLogin: boolean;
  startMinimized: boolean;
  startView: (typeof startViews)[number]["id"];
  onLaunch: (typeof onLaunchOptions)[number]["id"];
  onClose: (typeof closeOptions)[number]["id"];
  confirmQuit: boolean;
  cacheDir: string;
  /** `system` or a language tag (#213). */
  language: string;
  timeFormat: (typeof timeFormats)[number]["id"];
}

/** `core.metadata` and `core.onboarding`: what goes online, and first-run
 * setup again. */
/** Looking games up online, in a card of its own rather than under Setup
 * (#54). */
function OnlineSection() {
  const { t } = useLingui();
  const metadata = useSettingsSection<{ online: boolean }>("core.metadata");
  return (
    <SettingsSection
      icon={Cloud}
      title={t({ message: "Online", context: "settings section" })}
      description={t`What Caskade looks up on the internet.`}
    >
      {metadata.settings && (
        <SettingRow
          icon={ImageDown}
          label={t`Fetch artwork and game info online`}
          description={t`Covers, descriptions and genres from IGDB and Hasheous, looked up in the background.`}
        >
          <Switch
            isSelected={metadata.settings.online}
            onChange={(online) => metadata.setSettings((s) => ({ ...s, online }))}
          />
        </SettingRow>
      )}
      <SettingsStatus section={metadata} what={t`Online lookups`} />
    </SettingsSection>
  );
}

function SetupSection() {
  const { t } = useLingui();
  const onboarding = useSettingsSection<{ completed: boolean; sounds: boolean }>("core.onboarding");
  return (
    <SettingsSection
      icon={Sparkles}
      title={t({ message: "Setup", context: "settings section" })}
      description={t`The setup Caskade starts with.`}
    >
      {onboarding.settings && (
        <SettingRow
          icon={RotateCcw}
          label={t`First-run setup`}
          description={t`Choose your games folder, what to look for and how you play again.`}
        >
          <Button
            isDisabled={onboarding.saving}
            onPress={() => onboarding.setSettings((s) => ({ ...s, completed: false }))}
          >
            <Trans>Run setup again</Trans>
          </Button>
        </SettingRow>
      )}
      <SettingsStatus section={onboarding} what={t`First-run setup`} />
    </SettingsSection>
  );
}

export function GeneralSettings() {
  const { t } = useLingui();
  const navigate = useNavigate();
  const views = useOptions(startViews);
  const onLaunch = useOptions(onLaunchOptions);
  const onClose = useOptions(closeOptions);
  const times = useOptions(timeFormats);
  const { settings, setSettings, saving, error, reload } =
    useSettingsSection<GeneralPreferences>("core.general");
  const set = <K extends keyof GeneralPreferences>(key: K, value: GeneralPreferences[K]) =>
    setSettings((s) => ({ ...s, [key]: value }));
  if (!settings) return <SettingsLoading error={error} reload={reload} />;

  // Every preference on this page, from whichever section keeps it (#44);
  // first-run setup's completion and the console's last mode stay.
  const resetParts: ResetPart[] = [
    {
      namespace: "core.general",
      whole: true,
      fields: [
        { key: "launchOnLogin", label: t`Launch on system startup` },
        { key: "startMinimized", label: t`Start minimized` },
        { key: "startView", label: t`Open to` },
        { key: "onLaunch", label: t`When a game launches` },
        { key: "onClose", label: t`Close button` },
        { key: "confirmQuit", label: t`Confirm before quitting` },
        { key: "cacheDir", label: t`Cache folder` },
        { key: "language", label: t`Language` },
        { key: "timeFormat", label: t`Time format` },
      ],
    },
    { namespace: "core.console", fields: [{ key: "start", label: t`Start in` }] },
    { namespace: "core.appUpdates", fields: [{ key: "check", label: t`Check automatically` }] },
    {
      namespace: "core.metadata",
      fields: [{ key: "online", label: t`Fetch artwork and game info online` }],
    },
  ];

  // Two columns only where the panel itself has room for them beside both
  // navigation columns: a container query, in rem, so the interface scale
  // counts too (#41).
  return (
    <div data-settings="general" className="@container">
      <div className="grid items-start gap-4 @6xl:grid-cols-2">
        <div className="col-span-full flex items-center justify-between gap-3">
          <SettingsFeedback error={error} saving={saving} />
          <ResetDefaults
            group={t({ message: "General", context: "settings tab" })}
            label={t`Reset general settings`}
            parts={resetParts}
            keeps={t`Game updates, which has its own reset, settings on other pages and whether first-run setup is done stay as they are.`}
            isDisabled={saving}
          />
        </div>
        <SettingsSection
          icon={Power}
          title={t`Startup`}
          description={t`What happens when Caskade opens.`}
        >
          <SettingRow
            icon={Rocket}
            label={t`Launch on system startup`}
            description={t`Start Caskade when you log in.`}
          >
            <Switch isSelected={settings.launchOnLogin} onChange={(v) => set("launchOnLogin", v)} />
          </SettingRow>
          <SettingRow
            icon={MonitorUp}
            label={t`Start minimized`}
            // A disabled switch says why (#55).
            description={
              settings.launchOnLogin
                ? t`Open in the system tray instead of a window.`
                : t`Open in the system tray instead of a window. Needs Launch on system startup.`
            }
          >
            <Switch
              isSelected={settings.startMinimized}
              onChange={(v) => set("startMinimized", v)}
              isDisabled={!settings.launchOnLogin}
            />
          </SettingRow>
          <SettingRow icon={House} label={t`Open to`} description={t`The first view you see.`}>
            <Select
              options={views}
              value={settings.startView}
              onChange={(v) => set("startView", v)}
              className="w-44"
            />
          </SettingRow>
          <ConsoleStartRow />
        </SettingsSection>

        <SettingsSection
          icon={Gamepad2}
          title={t`Behaviour`}
          description={t`What the window does while you play.`}
        >
          <SettingRow
            icon={Play}
            label={t`When a game launches`}
            description={t`What Caskade does with its window.`}
          >
            <Select
              options={onLaunch}
              value={settings.onLaunch}
              onChange={(v) => set("onLaunch", v)}
              className="w-44"
            />
          </SettingRow>
          <SettingRow
            icon={LogOut}
            label={t`Close button`}
            description={t`Hiding keeps the guide button and downloads working.`}
          >
            <SegmentedControl
              options={onClose}
              value={settings.onClose}
              onChange={(v) => set("onClose", v)}
            />
          </SettingRow>
          <SettingRow
            icon={MessageSquareWarning}
            label={t`Confirm before quitting`}
            description={t`Ask while a game is running.`}
          >
            <Switch isSelected={settings.confirmQuit} onChange={(v) => set("confirmQuit", v)} />
          </SettingRow>
        </SettingsSection>

        <SettingsSection
          icon={HardDrive}
          title={t`Locations`}
          description={t`Where Caskade installs games and keeps its data.`}
        >
          <div className="mt-3 flex flex-col gap-4">
            <div data-part="storage-summary" className="flex flex-col gap-1.5">
              <span className="text-sm font-medium">
                <Trans>Game storage</Trans>
              </span>
              <div className="flex gap-2">
                <div className="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-control border border-border bg-surface-base/30 px-3">
                  <HardDrive className="size-4 shrink-0 text-fg-subtle" />
                  <span className="truncate text-sm">
                    <Trans>Storage locations</Trans>
                  </span>
                  <StorageSummary />
                </div>
                <Button onPress={() => navigate({ to: "/settings", search: { tab: "storage" } })}>
                  <Trans>Manage</Trans>
                </Button>
              </div>
              <span className="text-xs text-fg-muted">
                <Trans>Manage the default location and other game storage roots.</Trans>
              </span>
            </div>
            <TextField
              label={t`Cache folder`}
              description={t`Artwork, thumbnails and game details that Caskade can download again.`}
              icon={FolderOpen}
              mono
              value={settings.cacheDir}
              isReadOnly
            />
          </div>
        </SettingsSection>

        <SettingsSection
          icon={Globe}
          title={t`Language & Region`}
          description={t`How text, dates and times are shown.`}
        >
          <SettingRow icon={Languages} label={t`Language`}>
            <LanguageSelect value={settings.language} onChange={(v) => set("language", v)} />
          </SettingRow>
          <SettingRow icon={Clock} label={t`Time format`}>
            <SegmentedControl
              options={times}
              value={settings.timeFormat}
              onChange={(v) => set("timeFormat", v)}
            />
          </SettingRow>
        </SettingsSection>

        <AppUpdateSection />
        {/* Beside Caskade's own updates, not under Installer (#54). */}
        <UpdateSettings />
        <OnlineSection />
        <SetupSection />
      </div>
    </div>
  );
}
