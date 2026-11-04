/** Settings → Backup (#193): Back up now, automatic backups, and restoring
 * a backup. Admins only: a backup holds every account's data, and a
 * restore replaces it. */

import {
  Button,
  Checkbox,
  SegmentedControl,
  Select,
  SettingRow,
  SettingsSection,
} from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import type { UseQueryResult } from "@tanstack/react-query";
import {
  ArchiveRestore,
  CalendarClock,
  DatabaseBackup,
  FolderOpen,
  LifeBuoy,
  type LucideIcon,
} from "lucide-react";
import { useId, useState } from "react";
import {
  type BackupComponent,
  type BackupExclusion,
  type BackupInspection,
  type BackupPhase,
  type BackupStatus,
  optionalComponents,
  useBackupAction,
  useBackupPlan,
  useBackupStatus,
} from "../../../api/backup";
import { formatDateTime, formatSize } from "../../../api/presentation";
import { FailureNotice } from "../../../components/FailureNotice";
import { usePathPicker } from "../../../components/pathBrowser/PathPicker";
import { useDropZone } from "../../../drop/DropProvider";
import { ProgressLine } from "../../activity/ProgressLine";
import { DraftField } from "../DraftField";
import { type SettingsSectionState, SettingsStatus, useSettingsSection } from "../useSettings";
import { DeviceBackupList, KeepOnDevice } from "./DeviceBackups";

type Schedule = "off" | "daily" | "weekly";
interface BackupSettingsValue {
  destination: string;
  components: BackupComponent[];
  schedule: Schedule;
  keep: number;
}

const componentLabels: Record<BackupComponent, MessageDescriptor> = {
  library: msg`Library and settings`,
  artwork: msg`Your artwork`,
  downloadedArtwork: msg`Downloaded artwork`,
  plugins: msg`Plugins`,
  pluginData: msg`Plugins' files`,
  themes: msg`Themes`,
};

const componentDescriptions: Record<BackupComponent, MessageDescriptor> = {
  library: msg`Games, play time, collections, edits, launch actions, accounts and settings. Always included.`,
  artwork: msg`Covers and images you chose or imported, and web games' pictures.`,
  downloadedArtwork: msg`Images from IGDB. Caskade downloads them again after a restore, so most people leave them out.`,
  plugins: msg`Installed plugins.`,
  pluginData: msg`Files your plugins keep.`,
  themes: msg`Installed themes.`,
};

const exclusionLabels: Record<BackupExclusion, MessageDescriptor> = {
  credentials: msg`passwords and sign-ins`,
  browserData: msg`web games' browser data`,
  gameFiles: msg`games and their saves`,
  managedEmulators: msg`emulators Caskade installed`,
  downloadedRoms: msg`ROMs downloaded from RomM`,
  caches: msg`caches`,
  pendingJobs: msg`installs under way`,
  logs: msg`logs`,
};

const phaseLabels: Record<BackupPhase, MessageDescriptor> = {
  snapshot: msg`Copying the library…`,
  writing: msg`Writing the backup…`,
  verifying: msg`Checking the backup…`,
  extracting: msg`Unpacking the backup…`,
  migrating: msg`Updating the library…`,
};

export function BackupSettings() {
  return (
    <div className="flex flex-col gap-6">
      <BackUpSection />
      <ScheduleSection />
      <RestoreSection />
      <HelpSection />
    </div>
  );
}

function JobProgress({ status }: { status: BackupStatus }) {
  const { t } = useLingui();
  const progress = status.running?.progress;
  return (
    <ProgressLine
      label={t`Backup progress`}
      status={progress ? t(phaseLabels[progress.phase]) : t`Starting…`}
      done={progress?.doneBytes ?? 0}
      total={progress?.totalBytes ?? 0}
    />
  );
}

