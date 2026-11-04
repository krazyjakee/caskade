import { Button, SettingsSection, StatusDot } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Download, Info, Pencil, Play, Plus, RefreshCw, Server, X } from "lucide-react";
import { useState } from "react";
import { Button as AriaButton } from "react-aria-components";
import { formatDateTime, formatNumber } from "../../../api/presentation";
import { errorMessage } from "../../../api/reply";
import {
  emptyServer,
  type RommDownload,
  type RommServer,
  type RommServerStatus,
  serverId,
} from "../../../api/romm";
import { formatBytes } from "../../../api/storage";
import { useActionMenus } from "../../../components/ActionMenu";
import { FailureNotice } from "../../../components/FailureNotice";
import { SectionSync } from "../devices/SectionSync";
import { useEmulation } from "../emulators/EmulationProvider";
import { CacheSection } from "./CacheSection";
import { ServerEditor } from "./ServerEditor";
import { useRommApi, useRommOverview, useRommWrite } from "./useRomm";

function ServerState({ status }: { status: RommServerStatus | undefined }) {
  if (status?.syncing)
    return (
      <StatusDot tone="warning">
        <Trans comment="A RomM server's status">Syncing…</Trans>
      </StatusDot>
    );
  switch (status?.state) {
    case "connected": {
      const version = status.version;
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
    }
    case "offline":
      return (
        <StatusDot tone="warning">
          <Trans comment="A RomM server's status">Unreachable</Trans>
        </StatusDot>
      );
    case "error":
      return (
        <StatusDot tone="danger">
          <Trans comment="A RomM server's status">Can't connect</Trans>
        </StatusDot>
      );
    default:
      return (
        <StatusDot tone="off">
          <Trans comment="A RomM server's status: its connection hasn't been tried">
            Not tested
          </Trans>
        </StatusDot>
      );
  }
}

const phases: Record<RommDownload["phase"], MessageDescriptor> = {
  downloading: msg({ message: "Downloading", context: "download status" }),
  verifying: msg({ message: "Verifying", context: "download status" }),
  unpacking: msg({ message: "Unpacking", context: "download status" }),
  done: msg({ message: "Done", context: "download status" }),
  failed: msg({ message: "Failed", context: "download status" }),
  cancelled: msg({ message: "Cancelled", context: "download status" }),
};

/** Downloads started by Play, with their progress. */
function Downloads({ downloads }: { downloads: RommDownload[] }) {
  const { t } = useLingui();
  const api = useRommApi();
  const menus = useActionMenus();
  const [cancelError, setCancelError] = useState<string | null>(null);
  if (downloads.length === 0) return null;
  const cancel = (entryId: string) => {
    setCancelError(null);
    api.cancelDownload(entryId).catch((error: unknown) => setCancelError(errorMessage(error)));
  };
  return (
    <SettingsSection icon={Download} title={t({ message: "Downloads", context: "RomM" })}>
      {cancelError && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {cancelError}
        </p>
      )}
      <ul aria-label={t`RomM downloads`} className="mt-3 flex flex-col gap-3">
        {downloads.map((download) => {
          const percent = download.total
            ? Math.min(100, Math.round((download.received / download.total) * 100))
            : null;
          const shown =
            percent !== null && download.phase === "downloading" ? ` · ${percent}%` : "";
          return (
            <li
              key={download.entryId}
              {...menus.target(download.title, [
                download.phase !== "failed" && {
                  id: "cancel",
                  label: t`Cancel download`,
                  icon: X,
                  tone: "danger",
                  onAction: () => cancel(download.entryId),
                },
              ])}
              data-part="romm-download"
              data-status={download.phase}
              className="flex flex-col gap-1.5"
            >
              <div className="flex items-center gap-3 text-sm">
                <span className="font-medium">{download.title}</span>
                <span className="font-mono text-xs text-fg-subtle">
                  {t(phases[download.phase])}
                  {shown} · {formatBytes(download.received)}
                </span>
                {download.error && <span className="text-xs text-danger">{download.error}</span>}
                {download.phase !== "failed" && (
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={X}
                    className="ml-auto"
                    onPress={() => cancel(download.entryId)}
                  >
                    <Trans>Cancel</Trans>
                  </Button>
                )}
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-surface-base/80 ring-1 ring-border ring-inset">
                <div className="h-full bg-accent" style={{ width: `${percent ?? 0}%` }} />
              </div>
            </li>
          );
        })}
      </ul>
      {menus.menu}
    </SettingsSection>
  );
}

