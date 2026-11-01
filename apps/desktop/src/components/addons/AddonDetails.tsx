/** One catalogue add-on (#183): its description (as text), screenshots,
 * releases, what it asks for and why it may not fit, and installing,
 * updating, skipping and removing it through the shared package service.
 * A first plugin install goes through the plugin review; a theme can be
 * applied once installed (themes are CSS, nothing runs). */
import { Button, Checkbox } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { ImageOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import {
  type AddonOutcome,
  type AddonProgress,
  type AppMode,
  type CatalogueItem,
  useAddonMedia,
  useAddonsApi,
  useAddonsChanged,
} from "../../api/addons";
import type { AddonRecord } from "../../api/bindings";
import { capabilityLines, ReviewDialog } from "../../views/settings/PluginsSettings";
import { useSettingsSection } from "../../views/settings/useSettings";
import { StateBadge } from "./AddonCatalogue";

export function AddonDetails({
  item,
  mode,
  offline,
  record,
  close,
}: {
  item: CatalogueItem;
  mode: AppMode;
  offline: boolean;
  record: AddonRecord | undefined;
  close: () => void;
}) {
  const { t } = useLingui();
  const api = useAddonsApi();
  const changed = useAddonsChanged();
  const { entry, compatibility } = item;
  const release = compatibility.release;
  const [progress, setProgress] = useState<AddonProgress | null>(null);
  const [outcome, setOutcome] = useState<AddonOutcome | null>(null);
  const [keepData, setKeepData] = useState(true);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const primary = useRef<HTMLButtonElement>(null);
  useEffect(
    () =>
      api.subscribeProgress((next) => {
        if (next.id === entry.id) setProgress(next);
      }),
    [api, entry.id],
  );
  const install = useMutation({
    mutationFn: ({ accept, retry }: { accept: boolean; retry: boolean }) =>
      api.install(entry.id, mode, accept, retry),
    onSuccess: (result) => {
      setProgress(null);
      setOutcome(result);
      // A failure may be remembered as a rejected release.
      if (result.status !== "needsReview") void changed();
    },
    onError: () => setProgress(null),
  });
  const remove = useMutation({
    mutationFn: () => api.uninstall(entry.id, keepData),
    onSuccess: () => {
      setConfirmRemove(false);
      void changed();
    },
  });
  const skip = useMutation({
    mutationFn: (version: string | null) => api.skipUpdate(entry.id, version),
    onSuccess: () => void changed(),
  });
  const installed =
    compatibility.state === "installed" || compatibility.state === "updateAvailable";
  const rejected =
    record?.rejected && record.rejected.version === release?.version ? record.rejected : null;
  const skipped = compatibility.state === "updateAvailable" && record?.skipped === release?.version;
  const busy = install.isPending || remove.isPending;
  const review = outcome?.status === "needsReview" ? outcome : null;
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm console:p-0"
    >
      <Modal className="flex max-h-[88vh] w-full max-w-2xl flex-col rounded-panel border border-border-strong bg-surface-raised shadow-lg console:h-full console:max-h-none console:max-w-none console:rounded-none">
        <Dialog
          data-cs="addon-details"
          data-kind={entry.kind}
          data-state={compatibility.state}
          className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-6 outline-none"
        >
          <div className="flex flex-wrap items-start gap-3">
            <div className="min-w-0 flex-1">
              <Heading slot="title" className="font-display text-xl font-semibold">
                {entry.name}
              </Heading>
              <p className="text-sm text-fg-muted">
                {entry.kind === "plugin" ? t`Plugin` : t`Theme`} · {entry.author} · {entry.license}
              </p>
              <p className="font-mono text-xs text-fg-subtle">{entry.id}</p>
            </div>
            <StateBadge item={item} />
          </div>
          {entry.screenshots.length > 0 && (
            <ul
              data-part="screenshots"
              aria-label={t`Screenshots`}
              className="flex gap-3 overflow-x-auto"
            >
              {entry.screenshots.map((shot, index) => (
                <li key={shot.url} className="shrink-0">
                  <Screenshot id={entry.id} index={index} alt={shot.alt} />
                </li>
              ))}
            </ul>
          )}
          {/* Text, never markup. */}
          <p data-part="description" className="text-sm whitespace-pre-wrap">
            {entry.description || entry.summary}
          </p>
          {entry.deprecated && (
            <p role="note" className="text-sm text-warning">
              {t`Deprecated: ${entry.deprecated}`}
            </p>
          )}
          {compatibility.reasons.length > 0 && (
            <ul
              data-part="reasons"
              aria-label={t`Compatibility`}
              className="flex flex-col gap-1 text-sm"
            >
              {compatibility.reasons.map((reason) => (
                <li
                  key={`${reason.code}:${reason.message}`}
                  className={reason.blocking ? "text-danger" : "text-warning"}
                >
                  {reason.message}
                </li>
              ))}
            </ul>
          )}
          {release && (
            <dl data-part="release" className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
              <dt className="text-fg-muted">{t`Version`}</dt>
              <dd>
                {release.version}
                {release.published && (
                  <span className="text-fg-subtle"> · {release.published}</span>
                )}
              </dd>
              <dt className="text-fg-muted">{t`Runs on`}</dt>
              <dd>
                {release.os.length === 0
                  ? t`Linux, Windows and macOS`
                  : release.os
                      .map((os) => ({ linux: "Linux", windows: "Windows", macos: "macOS" })[os])
                      .join(", ")}
              </dd>
              {entry.kind === "plugin" && (
                <>
                  <dt className="text-fg-muted">{t`Modes`}</dt>
                  <dd>
                    {release.modes.length === 1
                      ? release.modes[0] === "desktop"
                        ? t`Desktop mode`
                        : t`Console mode`
                      : t`Desktop and console modes`}
                  </dd>
                  <dt className="text-fg-muted">{t`Asks for`}</dt>
                  <dd>
                    {release.permissions.length > 0
                      ? permissionLines(release.permissions).map((line) => (
                          <span key={line} className="block">
                            {line}
                          </span>
                        ))
                      : t`Nothing beyond its own storage`}
                  </dd>
                </>
              )}
              <dt className="text-fg-muted">{t`Download`}</dt>
              <dd>{t`${Math.max(1, Math.round(release.artifact.size / 1024))} KB`}</dd>
              {release.notes && (
                <>
                  <dt className="text-fg-muted">{t`What's new`}</dt>
                  <dd className="whitespace-pre-wrap">{release.notes}</dd>
                </>
              )}
            </dl>
          )}
          {progress && (
            <div data-part="progress" role="status" className="text-sm text-fg-muted">
              {progress.phase === "downloading"
                ? t`Downloading… ${Math.round((progress.received * 100) / Math.max(1, progress.total))}%`
                : progress.phase === "checking"
                  ? t`Checking the download…`
                  : progress.phase === "installing"
                    ? t`Installing…`
                    : t`Starting…`}
            </div>
          )}
          <Outcome outcome={outcome} rejected={rejected} />
          {offline && !installed && (
            <p className="text-sm text-fg-muted">{t`Installing needs a connection.`}</p>
          )}
          {confirmRemove && (
            <div
              data-part="remove-confirm"
              className="flex flex-col gap-2 rounded-panel border border-danger/40 p-3"
            >
              <p className="text-sm">
                {entry.kind === "plugin"
                  ? t`Remove ${entry.name}?`
                  : t`Remove ${entry.name}? If it's the theme in use, the default theme takes over.`}
              </p>
              {entry.kind === "plugin" && (
                <Checkbox data-spatial="focusable" isSelected={keepData} onChange={setKeepData}>
                  {t`Keep its settings and data for a reinstall`}
                </Checkbox>
              )}
              <div className="flex gap-2">
                <Button data-spatial="focusable" size="sm" onPress={() => setConfirmRemove(false)}>
                  <Trans>Keep it</Trans>
                </Button>
                <Button
                  data-part="remove-confirmed"
                  data-spatial="focusable"
                  size="sm"
                  variant="danger"
                  isDisabled={remove.isPending}
                  onPress={() => remove.mutate()}
                >
                  <Trans>Remove</Trans>
                </Button>
              </div>
              {remove.error && (
                <p role="alert" className="text-sm text-danger">
                  {remove.error.message}
                </p>
              )}
            </div>
          )}
          <div data-part="actions" className="flex flex-wrap justify-end gap-2">
            {install.isPending && (
              <Button data-spatial="focusable" onPress={() => void api.cancel(entry.id)}>
                <Trans>Cancel download</Trans>
              </Button>
            )}
            {installed && !confirmRemove && (
              <Button
                data-part="remove"
                data-spatial="focusable"
                variant="ghost"
                isDisabled={busy}
                onPress={() => setConfirmRemove(true)}
              >
                <Trans>Remove</Trans>
              </Button>
            )}
            {compatibility.state === "updateAvailable" && release && (
              <Button
                data-part="skip"
                data-spatial="focusable"
                variant="ghost"
                isDisabled={busy}
                onPress={() => skip.mutate(skipped ? null : release.version)}
              >
                {skipped ? <Trans>Offer this update again</Trans> : <Trans>Skip this update</Trans>}
              </Button>
            )}
            {entry.kind === "theme" && installed && <ApplyTheme id={entry.id} />}
            {release && compatibility.state !== "installed" && (
              <Button
                ref={primary}
                data-part="install"
                data-spatial="focusable"
                data-focus-default
                variant="primary"
                isDisabled={busy}
                onPress={() => install.mutate({ accept: false, retry: Boolean(rejected) })}
              >
                {compatibility.state === "updateAvailable"
                  ? t`Update to ${release.version}`
                  : rejected
                    ? t`Try again`
                    : t`Install`}
              </Button>
            )}
            <Button data-spatial="focusable" onPress={close} isDisabled={busy}>
              <Trans>Close</Trans>
            </Button>
          </div>
          {install.error && (
            <p role="alert" className="text-sm text-danger">
              {install.error.message}
            </p>
          )}
        </Dialog>
      </Modal>
      {review && (
        <ReviewDialog
          plugin={review.preview.plugin}
          installedVersion={review.preview.installedVersion}
          action={t`Install and allow`}
          busy={install.isPending}
          error={install.error?.message}
          accept={() => install.mutate({ accept: true, retry: false })}
          close={() => setOutcome(null)}
        />
      )}
    </ModalOverlay>
  );
}

