import {
  Button,
  Checkbox,
  Select,
  SettingRow,
  SettingsSection,
  Switch,
  TextField,
} from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useRouter } from "@tanstack/react-router";
import { Folder, FolderOpen, RefreshCw, ScanSearch } from "lucide-react";
import { useState } from "react";
import { type DiscoveryPreferences, scanning } from "../../api/discovery";
import { formatPath } from "../../api/presentation";
import { usePathPicker } from "../../components/pathBrowser/PathPicker";
import { useDropZone } from "../../drop/DropProvider";
import { useDiscovery } from "./discovery/DiscoveryProvider";
import { HiddenGames } from "./discovery/HiddenGames";
import { ReviewList } from "./discovery/ReviewList";
import { ResetDefaults } from "./ResetDefaults";
import { SettingsFeedback, SettingsLoading, useSettingsSection } from "./useSettings";

const intervals = [
  { id: "1h", label: msg({ message: "1 hour", comment: "How often discovery refreshes" }) },
  { id: "6h", label: msg({ message: "6 hours", comment: "How often discovery refreshes" }) },
  { id: "12h", label: msg({ message: "12 hours", comment: "How often discovery refreshes" }) },
  { id: "24h", label: msg({ message: "24 hours", comment: "How often discovery refreshes" }) },
  { id: "7d", label: msg({ message: "Weekly", comment: "How often discovery refreshes" }) },
];
const deepOptions = [
  ["customFolders", msg`Custom folders`, msg`Search only the custom paths listed below.`],
  ["externalDrives", msg`External drives`, msg`Search removable volumes reported by this device.`],
  [
    "portableExecutables",
    msg`Portable executables`,
    msg`Search Applications and Games inside your home directory.`,
  ],
  [
    "winePrefixes",
    msg`Wine and Proton prefixes`,
    msg`Look for compatibility prefixes inside your home directory.`,
  ],
  ["fullDisk", msg`Full disk scan`, msg`Search all reported volumes. This can be slow.`],
] as const;
/** The adapter's scan phases (`src-tauri/src/discovery`); others show as sent. */
const phases: Record<string, MessageDescriptor> = {
  idle: msg({ message: "Idle", context: "discovery phase" }),
  scanning: msg({ message: "Scanning", context: "discovery phase" }),
  importing: msg({ message: "Importing", context: "discovery phase" }),
  cancelling: msg({ message: "Cancelling", context: "discovery phase" }),
  complete: msg({ message: "Complete", context: "discovery phase" }),
  cancelled: msg({ message: "Cancelled", context: "discovery phase" }),
  failed: msg({ message: "Failed", context: "discovery phase" }),
};