function BackUpSection() {
  const { t } = useLingui();
  const picker = usePathPicker();
  const statusQuery = useBackupStatus();
  const status = statusQuery.data;
  const preferences = useSettingsSection<BackupSettingsValue>("core.backups");
  const settings = preferences.settings;
  const plan = useBackupPlan(settings?.destination || null, settings?.components ?? []);
  const start = useBackupAction((api) => api.start());
  const cancel = useBackupAction((api) => api.cancel());
  const reveal = useBackupAction((api, path: string) => api.reveal(path));
  const reason = useId();
  const setDestination = (destination: string) =>
    preferences.setSettings((current) => ({ ...current, destination }));
  const drop = useDropZone<HTMLDivElement>((paths) => {
    if (paths[0]) setDestination(paths[0]);
  });
  const title = t`Back up`;
  const description = t`A backup holds Caskade's library and settings in one file you can keep anywhere. Your games and their saves aren't in it.`;
  if (!settings || !status) {
    return (
      <Unloaded
        icon={DatabaseBackup}
        title={title}
        description={description}
        preferences={preferences}
        status={statusQuery}
      />
    );
  }
  const running = status.running;
  const backingUp = running?.kind === "backup" || running?.kind === "automaticBackup";
  const sizes = new Map(plan.data?.components.map((c) => [c.id, c]));
  const toggle = (id: BackupComponent, on: boolean) =>
    preferences.setSettings((current) => ({
      ...current,
      components: on
        ? [...new Set([...current.components, id])]
        : current.components.filter((c) => c !== id),
    }));
  const last = status.last;
  // The default folder is made when needed; a chosen one has to be there.
  const ownFolder = settings.destination && settings.destination !== status.defaultDestination;
  const missing = Boolean(ownFolder) && plan.data?.destinationExists === false;
  const short =
    plan.data?.availableBytes != null &&
    plan.data.availableBytes < plan.data.estimatedBytes + (sizes.get("library")?.bytes ?? 0);
  const blocked = missing || short;

  return (
    <SettingsSection icon={DatabaseBackup} title={title} description={description}>
      <div ref={drop.ref}>
        {/* Saved on blur or Enter: each save checks the folder on disk, so
            a half-typed path would flash "isn't there". */}
        <DraftField
          label={t`Backup folder`}
          value={settings.destination}
          placeholder={status.defaultDestination}
          commit={setDestination}
          pause={null}
          mono
          action={
            <Button
              data-focus-default
              onPress={async () => {
                const chosen = await picker.pick({
                  kind: "folder",
                  title: t`Choose a backup folder`,
                  startAt: settings.destination || status.defaultDestination,
                });
                if (chosen) setDestination(chosen);
              }}
            >
              <FolderOpen className="size-4" />
              <Trans>Browse…</Trans>
            </Button>
          }
        />
      </div>
      {missing && (
        <p role="alert" className="mt-2 text-sm text-warning">
          <Trans>
            This folder isn't there. Connect its drive, or choose another folder. Automatic backups
            try again later.
          </Trans>
        </p>
      )}
      {short && (
        <p role="alert" className="mt-2 text-sm text-warning">
          <Trans>There isn't enough free space in this folder for a backup.</Trans>
        </p>
      )}
      <fieldset className="mt-4 flex flex-col gap-2">
        <legend className="mb-2 text-sm font-medium">
          <Trans>What to back up</Trans>
        </legend>
        {(["library", ...optionalComponents] as BackupComponent[]).map((id) => {
          const size = sizes.get(id);
          return (
            <Checkbox
              key={id}
              isSelected={id === "library" || settings.components.includes(id)}
              isDisabled={id === "library"}
              onChange={(on) => toggle(id, on)}
            >
              <span className="flex flex-col">
                <span className="text-sm">
                  {t(componentLabels[id])}
                  {size && (
                    <span className="ml-2 font-mono text-xs text-fg-muted">
                      {formatSize(size.bytes)}
                    </span>
                  )}
                </span>
                <span className="text-xs text-fg-muted">{t(componentDescriptions[id])}</span>
              </span>
            </Checkbox>
          );
        })}
      </fieldset>
      <SettingsStatus section={preferences} what={t`Backup settings`} />
      {plan.data && (
        <p className="mt-3 text-xs text-fg-muted">
          <Trans>
            About {formatSize(plan.data.estimatedBytes)} before compression. Never included:{" "}
            {plan.data.excluded.map((e) => t(exclusionLabels[e])).join(", ")}.
          </Trans>
        </p>
      )}
      <div className="mt-4 flex flex-wrap items-center gap-3">
        {backingUp ? (
          <Button onPress={() => cancel.mutate(undefined)}>
            <Trans>Cancel</Trans>
          </Button>
        ) : (
          <Button
            variant="primary"
            isDisabled={running !== null || start.isPending || blocked}
            aria-describedby={blocked ? reason : undefined}
            onPress={() => start.mutate(undefined)}
          >
            <DatabaseBackup className="size-4" />
            <Trans>Back up now</Trans>
          </Button>
        )}
        {blocked && !backingUp && (
          <span className="flex items-center gap-2 text-sm text-fg-muted">
            <span id={reason}>
              {missing ? (
                <Trans>Can't back up: the backup folder isn't there.</Trans>
              ) : (
                <Trans>Can't back up: the backup folder doesn't have enough free space.</Trans>
              )}
            </span>
            {/* A drive plugged in or space freed tells the app nothing. */}
            <Button
              size="sm"
              variant="ghost"
              isDisabled={plan.isFetching}
              onPress={() => void plan.refetch()}
            >
              <Trans>Check again</Trans>
            </Button>
          </span>
        )}
        {last && !backingUp && <LastOutcome status={status} onReveal={reveal.mutate} />}
      </div>
      {backingUp && <JobProgress status={status} />}
      {last?.error && !backingUp && (
        <FailureNotice
          className="mt-3"
          variant="inline"
          title={last.automatic ? t`The automatic backup failed` : t`The backup failed`}
          error={last.error}
        />
      )}
      {(start.error || reveal.error) && (
        <FailureNotice className="mt-3" variant="inline" error={start.error ?? reveal.error} />
      )}
    </SettingsSection>
  );
}