function Screenshot({ id, index, alt }: { id: string; index: number; alt: string }) {
  const { t } = useLingui();
  const media = useAddonMedia(id, index);
  if (!media.data)
    return (
      <div
        data-part="screenshot-missing"
        role="img"
        aria-label={alt || t`Screenshot`}
        className="grid h-36 w-64 place-items-center rounded-control border border-border bg-surface-base/60 text-fg-subtle"
      >
        <ImageOff className="size-5" aria-hidden />
      </div>
    );
  return (
    <img
      data-part="screenshot"
      src={media.data}
      alt={alt}
      className="h-36 rounded-control object-cover"
    />
  );
}

function Outcome({
  outcome,
  rejected,
}: {
  outcome: AddonOutcome | null;
  rejected: AddonRecord["rejected"];
}) {
  const { t } = useLingui();
  if (outcome?.status === "installed")
    return (
      <p
        data-part="outcome"
        data-outcome="installed"
        role="status"
        className="text-sm text-success"
      >
        {outcome.needsConsent
          ? t`${outcome.version} is installed. It asks for more than before, so it's off until you allow it under Installed plugins.`
          : outcome.restartRequired
            ? t`${outcome.version} is installed. Its pages change when Caskade restarts.`
            : t`${outcome.version} is installed.`}
      </p>
    );
  const failure =
    outcome?.status === "failed"
      ? outcome
      : rejected && !outcome
        ? { kind: rejected.kind, message: rejected.message, rolledBack: false }
        : null;
  if (!failure) return null;
  return (
    <p data-part="outcome" data-outcome="failed" role="alert" className="text-sm text-danger">
      {failure.rolledBack
        ? t`It didn't install, and the previous version is back: ${failure.message}`
        : failure.kind === "network" || failure.kind === "storage"
          ? t`It didn't install; try again later: ${failure.message}`
          : t`It didn't install: ${failure.message}`}
    </p>
  );
}

