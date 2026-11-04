import {
  Button,
  Checkbox,
  SegmentedControl,
  Select,
  SettingRow,
  SettingsSection,
  StatusDot,
  Switch,
  TextField,
} from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import {
  Clock,
  KeyRound,
  Plug,
  Power,
  RefreshCw,
  Save,
  Server,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import type { EmulationSnapshot } from "../../../api/emulation";
import { errorMessage } from "../../../api/reply";
import {
  authKinds,
  type RommAuth,
  type RommConnection,
  type RommMapping,
  type RommServer,
  type RommServerStatus,
  type RommSyncReport,
  syncIntervals,
} from "../../../api/romm";
import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { useRommApi, useRommWrite } from "./useRomm";

const authLabels: Record<RommAuth, MessageDescriptor> = {
  password: msg({
    message: "Username & password",
    comment: "How Caskade signs in to a RomM server",
  }),
  token: msg({ message: "API token", comment: "How Caskade signs in to a RomM server" }),
};

const intervalLabels: Record<(typeof syncIntervals)[number]["id"], MessageDescriptor> = {
  manual: msg({ message: "Only when I ask", comment: "How often a RomM server syncs" }),
  "1h": msg({ message: "Every hour", comment: "How often a RomM server syncs" }),
  "6h": msg({ message: "Every 6 hours", comment: "How often a RomM server syncs" }),
  "24h": msg({ message: "Daily", comment: "How often a RomM server syncs" }),
};

/** Small rounded label: a RomM platform slug. */
function Chip({ mono = false, children }: { mono?: boolean; children: ReactNode }) {
  return (
    <span
      data-part="chip"
      className={`rounded-full bg-surface-hover px-2 py-0.5 text-xs whitespace-nowrap text-fg-muted ${mono ? "font-mono text-[11px]" : ""}`}
    >
      {children}
    </span>
  );
}

/** One RomM platform, as the server reported it or as last saved. */
interface PlatformRow {
  slug: string;
  name: string;
  romCount: number | null;
  suggested: string | null;
}

function rows(server: RommServer, report: RommConnection | null): PlatformRow[] {
  if (report) {
    // Platforms without ROMs have nothing to import.
    return report.platforms
      .filter(({ platform }) => platform.romCount > 0)
      .map(({ platform, suggested }) => ({
        slug: platform.slug,
        name: platform.customName || platform.name,
        romCount: platform.romCount,
        suggested,
      }));
  }
  return server.platforms.map((mapping) => ({
    slug: mapping.slug,
    name: mapping.slug,
    romCount: null,
    suggested: null,
  }));
}

const mappingOf = (server: RommServer, slug: string): RommMapping =>
  server.platforms.find((mapping) => mapping.slug === slug) ?? {
    slug,
    enabled: true,
    platform: null,
    emulatorId: null,
    profileId: null,
  };

/** "Launches with": the platform's default, or any profile that runs it. */
function launchOptions(
  platform: string | null,
  emulation: EmulationSnapshot | undefined,
  defaultLabel: string,
) {
  const options = [{ id: "default", label: defaultLabel }];
  if (!platform || !emulation) return options;
  for (const emulator of emulation.emulators) {
    for (const { config, resolved } of emulator.profiles) {
      if (resolved.platforms.includes(platform)) {
        options.push({
          id: `${emulator.id}:${config.id}`,
          label: `${emulator.name} · ${resolved.name}`,
        });
      }
    }
  }
  return options;
}

function Connection({
  status,
  busy,
  report,
  error,
}: {
  status: RommServerStatus | undefined;
  busy: boolean;
  report: RommConnection | null;
  error: string | null;
}) {
  const failed = (
    <StatusDot tone="danger">
      <Trans comment="A RomM server's status">Can't connect</Trans>
    </StatusDot>
  );
  if (busy)
    return (
      <StatusDot tone="warning">
        <Trans comment="A RomM server's status">Connecting…</Trans>
      </StatusDot>
    );
  if (error) return failed;
  const version = report?.version ?? status?.version;
  if (report || status?.state === "connected")
    return (
      <StatusDot tone="ok">
        {version ? (
          <Trans comment="A RomM server's status, with the server's version">
            Connected · v{version}
          </Trans>
        ) : (
          <Trans comment="A RomM server's status">Connected</Trans>
        )}
      </StatusDot>
    );
  if (status?.state === "offline")
    return (
      <StatusDot tone="warning">
        <Trans comment="A RomM server's status">Unreachable</Trans>
      </StatusDot>
    );
  if (status?.state === "error") return failed;
  return (
    <StatusDot tone="off">
      <Trans comment="A RomM server's status: its connection hasn't been tried">Not tested</Trans>
    </StatusDot>
  );
}

/** A server's address, sign-in, what it imports and how it syncs. */
export function ServerEditor({
  initial,
  status,
  isNew,
  emulation,
  onSaved,
  onRemoved,
}: {
  initial: RommServer;
  status: RommServerStatus | undefined;
  isNew: boolean;
  emulation: EmulationSnapshot | undefined;
  onSaved: (server: RommServer) => void;
  onRemoved: () => void;
}) {
  const { t } = useLingui();
  const api = useRommApi();
  const write = useRommWrite();
  const [server, setServer] = useState(initial);
  const [secret, setSecret] = useState("");
  const [report, setReport] = useState<RommConnection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [synced, setSynced] = useState<RommSyncReport | null>(null);
  const set = <K extends keyof RommServer>(key: K, value: RommServer[K]) =>
    setServer((old) => ({ ...old, [key]: value }));
  const setMapping = (slug: string, patch: Partial<RommMapping>) =>
    setServer((old) => {
      const next = { ...mappingOf(old, slug), ...patch };
      const exists = old.platforms.some((mapping) => mapping.slug === slug);
      return {
        ...old,
        platforms: exists
          ? old.platforms.map((mapping) => (mapping.slug === slug ? next : mapping))
          : [...old.platforms, next],
      };
    });

  const test = async () => {
    setTesting(true);
    setError(null);
    try {
      setReport(await api.testServer(server, secret || null));
    } catch (failure) {
      setReport(null);
      setError(errorMessage(failure));
    } finally {
      setTesting(false);
    }
  };
  const save = () => {
    setError(null);
    write.mutate((api) => api.saveServer(server, secret || null), {
      onSuccess: () => {
        setSecret("");
        onSaved(server);
      },
      onError: (failure) => setError(errorMessage(failure)),
    });
  };
  const sync = async () => {
    setSyncing(true);
    setError(null);
    try {
      const [result] = await api.sync(server.id);
      setSynced(result ?? null);
      write.mutate((api) => api.overview());
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setSyncing(false);
    }
  };
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const remove = () => {
    if (isNew) {
      onRemoved();
      return;
    }
    // It takes the server's games and downloads with it (#49). A save's
    // failure shows above; only the removal's shows in the dialog.
    write.reset();
    setConfirmingRemove(true);
  };

  const platforms = rows(server, report);
  const platformName = (id: string | null) =>
    emulation?.platforms.find((platform) => platform.id === id)?.name ?? id;
  const unchanged = !isNew && JSON.stringify(server) === JSON.stringify(initial) && !secret;
  const romsOnServer = (report?.platforms ?? []).reduce(
    (sum, row) => sum + row.platform.romCount,
    0,
  );
  let connection: string = status?.state ?? "untested";
  if (error) connection = "error";

  return (
    <SettingsSection
      icon={Server}
      title={server.name || (isNew ? t`New server` : t`Unnamed server`)}
      description={t`ROMs on this server appear in your library. They download the first time you play them.`}
      aside={
        <Button variant="danger" size="sm" icon={Trash2} onPress={remove}>
          {isNew
            ? t({ message: "Discard", comment: "Drops a RomM server that isn't saved yet" })
            : t({ message: "Remove", comment: "Removes a RomM server" })}
        </Button>
      }
    >
      <div className="mt-3 grid gap-4 md:grid-cols-2">
        <TextField
          label={t({ message: "Name", context: "RomM server" })}
          value={server.name}
          onChange={(value) => set("name", value)}
        />
        <TextField
          label={t`Server address`}
          mono
          placeholder="https://romm.example.com"
          value={server.url}
          onChange={(value) => set("url", value)}
        />
        <div className="flex flex-col gap-1.5">
          <span id={`romm-${server.id}-auth`} className="text-sm font-medium">
            <Trans comment="Followed by the ways to sign in to a RomM server">Sign in with</Trans>
          </span>
          <SegmentedControl
            aria-labelledby={`romm-${server.id}-auth`}
            options={authKinds.map(({ id }) => ({ id, label: t(authLabels[id]) }))}
            value={server.auth}
            onChange={(value) => {
              set("auth", value);
              setSecret("");
            }}
            className="self-start"
          />
        </div>
        <div className="grid gap-4 sm:grid-cols-2">
          {server.auth === "password" && (
            <TextField
              label={t`Username`}
              value={server.username}
              onChange={(value) => set("username", value)}
              autoComplete="username"
            />
          )}
          <TextField
            label={server.auth === "password" ? t`Password` : t(authLabels.token)}
            type="password"
            icon={KeyRound}
            placeholder={status?.hasSecret ? t`Saved in keychain` : ""}
            value={secret}
            onChange={setSecret}
            className={server.auth === "token" ? "sm:col-span-2" : ""}
          />
        </div>
      </div>

      <div className="mt-2">
        <SettingRow
          icon={ShieldAlert}
          label={t`Allow self-signed certificates`}
          description={t`For a server on your home network without a public certificate.`}
        >
          <Switch
            isSelected={server.allowInvalidCerts}
            onChange={(value) => set("allowInvalidCerts", value)}
          />
        </SettingRow>
      </div>

      <div
        data-part="connection"
        data-status={connection}
        className="mt-1 flex flex-wrap items-center gap-3 rounded-control border border-border bg-surface-overlay/50 p-3 data-[status=error]:border-danger/40 data-[status=error]:bg-danger/5"
      >
        <Button icon={Plug} onPress={() => void test()} isDisabled={testing || !server.url}>
          {testing
            ? t({ message: "Connecting…", comment: "A RomM server's status" })
            : t`Test connection`}
        </Button>
        <Connection status={status} busy={testing} report={report} error={error} />
        {error && (
          <span role="alert" className="text-xs text-danger">
            {error}
          </span>
        )}
        {report && (
          <span className="text-xs text-fg-muted">
            {plural(platforms.length, { one: "# platform", other: "# platforms" })} ·{" "}
            {plural(
              { roms: romsOnServer },
              { one: "# ROM on the server", other: "# ROMs on the server" },
            )}
          </span>
        )}
        <span className="ml-auto flex items-center gap-1.5 text-xs text-fg-muted">
          <KeyRound className="size-3.5" />{" "}
          <Trans>Credentials are kept in the system keychain.</Trans>
        </span>
      </div>

      <div className="mt-6 flex flex-wrap items-center gap-3">
        <h3 className="font-display font-semibold tracking-wide">
          <Trans context="RomM server">Platforms</Trans>
        </h3>
        <Switch
          size="sm"
          isSelected={server.importAll}
          onChange={(value) => set("importAll", value)}
          className="ml-auto"
        >
          <Trans>Import everything, including platforms added later</Trans>
        </Switch>
      </div>
      {platforms.length === 0 ? (
        <p className="mt-3 rounded-control border border-dashed border-border-strong p-6 text-center text-sm text-fg-muted">
          {report
            ? t`This server has no ROMs yet.`
            : t`Test the connection to see this server's platforms.`}
        </p>
      ) : (
        <div className="mt-3 overflow-x-auto rounded-control border border-border">
          <table className="w-full text-left text-sm">
            <thead className="bg-surface-overlay font-mono text-[11px] tracking-wider text-fg-subtle uppercase">
              <tr>
                <th className="w-10 px-3 py-2.5">
                  <span className="sr-only">
                    <Trans context="RomM platform column">Import</Trans>
                  </span>
                </th>
                <th className="px-3 py-2.5 font-medium">
                  <Trans context="RomM platform column">RomM platform</Trans>
                </th>
                <th className="px-3 py-2.5 text-right font-medium">
                  <Trans context="RomM platform column">ROMs</Trans>
                </th>
                <th className="px-3 py-2.5 font-medium">
                  <Trans context="RomM platform column">Caskade platform</Trans>
                </th>
                <th className="px-3 py-2.5 font-medium">
                  <Trans context="RomM platform column">Launches with</Trans>
                </th>
              </tr>
            </thead>
            <tbody>
              {platforms.map((row) => {
                const mapping = mappingOf(server, row.slug);
                const platform = mapping.platform ?? row.suggested;
                const on = !!platform && (server.importAll || mapping.enabled);
                const launch =
                  mapping.emulatorId && mapping.profileId
                    ? `${mapping.emulatorId}:${mapping.profileId}`
                    : "default";
                return (
                  <tr
                    key={row.slug}
                    data-part="romm-platform"
                    data-enabled={on || undefined}
                    data-unmatched={!platform || undefined}
                    className="border-t border-border"
                  >
                    <td className="px-3 py-2">
                      <Checkbox
                        aria-label={t`Import ${{ platform: row.name }}`}
                        isSelected={on}
                        isDisabled={server.importAll || !platform}
                        onChange={(enabled) => setMapping(row.slug, { enabled })}
                      />
                    </td>
                    <td className="px-3 py-2">
                      <div className="font-medium">{row.name}</div>
                      <Chip mono>{row.slug}</Chip>
                    </td>
                    <td className="px-3 py-2 text-right font-mono text-xs text-fg-muted">
                      {row.romCount ?? "—"}
                    </td>
                    <td className="px-3 py-2">
                      <Select
                        aria-label={t`Caskade platform for ${{ platform: row.name }}`}
                        options={[
                          {
                            id: "auto",
                            label: row.suggested
                              ? t({
                                  message: `Automatic (${{ platform: platformName(row.suggested) }})`,
                                  comment: "The Caskade platform matched to a RomM platform",
                                })
                              : t({
                                  message: "Not matched",
                                  comment: "No Caskade platform matches a RomM platform",
                                }),
                          },
                          ...(emulation?.platforms ?? []).map((option) => ({
                            id: option.id,
                            label: option.name,
                          })),
                        ]}
                        value={mapping.platform ?? "auto"}
                        onChange={(value) =>
                          setMapping(row.slug, {
                            platform: value === "auto" ? null : value,
                            emulatorId: null,
                            profileId: null,
                          })
                        }
                        className="w-56"
                      />
                    </td>
                    <td className="px-3 py-2">
                      {platform ? (
                        <Select
                          aria-label={t`Emulator for ${{ platform: row.name }}`}
                          options={launchOptions(platform, emulation, t`Platform default`)}
                          value={launch}
                          onChange={(value) => {
                            const [emulatorId, profileId] =
                              value === "default" ? [null, null] : value.split(":");
                            setMapping(row.slug, {
                              emulatorId: emulatorId ?? null,
                              profileId: profileId ?? null,
                            });
                          }}
                          isDisabled={!on}
                          className="w-full max-w-96 min-w-64"
                        />
                      ) : (
                        <span className="text-xs text-fg-muted">
                          <Trans>Match a platform to import these ROMs.</Trans>
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {report && report.collections.length > 0 && (
        <div className="mt-6">
          <h3 className="font-display font-semibold tracking-wide">
            <Trans context="RomM server">Collections</Trans>
          </h3>
          <p className="text-xs text-fg-muted">
            <Trans>Their ROMs are imported even when their platform is off.</Trans>
          </p>
          <div className="mt-3 flex flex-wrap gap-x-6 gap-y-2">
            {report.collections.map((collection) => (
              <Checkbox
                key={collection.id}
                isSelected={server.importAll || server.collections.includes(collection.id)}
                isDisabled={server.importAll}
                onChange={(selected) =>
                  set(
                    "collections",
                    selected
                      ? [...server.collections, collection.id]
                      : server.collections.filter((id) => id !== collection.id),
                  )
                }
              >
                {collection.name}
                <span className="font-mono text-xs text-fg-muted">{collection.romIds.length}</span>
              </Checkbox>
            ))}
          </div>
        </div>
      )}

      <div className="mt-6 grid gap-x-8 xl:grid-cols-2">
        <SettingRow
          icon={Power}
          label={t`Sync on startup`}
          description={t`Pick up new and changed ROMs when Caskade opens.`}
        >
          <Switch
            isSelected={server.syncOnStartup}
            onChange={(value) => set("syncOnStartup", value)}
          />
        </SettingRow>
        <SettingRow icon={Clock} label={t`Sync in the background`}>
          <Select
            options={syncIntervals.map(({ id }) => ({ id, label: t(intervalLabels[id]) }))}
            value={
              syncIntervals.find((option) => option.id === server.syncInterval)?.id ?? "manual"
            }
            onChange={(value) => set("syncInterval", value)}
            className="w-44"
          />
        </SettingRow>
      </div>

      <div className="mt-4 flex flex-wrap items-center gap-3">
        <Button
          variant="primary"
          icon={Save}
          onPress={save}
          isDisabled={write.isPending || unchanged || !server.url}
        >
          {isNew ? t`Save server` : t`Save changes`}
        </Button>
        {!isNew && (
          <Button icon={RefreshCw} onPress={() => void sync()} isDisabled={syncing}>
            {syncing ? t({ message: "Syncing…", comment: "A RomM server's status" }) : t`Sync now`}
          </Button>
        )}
        {synced && (
          <span data-part="sync-report" className="text-xs text-fg-muted">
            {t({
              message: `${{ added: synced.added }} added · ${{ updated: synced.updated }} updated · ${{ removed: synced.removed }} removed`,
              comment: "What a RomM sync changed: counts of games",
            })}
            {synced.unmatchedPlatforms.length > 0 && (
              <>
                {" · "}
                {t({
                  message: `no Caskade platform for ${{ platforms: synced.unmatchedPlatforms.join(", ") }}`,
                  comment:
                    "After a RomM sync's counts: RomM platforms, by slug, that match none of Caskade's",
                })}
              </>
            )}
          </span>
        )}
      </div>
      {confirmingRemove && (
        <ConfirmDialog
          title={t`Remove ${{ server: initial.name || t`Unnamed server` }}?`}
          confirmLabel={t({ message: "Remove", comment: "Removes a RomM server" })}
          pendingLabel={t`Removing…`}
          isPending={write.isPending}
          error={write.error}
          onConfirm={() =>
            write.mutate((api) => api.removeServer(server.id), {
              onSuccess: () => {
                setConfirmingRemove(false);
                onRemoved();
              },
            })
          }
          close={() => {
            write.reset();
            setConfirmingRemove(false);
          }}
        >
          <p>
            <Trans>
              Its games leave your library, the ROMs downloaded from it are deleted and its sign-in
              is forgotten. Devices that sync RomM servers remove it too.
            </Trans>
          </p>
          <p>
            <Trans>Adding it again brings its games back, to download again.</Trans>
          </p>
        </ConfirmDialog>
      )}
    </SettingsSection>
  );
}