function LastOutcome({
  status,
  onReveal,
}: {
  status: BackupStatus;
  onReveal: (path: string) => void;
}) {
  const last = status.last;
  if (!last || last.error || !last.path) return null;
  const path = last.path;
  const time = formatDateTime(new Date(last.atMs));
  const size = formatSize(last.bytes);
  return (
    <span className="flex items-center gap-2 text-sm text-fg-muted">
      <span role="status">
        {last.automatic ? (
          <Trans>
            Last backed up automatically {time} ({size}), checked.
          </Trans>
        ) : (
          <Trans>
            Backed up {time} ({size}), checked.
          </Trans>
        )}
      </span>
      <Button size="sm" variant="ghost" onPress={() => onReveal(path)}>
        <FolderOpen className="size-4" />
        <Trans>Show in folder</Trans>
      </Button>
    </span>
  );
}

function ScheduleSection() {
  const { t } = useLingui();
  const statusQuery = useBackupStatus();
  const status = statusQuery.data;
  const preferences = useSettingsSection<BackupSettingsValue>("core.backups");
  const settings = preferences.settings;
  const title = t`Automatic backups`;
  const description = t`Made in the background to the backup folder, with the same contents. If Caskade isn't running when one is due, it's made soon after the next start.`;
  if (!settings || !status) {
    return (
      <Unloaded
        icon={CalendarClock}
        title={title}
        description={description}
        preferences={preferences}
        status={statusQuery}
      />
    );
  }
  const next = status.nextAutomaticMs;
  const keepOptions = [3, 5, 7, 14, 30].map((n) => ({ id: String(n), label: String(n) }));
  if (!keepOptions.some((o) => o.id === String(settings.keep))) {
    keepOptions.push({ id: String(settings.keep), label: String(settings.keep) });
  }
  return (
    <SettingsSection icon={CalendarClock} title={title} description={description}>
      <SettingRow
        label={t`Back up automatically`}
        description={
          settings.schedule !== "off" && next !== null
            ? next <= Date.now()
              ? t`Due now: it starts within a few minutes.`
              : t`Next one ${formatDateTime(new Date(next))}.`
            : undefined
        }
      >
        <SegmentedControl<Schedule>
          options={[
            { id: "off", label: t({ message: "Off", context: "automatic backups" }) },
            { id: "daily", label: t`Daily` },
            { id: "weekly", label: t`Weekly` },
          ]}
          value={settings.schedule}
          onChange={(schedule) => preferences.setSettings((current) => ({ ...current, schedule }))}
        />
      </SettingRow>
      <SettingRow
        label={t`Automatic backups to keep`}
        description={t`Older ones are removed only after a new one has been checked. Backups you make yourself, and other files, are never removed.`}
      >
        <Select
          options={keepOptions}
          value={String(settings.keep)}
          onChange={(keep) =>
            preferences.setSettings((current) => ({ ...current, keep: Number(keep) }))
          }
        />
      </SettingRow>
      <SettingsStatus section={preferences} what={title} />
      <KeepOnDevice />
    </SettingsSection>
  );
}

