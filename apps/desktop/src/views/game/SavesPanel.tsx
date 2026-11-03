/** A game's saves on its page (#241): the versions kept, which device and
 * account made each and when, restoring one, where the saves live here,
 * and the choice when two devices both played. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { FolderOpen } from "lucide-react";
import { useState } from "react";
import { useAccounts } from "../../api/accounts";
import { useGame } from "../../api/hooks";
import { formatBytes, formatDateTime } from "../../api/presentation";
import { useOptionalLibraryApi } from "../../api/provider";
import {
  useSaveHistory,
  useSaveLocations,
  type VersionReason,
  type VersionState,
} from "../../api/saves";
import { useSaveDeviceName } from "../../components/saves/devices";
import { SaveConflictDialog } from "../../components/saves/SaveConflictDialog";
import { ShareGameDialog } from "../../components/shares/ShareGameDialog";
import { useMode } from "../../shell/console/mode";

/** `entryId` is the release Play starts: restores go into its save
 * folders. `running`: a restore waits until the game has closed. */
export function SavesPanel({
  gameId,
  entryId,
  running,
}: {
  gameId: string;
  entryId: string | undefined;
  running: boolean;
}) {
  const { t } = useLingui();
  const { query, restore, choose, available } = useSaveHistory(gameId);
  const locations = useSaveLocations(entryId).query.data?.locations ?? [];
  const deviceName = useSaveDeviceName();
  const accountName = useAccountName();
  const reasonLabel = useReasonLabel();
  const [choosing, setChoosing] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  // A version shared with someone else through a link (#248).
  const [sharing, setSharing] = useState<string | null>(null);
  const title = useGame(gameId).data?.title ?? "";
  if (!available || !query.data) return null;
  const history = query.data;
  if (history.versions.length === 0 && locations.length === 0) return null;
  const conflict = history.versions.filter((each) => history.conflict.includes(each.version.id));
  const busy = restore.isPending || choose.isPending;
  const canRestore = (each: VersionState) =>
    Boolean(entryId) && !running && !busy && !each.current && each.complete;
  return (
    <section data-cs="saves-panel" className="mt-8" aria-labelledby="saves-heading">
      <h2 id="saves-heading" className="font-display text-xl font-semibold">
        <Trans>Saves</Trans>
      </h2>
      {!history.syncing && (
        <p className="mt-1 text-sm text-fg-muted">
          <Trans>Saves stay on this device: syncing them is off in Settings → Devices.</Trans>
        </p>
      )}
      {conflict.length > 1 && (
        <div
          data-part="conflict"
          className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-panel border border-warning/50 bg-warning/10 p-4 text-sm"
        >
          <p>
            <Trans>
              This game was played on more than one device since they last shared saves. Choose
              which save to play with; the other is kept.
            </Trans>
          </p>
          <Button
            data-spatial="focusable"
            isDisabled={!entryId || running || busy}
            onPress={() => setChoosing(true)}
          >
            <Trans>Choose a save…</Trans>
          </Button>
        </div>
      )}
      <LocationList entryId={entryId} count={locations.length} paths={locations} />
      {history.versions.length === 0 ? (
        <p className="mt-4 text-sm text-fg-muted">
          <Trans>No saves kept yet. A version is kept each time you finish playing.</Trans>
        </p>
      ) : (
        <ul data-part="versions" className="mt-4 flex flex-col gap-2">
          {history.versions.map((each) => {
            const version = each.version;
            const who = [deviceName(version), accountName(version.accountId)]
              .filter(Boolean)
              .join(" · ");
            return (
              <li
                key={version.id}
                data-part="version"
                data-status={each.current ? "current" : each.complete ? "kept" : "arriving"}
                className="flex flex-wrap items-center justify-between gap-3 rounded-panel border border-border bg-surface-raised p-3"
              >
                <div className="min-w-0">
                  <p className="text-sm font-medium">
                    {formatDateTime(version.createdAtMs)}
                    {each.current && (
                      <span className="ml-2 rounded-full border border-accent px-2 py-0.5 text-xs font-normal text-accent-alt">
                        {t({ message: "In use", context: "save version" })}
                      </span>
                    )}
                  </p>
                  <p className="text-xs text-fg-muted">
                    {who} · {reasonLabel(version.reason)} · {formatBytes(version.sizeBytes)}
                    {!each.complete && ` · ${t`Not on this device yet`}`}
                  </p>
                </div>
                <div className="flex gap-2">
                  {each.complete && (
                    <Button
                      data-spatial="focusable"
                      size="sm"
                      variant="ghost"
                      aria-label={t`Share this save with someone…`}
                      onPress={() => setSharing(version.id)}
                    >
                      <Trans>Share…</Trans>
                    </Button>
                  )}
                  {!each.current && (
                    <Button
                      data-spatial="focusable"
                      size="sm"
                      isDisabled={!canRestore(each)}
                      onPress={() =>
                        entryId &&
                        restore.mutate(
                          { entryId, versionId: version.id },
                          {
                            onSuccess: () =>
                              setDone(t`Restored. The save that was in use is kept as a version.`),
                          },
                        )
                      }
                    >
                      <Trans>Restore</Trans>
                    </Button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
      {running && history.versions.length > 0 && (
        <p className="mt-2 text-xs text-fg-subtle">
          <Trans>Restoring waits until the game has closed.</Trans>
        </p>
      )}
      {(restore.isError || choose.isError) && (
        <p role="alert" className="mt-2 text-sm text-danger">
          {(restore.error ?? choose.error)?.message}
        </p>
      )}
      {done && !restore.isError && (
        <p role="status" className="mt-2 text-sm text-fg-muted">
          {done}
        </p>
      )}
      {choosing && entryId && (
        <SaveConflictDialog
          versions={conflict.map((each) => each.version)}
          busy={choose.isPending}
          error={choose.isError ? choose.error.message : undefined}
          action={t`Use this save`}
          close={() => {
            setChoosing(false);
            choose.reset();
          }}
          choose={(versionId) =>
            choose.mutate({ entryId, versionId }, { onSuccess: () => setChoosing(false) })
          }
        />
      )}
      {sharing && (
        <ShareGameDialog
          target={{
            kind: "item",
            item: { kind: "save", versionId: sharing },
            title: t`a save of ${title}`,
          }}
          close={() => setSharing(null)}
        />
      )}
    </section>
  );
}

/** Where the release's saves live on this device, each with Show in
 * folder in desktop mode. */
function LocationList({
  entryId,
  count,
  paths,
}: {
  entryId: string | undefined;
  count: number;
  paths: { spec: { path: string } }[];
}) {
  const { t } = useLingui();
  const { mode } = useMode();
  const api = useOptionalLibraryApi()?.saves;
  const [error, setError] = useState<string | null>(null);
  if (count === 0 || !entryId) return null;
  return (
    <div data-part="locations" className="mt-4">
      <h3 className="text-sm font-semibold">
        <Trans>Where they are on this device</Trans>
      </h3>
      <ul className="mt-2 flex flex-col gap-1">
        {paths.map(({ spec: { path } }, index) => (
          <li key={path} className="flex items-center justify-between gap-3">
            <span className="min-w-0 break-all font-mono text-xs text-fg-muted">{path}</span>
            {mode === "desktop" && api && (
              <Button
                data-spatial="focusable"
                size="sm"
                variant="ghost"
                aria-label={t`Show in folder: ${path}`}
                onPress={() => {
                  setError(null);
                  api
                    .reveal(entryId, index)
                    .catch((failure: unknown) =>
                      setError(failure instanceof Error ? failure.message : String(failure)),
                    );
                }}
              >
                <FolderOpen aria-hidden className="size-4" />
                <Trans>Show in folder</Trans>
              </Button>
            )}
          </li>
        ))}
      </ul>
      {error && (
        <p role="alert" className="mt-1 text-sm text-danger">
          {error}
        </p>
      )}
    </div>
  );
}

/** An account's name; empty for saves made with nobody signed in. */
function useAccountName() {
  const { t } = useLingui();
  const accounts = useAccounts().data?.accounts ?? [];
  return (id: string) =>
    id === ""
      ? ""
      : (accounts.find((account) => account.id === id)?.name ??
        t({ message: "Another account", context: "save version" }));
}

function useReasonLabel() {
  const { t } = useLingui();
  return (reason: VersionReason): string => {
    switch (reason) {
      case "session":
        return t({ message: "After playing", context: "save version" });
      case "found":
        return t({ message: "Changed outside Caskade", context: "save version" });
      case "beforeRestore":
        return t({ message: "Before a restore", context: "save version" });
      case "restored":
        return t({ message: "Restored", context: "save version" });
      case "chosen":
        return t({ message: "Chosen between devices", context: "save version" });
      case "shared":
        return t({ message: "Shared by someone", context: "save version" });
    }
  };
}