/** Settings → Emulation → RomM Server (#1): remote ROM libraries. */
export function RommPanel() {
  const { t } = useLingui();
  const overview = useRommOverview();
  const emulation = useEmulation();
  const api = useRommApi();
  const write = useRommWrite();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<RommServer | null>(null);
  const [syncing, setSyncing] = useState<string | null>(null);
  // A sync that fails before the core records it on the server's row.
  const [syncErrors, setSyncErrors] = useState<Record<string, string>>({});
  // A right click on a server offers its actions.
  const menus = useActionMenus();

  if (overview.isPending)
    return (
      <p className="text-sm text-fg-subtle">
        <Trans>Loading RomM servers…</Trans>
      </p>
    );
  if (overview.isError) {
    return (
      <FailureNotice
        variant="inline"
        error={overview.error}
        title={t`RomM servers couldn't be loaded`}
        onRetry={() => void overview.refetch()}
      />
    );
  }
  const data = overview.data;
  const servers = data.settings.servers;
  const status = (id: string) => data.servers.find((server) => server.id === id);
  const lastSync = (state: RommServerStatus | undefined) =>
    state?.lastSyncMs
      ? formatDateTime(state.lastSyncMs)
      : t({ message: "Not this session", comment: "A RomM server's last sync: none yet" });
  const selected = draft ?? servers.find((server) => server.id === selectedId) ?? servers[0];
  const add = () =>
    setDraft(
      emptyServer(
        serverId(
          "romm",
          servers.map((server) => server.id),
        ),
      ),
    );
  const sync = (id: string) => {
    setSyncing(id);
    setSyncErrors(({ [id]: _, ...rest }) => rest);
    void api
      .sync(id)
      .catch((error: unknown) =>
        setSyncErrors((errors) => ({ ...errors, [id]: errorMessage(error) })),
      )
      .finally(() => {
        setSyncing(null);
        write.mutate((api) => api.overview());
      });
  };

  return (
    <div data-cs="romm" className="flex flex-col gap-4">
      <SettingsSection
        icon={Server}
        title={t`RomM Servers`}
        description={t`Connect a RomM server to add its ROMs to your library. Each game downloads when you first play it and runs with your emulators.`}
        aside={
          <Button size="sm" icon={Plus} onPress={add} isDisabled={draft !== null}>
            <Trans>Add Server</Trans>
          </Button>
        }
      >
        {servers.length === 0 && !draft ? (
          <div className="mt-3 flex flex-col items-center gap-3 rounded-control border border-dashed border-border-strong p-8 text-center">
            <Server className="size-8 text-fg-subtle" />
            <p className="max-w-md text-sm text-fg-muted">
              <Trans>
                RomM is a self-hosted ROM manager. Connect yours and every ROM on it shows up in
                your library, with artwork, without downloading anything up front.
              </Trans>
            </p>
            <Button variant="primary" icon={Plus} onPress={add}>
              <Trans>Connect a Server</Trans>
            </Button>
          </div>
        ) : (
          servers.length > 0 && (
            <div className="mt-3 overflow-x-auto rounded-control border border-border">
              <table className="w-full text-left text-sm">
                <thead className="bg-surface-overlay font-mono text-[11px] tracking-wider text-fg-subtle uppercase">
                  <tr>
                    <th className="px-4 py-2.5 font-medium">
                      <Trans context="RomM server column">Server</Trans>
                    </th>
                    <th className="px-4 py-2.5 font-medium">
                      <Trans context="RomM server column">Status</Trans>
                    </th>
                    <th className="px-4 py-2.5 text-right font-medium">
                      <Trans context="RomM server column">In library</Trans>
                    </th>
                    <th className="px-4 py-2.5 font-medium">
                      <Trans context="RomM server column">Last sync</Trans>
                    </th>
                    <th className="px-4 py-2.5">
                      <span className="sr-only">
                        <Trans context="RomM server column">Actions</Trans>
                      </span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {servers.map((server) => {
                    const state = status(server.id);
                    const busy = syncing === server.id || !!state?.syncing;
                    const active = !draft && server.id === selected?.id;
                    return (
                      <tr
                        key={server.id}
                        {...menus.target(server.name || server.url, [
                          {
                            id: "sync",
                            label: t`Sync now`,
                            icon: RefreshCw,
                            isDisabled: busy,
                            onAction: () => sync(server.id),
                          },
                          {
                            id: "edit",
                            label: t`Edit server`,
                            icon: Pencil,
                            onAction: () => {
                              setDraft(null);
                              setSelectedId(server.id);
                            },
                          },
                        ])}
                        data-part="romm-server"
                        data-status={state?.state ?? "untested"}
                        data-selected={active || undefined}
                        className="border-t border-border data-selected:bg-accent/10"
                      >
                        <td className="px-4 py-2.5">
                          <AriaButton
                            onPress={() => {
                              setDraft(null);
                              setSelectedId(server.id);
                            }}
                            aria-pressed={active}
                            className="rounded-[4px] text-left font-medium data-focus-visible:focus-ring data-hovered:text-accent-alt"
                          >
                            {server.name ||
                              t({ message: "Unnamed", comment: "A RomM server without a name" })}
                          </AriaButton>
                          <div className="font-mono text-xs text-fg-subtle">{server.url}</div>
                        </td>
                        <td className="px-4 py-2.5">
                          <ServerState status={state} />
                          {state?.error && (
                            <div className="mt-1 text-xs text-fg-subtle">{state.error}</div>
                          )}
                          {syncErrors[server.id] && (
                            <div role="alert" className="mt-1 text-xs text-danger">
                              {syncErrors[server.id]}
                            </div>
                          )}
                        </td>
                        <td className="px-4 py-2.5 text-right font-mono">
                          {formatNumber(state?.games ?? 0)}
                        </td>
                        <td className="px-4 py-2.5 text-xs whitespace-nowrap text-fg-muted">
                          {lastSync(state)}
                        </td>
                        <td className="px-4 py-2.5 text-right">
                          <Button
                            size="sm"
                            icon={busy ? RefreshCw : Play}
                            isDisabled={busy}
                            onPress={() => sync(server.id)}
                            aria-label={t`Sync ${{ server: server.name || server.url }}`}
                          >
                            <Trans>Sync Now</Trans>
                          </Button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )
        )}
        {menus.menu}
        {write.error && (
          <p role="alert" className="mt-3 text-sm text-danger">
            {write.error.message}
          </p>
        )}
        <p className="mt-3 flex gap-2 text-xs text-fg-subtle">
          <Info className="size-4 shrink-0" />
          <Trans>
            Games from a server you can't reach stay in your library. Downloaded ones still play;
            the rest show as unavailable until the server is back.
          </Trans>
        </p>
        <SectionSync namespace="core.romm" />
      </SettingsSection>

      {selected && (
        <ServerEditor
          key={selected.id + (draft ? ":new" : "")}
          initial={selected}
          status={draft ? undefined : status(selected.id)}
          isNew={draft !== null}
          emulation={emulation.data}
          onSaved={(server) => {
            setDraft(null);
            setSelectedId(server.id);
          }}
          onRemoved={() => {
            setDraft(null);
            setSelectedId(null);
          }}
        />
      )}

      <Downloads downloads={data.downloads} />
      <CacheSection overview={data} />
    </div>
  );
}