function RestoreSection() {
  const { t } = useLingui();
  const picker = usePathPicker();
  const statusQuery = useBackupStatus();
  const status = statusQuery.data;
  const [chosen, setChosen] = useState<{ path: string; inspection: BackupInspection } | null>(null);
  const [components, setComponents] = useState<BackupComponent[]>([]);
  const inspect = useBackupAction((api, path: string) => api.inspect(path));
  const restore = useBackupAction(
    async (api, choice: { path: string; components: BackupComponent[] }) => {
      await api.restore(choice.path, choice.components);
      await api.restart();
    },
  );
  const restart = useBackupAction((api) => api.restart());
  const discard = useBackupAction((api) => api.discardRestore());
  const open = async (path: string) => {
    setChosen(null);
    const inspection = await inspect.mutateAsync(path);
    setChosen({ path, inspection });
    setComponents(inspection.components.map((c) => c.id).filter((id) => id !== "library"));
  };
  const drop = useDropZone<HTMLDivElement>((paths) => {
    if (paths[0]) void open(paths[0]).catch(() => {});
  });
  const title = t`Restore`;
  const description = t`Restoring replaces this library and its settings with the backup's, then restarts Caskade. Nothing is merged, and your games' files aren't touched.`;
  if (!status) {
    return (
      <Unloaded
        icon={ArchiveRestore}
        title={title}
        description={description}
        status={statusQuery}
      />
    );
  }
  const staged = status.stagedRestore;
  const outcome = status.lastRestore;
  const busy = status.running !== null;

  return (
    <SettingsSection icon={ArchiveRestore} title={title} description={description}>
      {outcome && (
        <p role="status" className={`mb-3 text-sm ${outcome.error ? "text-danger" : "text-fg"}`}>
          {outcome.error ? (
            <Trans>
              The restore of {outcome.journal.archive} was undone, and the library from before is
              back: {outcome.error}
            </Trans>
          ) : (
            <Trans>Restored from {outcome.journal.archive}.</Trans>
          )}
        </p>
      )}
      {staged ? (
        <div className="flex flex-col gap-3">
          <p className="text-sm">
            <Trans>
              {staged.archive} is ready to restore. Caskade applies it when it restarts.
            </Trans>
          </p>
          <div className="flex gap-3">
            <Button variant="primary" isDisabled={busy} onPress={() => restart.mutate(undefined)}>
              {/* The same words as the restore it finishes (#57). */}
              <Trans>Restore and restart</Trans>
            </Button>
            <Button isDisabled={busy} onPress={() => discard.mutate(undefined)}>
              <Trans>Don't restore</Trans>
            </Button>
          </div>
        </div>
      ) : (
        <div ref={drop.ref}>
          <Button
            isDisabled={busy || inspect.isPending}
            onPress={async () => {
              const path = await picker.pick({
                kind: "file",
                title: t`Choose a Caskade backup`,
                startAt: status.defaultDestination,
                extensions: ["zip"],
              });
              if (path) await open(path).catch(() => {});
            }}
          >
            <FolderOpen className="size-4" />
            <Trans>Choose a backup…</Trans>
          </Button>
          <DeviceBackupList open={open} busy={busy || inspect.isPending} />
        </div>
      )}
      {status.running?.kind === "inspect" || status.running?.kind === "restore" ? (
        <JobProgress status={status} />
      ) : null}
      {chosen && !staged && (
        <RestorePreview
          inspection={chosen.inspection}
          components={components}
          onComponents={setComponents}
          busy={busy || restore.isPending}
          onRestore={() => restore.mutate({ path: chosen.path, components })}
          onCancel={() => setChosen(null)}
        />
      )}
      {[inspect.error, restore.error, restart.error, discard.error].map(
        (error, index) =>
          error && (
            <FailureNotice
              // biome-ignore lint/suspicious/noArrayIndexKey: one slot per action
              key={index}
              className="mt-3"
              variant="inline"
              error={error}
            />
          ),
      )}
    </SettingsSection>
  );
}

/** A section whose backup settings or backups' state didn't load (#42):
 * why, with Try again, in place of the section. Nothing while they load. */
function Unloaded({
  icon,
  title,
  description,
  preferences,
  status,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  preferences?: SettingsSectionState;
  status: UseQueryResult<BackupStatus>;
}) {
  const { t } = useLingui();
  if (!preferences?.loadError && !status.error) return null;
  return (
    <SettingsSection icon={icon} title={title} description={description}>
      {preferences && (
        <SettingsStatus section={preferences} what={t`Backup settings`} className="" />
      )}
      {status.error && (
        <FailureNotice
          variant="inline"
          className={preferences?.loadError ? "mt-3" : ""}
          error={status.error}
          title={t`Backups couldn't be loaded`}
          onRetry={() => void status.refetch()}
        />
      )}
    </SettingsSection>
  );
}