export function LibraryDiscoverySettings() {
  const { t } = useLingui();
  const preferences = useSettingsSection<DiscoveryPreferences>("core.discovery");
  const launchers = useSettingsSection<{ enabled: Record<string, boolean> }>("core.launchers");
  const history = useSettingsSection<{ seedPlaytime: boolean }>("discovery.import");
  const discovery = useDiscovery();
  const [path, setPath] = useState("");
  const picker = usePathPicker();
  // A folder dropped on the field fills it in; Add folder still adds it.
  const drop = useDropZone<HTMLDivElement>((paths) => {
    if (paths[0]) setPath(paths[0]);
  });
  const settings = preferences.settings;
  if (!settings) return <SettingsLoading error={preferences.error} reload={preferences.reload} />;
  const snapshot = discovery.data;
  const phase = snapshot && phases[snapshot.phase];
  const busy = Boolean(scanning(snapshot) || discovery.action.isPending);
  const toggle = (
    key: "enabled" | "scanOnStartup" | "backgroundRefresh" | "watchFolders",
    enabled: boolean,
  ) => preferences.setSettings((s) => ({ ...s, [key]: enabled }));
  const error = discovery.error ?? discovery.action.error;
  return (
    <div data-settings="library-discovery" data-cs="discovery-settings" className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <SettingsFeedback
          error={preferences.error ?? launchers.error ?? history.error}
          saving={preferences.saving || launchers.saving || history.saving}
        />
        {/* Names what goes back and asks first (#44, #49). */}
        <ResetDefaults
          group={t({ message: "Library Discovery", context: "settings tab" })}
          label={t`Reset discovery settings`}
          parts={[
            {
              namespace: "core.discovery",
              whole: true,
              fields: [
                { key: "enabled", label: t`Enable library discovery` },
                { key: "scanOnStartup", label: t`Full scan on startup` },
                { key: "backgroundRefresh", label: t`Background refresh` },
                { key: "refreshInterval", label: t`Refresh interval` },
                { key: "watchFolders", label: t`Watch library folders` },
                { key: "osChecks", label: t`Operating-system checks` },
                { key: "deepScans", label: t`Optional deep scans` },
                { key: "scanPaths", label: t`Custom folders` },
                { key: "ignoredEntries", label: t`Ignored results` },
              ],
            },
          ]}
          keeps={t`Launchers, play history import and settings on other pages stay as they are.`}
          isDisabled={preferences.saving}
        />
      </div>
      <SettingsSection
        icon={ScanSearch}
        title={t`Automatic Library Discovery`}
        description={t`Read launcher and system metadata without running launcher commands.`}
      >
        <SettingRow
          label={t`Enable library discovery`}
          description={t`Controls manual scans, scheduled refresh and folder watching.`}
        >
          <Switch
            isSelected={settings.enabled}
            onChange={(enabled) => toggle("enabled", enabled)}
          />
        </SettingRow>
        <div className="mt-3 flex flex-wrap items-center gap-3">
          <Button
            variant="primary"
            isDisabled={!settings.enabled || busy}
            onPress={() => discovery.action.mutate({ type: "scan" })}
          >
            <Trans>Scan now</Trans>
          </Button>
          <Button
            isDisabled={!scanning(snapshot) || discovery.action.isPending}
            onPress={() => discovery.action.mutate({ type: "cancel" })}
          >
            <Trans>Cancel scan</Trans>
          </Button>
          <Button
            isDisabled={discovery.action.isPending}
            onPress={() => discovery.action.mutate({ type: "launchers" })}
          >
            <Trans>Refresh launcher detection</Trans>
          </Button>
          <p role="status" className="text-sm text-fg-muted">
            {snapshot
              ? [
                  phase ? t(phase) : snapshot.phase,
                  t({
                    message: `${{ found: snapshot.found }} groups found`,
                    comment: "Games discovery found, counting each game's copies once",
                  }),
                  t`${{ imported: snapshot.imported }} imported`,
                  snapshot.watching && t`Watching folders`,
                ]
                  .filter(Boolean)
                  .join(" · ")
              : t`Loading discovery status…`}
          </p>
        </div>
        {error && (
          <p role="alert" className="mt-3 text-sm text-danger">
            {error.message}
          </p>
        )}
        {snapshot?.error && (
          <p role="alert" className="mt-3 text-sm text-danger">
            {snapshot.error}
          </p>
        )}
      </SettingsSection>
      <fieldset disabled={!settings.enabled} className="space-y-4 disabled:opacity-50">
        <SettingsSection
          icon={RefreshCw}
          title={t`Scan behaviour`}
          description={t`A full scan reconciles missing records; incremental watching picks up local changes.`}
        >
          <SettingRow
            label={t`Full scan on startup`}
            description={t`Reconcile the library when Caskade starts. Watching may still perform its own initial check.`}
          >
            <Switch
              isSelected={settings.scanOnStartup}
              onChange={(v) => toggle("scanOnStartup", v)}
            />
          </SettingRow>
          <SettingRow
            label={t`Background refresh`}
            description={t`Run a full scan on the selected schedule.`}
          >
            <Select
              aria-label={t`Refresh interval`}
              value={settings.refreshInterval}
              options={intervals.map(({ id, label }) => ({ id, label: t(label) }))}
              isDisabled={!settings.backgroundRefresh}
              onChange={(v) => preferences.setSettings((s) => ({ ...s, refreshInterval: v }))}
            />
            <Switch
              isSelected={settings.backgroundRefresh}
              onChange={(v) => toggle("backgroundRefresh", v)}
            />
          </SettingRow>
          <SettingRow
            label={t`Watch library folders`}
            description={t`Monitor metadata paths, including locations for launchers installed later.`}
          >
            <Switch
              isSelected={settings.watchFolders}
              onChange={(v) => toggle("watchFolders", v)}
            />
          </SettingRow>
          {history.settings && (
            <SettingRow
              label={t`Import existing play history`}
              description={t`Seed known launcher playtime on first import, without duplicating later sessions.`}
            >
              <Switch
                isSelected={history.settings.seedPlaytime}
                onChange={(value) => history.setSettings((s) => ({ ...s, seedPlaytime: value }))}
              />
            </SettingRow>
          )}
        </SettingsSection>
        <SettingsSection
          title={t`Launchers and operating-system checks`}
          description={
            snapshot
              ? t({
                  message: `Checks use the backend ${{ os: snapshot.os }} profile.`,
                  comment: "{os} is the operating system's id: linux, windows or macos",
                })
              : t`Waiting for the backend device profile.`
          }
        >
          <LaunchersLink />
          <div className="my-3 grid gap-3 md:grid-cols-2">
            {[
              ...new Set(
                snapshot?.launchers.map((launcher) =>
                  launcher.source === "rare" ? "legendary" : launcher.source,
                ) ?? [],
              ),
            ].map((id) => {
              const matches =
                snapshot?.launchers.filter(
                  (launcher) =>
                    launcher.source === id || (id === "legendary" && launcher.source === "rare"),
                ) ?? [];
              return (
                <div key={id} className="rounded-control border border-border p-3">
                  <Checkbox
                    isSelected={launchers.settings?.enabled[id] !== false}
                    onChange={(enabled) =>
                      launchers.setSettings((s) => ({
                        ...s,
                        enabled: { ...s.enabled, [id]: enabled },
                      }))
                    }
                  >
                    {id}
                  </Checkbox>
                  <p className="mt-1 text-xs text-fg-muted">
                    {matches.some((launcher) => launcher.found)
                      ? t({ message: "Detected", comment: "A launcher was found on this device" })
                      : t({
                          message: "Not found",
                          comment: "A launcher wasn't found on this device",
                        })}
                  </p>
                  {matches
                    .filter((launcher) => launcher.found)
                    .map((launcher) => (
                      <p
                        key={`${launcher.path}:${launcher.version}`}
                        className="mt-1 break-all font-mono text-xs text-fg-subtle"
                      >
                        {launcher.path ? formatPath(launcher.path) : t`Location unknown`}
                        {launcher.version ? ` · ${launcher.version}` : ""}
                      </p>
                    ))}
                </div>
              );
            })}
          </div>
          <div className="space-y-3">
            {snapshot?.checks.map((check) => (
              <Checkbox
                key={check.id}
                isSelected={settings.osChecks[snapshot.os]?.[check.id] !== false}
                onChange={(enabled) =>
                  preferences.setSettings((s) => ({
                    ...s,
                    osChecks: {
                      ...s.osChecks,
                      [snapshot.os]: { ...s.osChecks[snapshot.os], [check.id]: enabled },
                    },
                  }))
                }
              >
                {check.label}
              </Checkbox>
            ))}
          </div>
        </SettingsSection>
        <SettingsSection
          icon={Folder}
          title={t`Optional deep scans`}
          description={t`All deep scans are off by default. Canonical identity merging always remains enabled.`}
        >
          {deepOptions.map(([key, label, description]) => (
            <SettingRow key={key} label={t(label)} description={t(description)}>
              <Switch
                isSelected={settings.deepScans[key]}
                onChange={(enabled) =>
                  preferences.setSettings((s) => ({
                    ...s,
                    deepScans: { ...s.deepScans, [key]: enabled },
                  }))
                }
              />
            </SettingRow>
          ))}
          <div ref={drop.ref} className="mt-4">
            <TextField
              label={t`Absolute custom folder`}
              value={path}
              onChange={setPath}
              mono
              description={t`Enter a full path for this device. No folders are created or modified.`}
              action={
                <span className="flex gap-2">
                  <Button
                    onPress={async () => {
                      const chosen = await picker.pick({
                        kind: "folder",
                        title: t`Choose a folder to scan`,
                        startAt: path.trim() || null,
                      });
                      // Cancel changes nothing.
                      if (chosen) setPath(chosen);
                    }}
                  >
                    <FolderOpen className="size-4" />
                    <Trans>Browse…</Trans>
                  </Button>
                  <Button
                    isDisabled={!path.trim() || (settings.scanPaths?.length ?? 0) >= 64}
                    onPress={() => {
                      const value = path.trim();
                      preferences.setSettings((s) => ({
                        ...s,
                        scanPaths: [...new Set([...(s.scanPaths ?? []), value])],
                      }));
                      setPath("");
                    }}
                  >
                    <Trans>Add folder</Trans>
                  </Button>
                </span>
              }
            />
            <ul className="mt-3 space-y-2">
              {settings.scanPaths?.map((folder) => (
                <li
                  key={folder}
                  className="flex items-center justify-between gap-3 rounded-control border border-border p-2"
                >
                  <span className="break-all font-mono text-xs">{folder}</span>
                  <Button
                    size="sm"
                    aria-label={t`Remove ${{ folder }}`}
                    onPress={() =>
                      preferences.setSettings((s) => ({
                        ...s,
                        scanPaths: s.scanPaths?.filter((p) => p !== folder),
                      }))
                    }
                  >
                    <Trans>Remove</Trans>
                  </Button>
                </li>
              ))}
            </ul>
          </div>
        </SettingsSection>
      </fieldset>
      {snapshot && (
        <>
          <SettingsSection
            title={t`Last source reports`}
            description={t`Completion counts, warnings and errors from the backend.`}
          >
            <ul className="mt-3 space-y-3">
              {snapshot.sources.map((source) => {
                const games = plural({ total: source.count }, { one: "# game", other: "# games" });
                return (
                  <li key={source.source} className="rounded-control border border-border p-3">
                    <p className="text-sm font-medium">
                      {t({
                        message: `${{ source: source.source }}: ${{ games }} · ${{ duration: source.durationMs }} ms`,
                        comment:
                          "A discovery source's last report: its name, the games it found and how long it took",
                      })}
                    </p>
                    {source.error && <p className="mt-1 text-sm text-danger">{source.error}</p>}
                    <ul className="mt-1 text-xs text-fg-muted">
                      {source.warnings.map((warning) => (
                        <li key={warning}>{warning}</li>
                      ))}
                    </ul>
                  </li>
                );
              })}
            </ul>
            {snapshot.warnings.map((warning) => (
              <p key={warning} className="mt-2 text-sm text-fg-muted">
                {warning}
              </p>
            ))}
          </SettingsSection>
          <ReviewList
            snapshot={snapshot}
            busy={busy}
            decide={(ids, accept) =>
              discovery.action.mutate({ type: "review", ids, accept, revision: snapshot.revision })
            }
          />
        </>
      )}
      <HiddenGames />
      <SettingsSection
        title={t`Ignored results`}
        description={t`These source records are excluded from later discovery scans.`}
      >
        <ul className="mt-3 space-y-2">
          {settings.ignoredEntries?.map((item) => (
            <li
              key={`${item.source}:${item.key}`}
              className="flex items-center justify-between gap-3"
            >
              <span className="break-all font-mono text-xs">
                {item.source}: {item.key}
              </span>
              <Button
                size="sm"
                onPress={() =>
                  preferences.setSettings((s) => ({
                    ...s,
                    ignoredEntries: s.ignoredEntries?.filter(
                      (entry) => entry.source !== item.source || entry.key !== item.key,
                    ),
                  }))
                }
              >
                <Trans>Allow again</Trans>
              </Button>
            </li>
          ))}
        </ul>
      </SettingsSection>
    </div>
  );
}

/** Launchers have their own tab too (#46); only where there is a router. */
function LaunchersLink() {
  const router = useRouter({ warn: false }) as ReturnType<typeof useRouter> | undefined;
  if (!router) return null;
  return (
    <p className="mt-1 text-sm text-fg-muted">
      <Trans>
        See also:{" "}
        <Link to="/settings" search={{ tab: "launchers" }} className="text-accent-alt">
          Launchers
        </Link>
      </Trans>
    </p>
  );
}
