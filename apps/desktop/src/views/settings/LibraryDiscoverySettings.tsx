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
import { usePathPicker } from "../../components/pathBrowser/PathPicker";
import { useDropZone } from "../../drop/DropProvider";
import { useDiscovery } from "./discovery/DiscoveryProvider";
import { HiddenGames } from "./discovery/HiddenGames";
import { ReviewList } from "./discovery/ReviewList";
import { useSourceName } from "./launchers";
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
  const history = useSettingsSection<{ seedPlaytime: boolean }>("discovery.import");
  const discovery = useDiscovery();
  const sourceName = useSourceName();
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
          error={preferences.error ?? history.error}
          saving={preferences.saving || history.saving}
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
        description={t`Finds the games your launchers and your system already know about. It reads their records and never starts a launcher.`}
      >
        <SettingRow
          label={t`Enable library discovery`}
          description={t`Off stops every scan, Scan now included, and watching folders.`}
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
          <p role="status" className="text-sm text-fg-muted">
            {snapshot
              ? [
                  phase ? t(phase) : snapshot.phase,
                  t({
                    message: `${{ found: snapshot.found }} games found`,
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
      {/* The decision first; the settings and reports below don't need one (#54). */}
      {snapshot && (
        <ReviewList
          snapshot={snapshot}
          busy={busy}
          decide={(ids, accept) =>
            discovery.action.mutate({ type: "review", ids, accept, revision: snapshot.revision })
          }
        />
      )}
      <fieldset disabled={!settings.enabled} className="space-y-4 disabled:opacity-50">
        <SettingsSection
          icon={RefreshCw}
          title={t`Scan behaviour`}
          description={t`A full scan looks everywhere again and notices games that were removed. Watching picks up changes as they happen.`}
        >
          <SettingRow
            label={t`Full scan on startup`}
            description={t`Look everywhere again each time Caskade starts.`}
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
            description={t`Notice games installed or removed while Caskade runs, in launchers installed later too.`}
          >
            <Switch
              isSelected={settings.watchFolders}
              onChange={(v) => toggle("watchFolders", v)}
            />
          </SettingRow>
          {history.settings && (
            <SettingRow
              label={t`Import existing play history`}
              description={t`When a game joins your library, bring in the play time its launcher recorded. Time is never counted twice.`}
            >
              <Switch
                isSelected={history.settings.seedPlaytime}
                onChange={(value) => history.setSettings((s) => ({ ...s, seedPlaytime: value }))}
              />
            </SettingRow>
          )}
        </SettingsSection>
        <SettingsSection
          title={t`Operating-system checks`}
          description={t`Games the system itself keeps a record of, besides launchers' own.`}
        >
          <LaunchersLink />
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
          description={t`Slower searches for games that no launcher knows about. Each is off until you turn it on.`}
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
              label={t`Folder to search`}
              value={path}
              onChange={setPath}
              mono
              description={
                settings.deepScans.customFolders
                  ? t`A full path on this device. Caskade only reads it.`
                  : t`A full path on this device. Folders are searched while Custom folders is on.`
              }
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
        <SettingsSection
          title={t`Last source reports`}
          description={t`What each source found in the last scan, and any problems.`}
        >
          <ul className="mt-3 space-y-3">
            {snapshot.sources.map((source) => {
              const games = plural({ total: source.count }, { one: "# game", other: "# games" });
              return (
                <li key={source.source} className="rounded-control border border-border p-3">
                  <p className="text-sm font-medium">
                    {t({
                      message: `${{ source: sourceName(source.source) }}: ${{ games }}`,
                      comment: "A discovery source's last report: its name and the games it found",
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
      )}
      <HiddenGames />
      <SettingsSection
        title={t`Ignored results`}
        description={t`Games you chose to ignore. Scans leave them out of your library.`}
      >
        {!settings.ignoredEntries?.length && (
          <p className="mt-3 text-sm text-fg-muted">
            <Trans>Nothing ignored.</Trans>
          </p>
        )}
        <ul className="mt-3 space-y-2">
          {settings.ignoredEntries?.map((item) => (
            <li
              key={`${item.source}:${item.key}`}
              className="flex items-center justify-between gap-3"
            >
              {/* Decisions from before titles were kept show the record's key. */}
              <span className="min-w-0">
                <span className="block text-sm">{item.title ?? item.key}</span>
                <span className="block text-xs text-fg-muted">{sourceName(item.source)}</span>
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
        Which launchers are scanned:{" "}
        <Link to="/settings" search={{ tab: "launchers" }} className="text-accent-alt">
          Launchers
        </Link>
      </Trans>
    </p>
  );
}