function RestorePreview({
  inspection,
  components,
  onComponents,
  busy,
  onRestore,
  onCancel,
}: {
  inspection: BackupInspection;
  components: BackupComponent[];
  onComponents: (components: BackupComponent[]) => void;
  busy: boolean;
  onRestore: () => void;
  onCancel: () => void;
}) {
  const { t } = useLingui();
  const manifest = inspection.manifest;
  const games = manifest.tables.find((table) => table.name === "games")?.rows ?? 0;
  const gameCount = plural({ total: games }, { one: "# game", other: "# games" });
  const made = formatDateTime(new Date(manifest.createdAtMs));
  const version = manifest.appVersion;
  return (
    <section
      aria-label={t`Backup to restore`}
      className="mt-4 flex flex-col gap-3 rounded-control border border-border bg-surface-base/30 p-4"
    >
      <p className="text-sm">
        <Trans>
          Made {made} by Caskade {version}, with {gameCount}.
        </Trans>
      </p>
      <fieldset className="flex flex-col gap-2">
        <legend className="mb-1 text-sm font-medium">
          <Trans>Restore</Trans>
        </legend>
        {inspection.components.map(({ id, bytes }) => (
          <Checkbox
            key={id}
            isSelected={id === "library" || components.includes(id)}
            isDisabled={id === "library"}
            onChange={(on) =>
              onComponents(on ? [...components, id] : components.filter((c) => c !== id))
            }
          >
            {t(componentLabels[id])}
            <span className="ml-2 font-mono text-xs text-fg-muted">{formatSize(bytes)}</span>
          </Checkbox>
        ))}
      </fieldset>
      <ul className="flex list-disc flex-col gap-1 pl-5 text-xs text-fg-muted">
        <li>
          <Trans>
            Parts you leave unticked stay as they are now. Plugins restored without their files ask
            for their permissions again.
          </Trans>
        </li>
        <li>
          <Trans>
            Passwords and sign-ins aren't in backups: sign in to RomM and your stores again
            afterwards.
          </Trans>
        </li>
        {inspection.needsMigration && (
          <li>
            <Trans>It was made by an older Caskade: its library is updated as it's restored.</Trans>
          </li>
        )}
        {inspection.otherOs && (
          <li className="text-warning">
            <Trans>
              It was made on another operating system: games and storage locations whose folders
              aren't on this computer show as missing until you find them again.
            </Trans>
          </li>
        )}
        {inspection.missingRoots.length > 0 && (
          <li className="text-warning">
            <Trans>
              Storage locations not found here: {inspection.missingRoots.join(", ")}. They show as
              offline until their drives are connected.
            </Trans>
          </li>
        )}
        {inspection.missingGames > 0 && (
          <li className="text-warning">
            {plural(inspection.missingGames, {
              one: "# game's folder isn't on this computer; the next scan finds it if it's elsewhere.",
              other:
                "# games' folders aren't on this computer; the next scan finds those that are elsewhere.",
            })}
          </li>
        )}
      </ul>
      <div className="flex gap-3">
        <Button variant="danger" isDisabled={busy} onPress={onRestore}>
          <ArchiveRestore className="size-4" />
          <Trans>Restore and restart</Trans>
        </Button>
        <Button isDisabled={busy} onPress={onCancel}>
          <Trans>Cancel</Trans>
        </Button>
      </div>
    </section>
  );
}

function HelpSection() {
  const { t } = useLingui();
  return (
    <SettingsSection icon={LifeBuoy} title={t`About backups`}>
      <ul className="flex list-disc flex-col gap-2 pl-5 text-sm text-fg-muted">
        <li>
          <Trans>
            Backups never hold passwords or sign-ins, web games' browser data, logs, caches, or your
            games and their saves.
          </Trans>
        </li>
        <li>
          <Trans>
            Caskade keeps versions of each game's saves and sends the newest to your paired devices.
            Find them in Settings → Devices → Saves, and restore one from the game's page.
          </Trans>
        </li>
        <li>
          <Trans>
            To move to another computer, copy a backup there, install Caskade, and restore it from
            here. Games keep their place in your library; Caskade finds their folders again when you
            scan, or you can point it at them.
          </Trans>
        </li>
        <li>
          <Trans>
            If Caskade can't open its library, its error message offers Restore from backup…. You
            can also start Caskade with --restore and a backup file.
          </Trans>
        </li>
        <li>
          <Trans>
            Every backup is read back and checked before it's reported done, and a restore keeps the
            library it replaces until the restored one opens.
          </Trans>
        </li>
      </ul>
    </SettingsSection>
  );
}