/** Makes an installed theme the one in use, as Appearance does. */
function ApplyTheme({ id }: { id: string }) {
  const { settings, setSettings, saving } = useSettingsSection<{ theme?: string }>(
    "core.appearance",
  );
  const active = settings?.theme === id;
  return (
    <Button
      data-part="apply"
      data-spatial="focusable"
      variant="secondary"
      isDisabled={active || saving || !settings}
      onPress={() => setSettings((current) => ({ ...current, theme: id }))}
    >
      {active ? <Trans>In use</Trans> : <Trans>Use this theme</Trans>}
    </Button>
  );
}

/** The catalogue's permission words in the review's plain words; a word
 * this version doesn't know stays as written. */
function permissionLines(words: string[]): string[] {
  const permissions: NonNullable<Parameters<typeof capabilityLines>[0]> = {
    network: [],
    fs: [],
    process: false,
    library: "none",
  };
  const unknown: string[] = [];
  for (const word of words) {
    if (word === "library.read") permissions.library = "read";
    else if (word === "library.write") permissions.library = "write";
    else if (word === "process") permissions.process = true;
    else if (word.startsWith("network:")) permissions.network.push(word.slice("network:".length));
    else if (word.startsWith("fs:")) permissions.fs.push(word.slice("fs:".length));
    else unknown.push(word);
  }
  return [...capabilityLines(permissions), ...unknown];
}
