import {
  Button,
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
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  CloudOff,
  FilterX,
  FolderSearch,
  FolderTree,
  Info,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import { useState } from "react";
import { Button as AriaButton } from "react-aria-components";
import type {
  EmulationSnapshot,
  EmulatorTarget,
  RomScanner,
  ScanJob,
  ScannerRow,
} from "../../../api/emulation";
import { formatDateTime } from "../../../api/presentation";
import { useActionMenus } from "../../../components/ActionMenu";
import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { UnsavedHint, useUnsavedChanges } from "../unsavedChanges";
import { PathField } from "./EmulationFields";
import {
  emulationKey,
  mergeScan,
  scanJobsKey,
  useEmulationApi,
  useScanJobs,
  useScanners,
} from "./EmulationProvider";
import { ScanReview } from "./ScanReview";

const playlistTypes = new Set(["m3u", "cue", "gdi"]);
const exclusionSyntax = [
  { pattern: "bios/", meaning: msg`This exact path under the scan folder` },
  { pattern: ">*.txt", meaning: msg`A name or path at any depth (glob)` },
  { pattern: "?^beta/.*", meaning: msg`Regex anchored at the scan folder` },
  { pattern: ">?\\(Beta\\)", meaning: msg`Regex at any depth` },
];
type Toggle = "recursive" | "scanArchives" | "skipCloudPlaceholders" | "inLibraryUpdate";
const toggles: {
  key: Toggle;
  icon: typeof FolderTree;
  label: MessageDescriptor;
  description: MessageDescriptor;
}[] = [
  {
    key: "recursive",
    icon: FolderTree,
    label: msg`Scan subfolders`,
    description: msg`Look in every folder below the scan folder.`,
  },
  {
    key: "scanArchives",
    icon: Archive,
    label: msg`Look inside archives`,
    description: msg`Find ROMs inside zip and 7z files the profile can't open directly.`,
  },
  {
    key: "skipCloudPlaceholders",
    icon: CloudOff,
    label: msg`Skip cloud-only files`,
    description: msg`Don't download OneDrive, Dropbox or iCloud placeholders to scan them.`,
  },
  {
    key: "inLibraryUpdate",
    icon: RefreshCw,
    label: msg`Include in library update`,
    description: msg`Import new ROMs whenever the library updates, without a review.`,
  },
];
const launchModes = [
  {
    id: "platform_default",
    label: msg({ message: "Platform default", comment: "What a scanner's games launch with" }),
  },
  { id: "profile", label: msg`This scanner's profile` },
  { id: "ask_profile", label: msg`Ask for a profile of this emulator` },
  { id: "ask_emulator", label: msg`Ask for any emulator` },
] as const;
const ref = (emulatorId: string, profileId: string) => `${emulatorId}/${profileId}`;
const platformShort = (snapshot: EmulationSnapshot, id: string) =>
  snapshot.platforms.find((platform) => platform.id === id)?.short ?? id;

function launchWith(mode: EmulatorTarget["type"], scanner: RomScanner): EmulatorTarget {
  switch (mode) {
    case "profile":
      return { type: "profile", ...scanner.profile };
    case "ask_profile":
      return { type: "ask_profile", emulatorId: scanner.profile.emulatorId };
    default:
      return { type: mode };
  }
}

function profileOptions(snapshot: EmulationSnapshot) {
  return snapshot.emulators.flatMap((emulator) =>
    emulator.profiles.map(({ config, resolved }) => ({
      id: ref(emulator.id, config.id),
      label: `${emulator.name} · ${resolved.name}`,
      emulatorId: emulator.id,
      profileId: config.id,
      platforms: resolved.platforms,
      extensions: resolved.extensions,
    })),
  );
}

function ScannerEditor({
  initial,
  row,
  snapshot,
  saved,
  removed,
}: {
  initial: RomScanner;
  row?: ScannerRow;
  snapshot: EmulationSnapshot;
  saved: (scanner: RomScanner, created: boolean) => void;
  removed: () => void;
}) {
  const { t } = useLingui();
  const api = useEmulationApi();
  const [draft, setDraft] = useState(initial);
  const [base, setBase] = useState(initial);
  // A review or the include switch changes the saved scanner; follow it
  // unless the draft has edits of its own.
  if (initial !== base) {
    setBase(initial);
    if (JSON.stringify(draft) === JSON.stringify(base)) setDraft(initial);
  }
  const created = !initial.id;
  const save = useMutation({
    mutationFn: () => api.saveScanner(draft),
    onSuccess: (next) => saved(next, created),
  });
  const remove = useMutation({
    mutationFn: () => api.removeScanner(initial.id),
    onSuccess: removed,
  });
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const profiles = profileOptions(snapshot);
  const profile = profiles.find(
    (option) => option.id === ref(draft.profile.emulatorId, draft.profile.profileId),
  );
  const set = <K extends keyof RomScanner>(key: K, value: RomScanner[K]) =>
    setDraft((old) => ({ ...old, [key]: value }));
  const title = created ? t`New ROM scanner` : draft.name || t`Unnamed scanner`;
  const dirty = JSON.stringify(draft) !== JSON.stringify(base);
  // The scanner waits for its Save button (#43): leaving the section asks first.
  useUnsavedChanges(dirty, {
    label: title,
    save: async () => {
      if (!profile) return false;
      await save.mutateAsync();
      return true;
    },
    discard: () => setDraft(base),
  });
  return (
    <SettingsSection
      icon={FolderSearch}
      title={title}
      description={t`The profile decides which files match. Imported games launch with the choice below.`}
      aside={
        created ? undefined : (
          <Button
            variant="danger"
            size="sm"
            icon={Trash2}
            isDisabled={remove.isPending}
            onPress={() => {
              remove.reset();
              setConfirmingRemove(true);
            }}
          >
            <Trans comment="Remove the ROM scanner">Remove</Trans>
          </Button>
        )
      }
    >
      <div data-cs="scanner-editor" className="mt-3 grid gap-4 md:grid-cols-2">
        <TextField
          label={t`Scanner name`}
          value={draft.name}
          onChange={(name) => set("name", name)}
        />
        <PathField
          label={t`ROM folder`}
          value={draft.folder}
          onChange={(folder) => set("folder", folder)}
        />
        <Select
          label={t`Emulator profile`}
          placeholder={t`Choose a profile`}
          options={profiles.map(({ id, label }) => ({ id, label }))}
          value={profile?.id ?? ""}
          isDisabled={!profiles.length}
          onChange={(id) => {
            const next = profiles.find((option) => option.id === id);
            if (!next) return;
            setDraft((old) => {
              const scanner = {
                ...old,
                platform: null,
                profile: { emulatorId: next.emulatorId, profileId: next.profileId },
              };
              return { ...scanner, launchWith: launchWith(old.launchWith.type, scanner) };
            });
          }}
        />
        <Select
          label={t({ message: "Platform", comment: "A ROM scanner's platform" })}
          options={[
            {
              id: "profile",
              label: profile?.platforms.length
                ? t`From profile (${{ platforms: profile.platforms.map((id) => platformShort(snapshot, id)).join(", ") }})`
                : t`From profile`,
            },
            ...(profile?.platforms ?? []).map((id) => ({
              id,
              label: snapshot.platforms.find((platform) => platform.id === id)?.name ?? id,
            })),
          ]}
          value={draft.platform ?? "profile"}
          onChange={(platform) => set("platform", platform === "profile" ? null : platform)}
        />
        <Select
          label={t`Games launch with`}
          options={launchModes.map(({ id, label }) => ({ id, label: t(label) }))}
          value={draft.launchWith.type}
          onChange={(mode) => set("launchWith", launchWith(mode, draft))}
        />
      </div>
      {!profiles.length && (
        <p className="mt-3 text-sm text-fg-muted">
          <Trans>
            Add an emulator with a profile first; its file types decide what a scanner finds.
          </Trans>
        </p>
      )}
      {profile && (
        <div className="mt-3 flex flex-wrap items-center gap-1.5 text-xs text-fg-subtle">
          <Trans comment="Followed by the file extensions the scanner's profile matches">
            Matches
          </Trans>
          {profile.extensions.map((extension) => (
            <span
              key={extension}
              data-part="extension"
              className="rounded-control border border-border px-1.5 py-0.5 font-mono"
            >
              .{extension}
            </span>
          ))}
          {profile.extensions.some((extension) => playlistTypes.has(extension)) && (
            <span>
              ·{" "}
              <Trans>
                Playlists absorb the files they list, so disc tracks aren't separate games.
              </Trans>
            </span>
          )}
        </div>
      )}
      {row?.flatpakHint && (
        <p
          data-part="flatpak-hint"
          className="mt-3 flex gap-2 rounded-control border border-accent-alt/30 bg-accent-alt/5 p-3 text-sm text-fg-muted"
        >
          <Info className="mt-0.5 size-4 shrink-0 text-accent-alt" />
          {row.flatpakHint}
        </p>
      )}
      {row?.problem && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {row.problem}
        </p>
      )}
      <div className="mt-4">
        {toggles.map(({ key, icon, label, description }) => (
          <SettingRow key={key} icon={icon} label={t(label)} description={t(description)}>
            <Switch isSelected={draft[key]} onChange={(on) => set(key, on)} />
          </SettingRow>
        ))}
      </div>
      <div className="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_18rem]">
        <div>
          <h3 className="text-sm font-medium">
            <Trans>Exclusions</Trans>
          </h3>
          <p className="text-xs text-fg-subtle">
            <Trans>
              Files and folders this scanner always skips. Unticking a game in a review adds it
              here.
            </Trans>
          </p>
          <ExclusionList
            patterns={draft.exclusions}
            onChange={(patterns) => set("exclusions", patterns)}
          />
        </div>
        <ExclusionSyntax />
      </div>
      {save.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {save.error.message}
        </p>
      )}
      {confirmingRemove && (
        <ConfirmDialog
          title={t`Remove ${{ scanner: title }}?`}
          confirmLabel={t({ message: "Remove", comment: "Confirms removing a ROM scanner" })}
          pendingLabel={t`Removing…`}
          isPending={remove.isPending}
          error={remove.error}
          onConfirm={() => remove.mutate()}
          close={() => setConfirmingRemove(false)}
        >
          <p>
            <Trans>
              Its games stay in your library as not installed, because Caskade no longer looks for
              their ROMs. Your ROM files aren't touched.
            </Trans>
          </p>
        </ConfirmDialog>
      )}
      <div className="mt-4 flex items-center justify-end gap-2">
        <UnsavedHint dirty={dirty} className="mr-auto" />
        <Button
          variant="primary"
          isDisabled={save.isPending || !profile}
          onPress={() => save.mutate()}
        >
          {save.isPending ? t`Saving…` : created ? t`Save and scan` : t`Save scanner`}
        </Button>
      </div>
    </SettingsSection>
  );
}

/** Exclusion patterns with remove buttons and a field to add one. */
function ExclusionList({
  global = false,
  patterns,
  onChange,
  isDisabled,
}: {
  /** Every scanner's list rather than one scanner's. */
  global?: boolean;
  patterns: string[];
  onChange: (patterns: string[]) => void;
  isDisabled?: boolean;
}) {
  const { t } = useLingui();
  const [pattern, setPattern] = useState("");
  const add = () => {
    const next = pattern.trim();
    if (next && !patterns.includes(next)) onChange([...patterns, next]);
    setPattern("");
  };
  return (
    <>
      <ul
        aria-label={global ? t`Global exclusions` : t`Exclusions`}
        className="mt-2 flex flex-col gap-1"
      >
        {patterns.map((exclusion) => (
          <li
            key={exclusion}
            data-part="exclusion"
            className="flex items-center gap-2 rounded-control border border-border bg-surface-base/60 py-1 pr-1 pl-3"
          >
            <code className="flex-1 truncate font-mono text-xs">{exclusion}</code>
            <AriaButton
              aria-label={
                global
                  ? t`Remove global exclusion ${{ pattern: exclusion }}`
                  : t`Remove exclusion ${{ pattern: exclusion }}`
              }
              isDisabled={isDisabled}
              onPress={() => onChange(patterns.filter((other) => other !== exclusion))}
              className="grid size-7 place-items-center rounded-[6px] text-fg-subtle data-focus-visible:focus-ring data-hovered:bg-surface-hover data-hovered:text-danger"
            >
              <X className="size-4" />
            </AriaButton>
          </li>
        ))}
        {patterns.length === 0 && (
          <li className="text-xs text-fg-subtle italic">
            <Trans>No exclusions.</Trans>
          </li>
        )}
      </ul>
      <form
        className="mt-2"
        onSubmit={(event) => {
          event.preventDefault();
          add();
        }}
      >
        <TextField
          aria-label={global ? t`New global exclusion pattern` : t`New exclusion pattern`}
          mono
          placeholder=">*.sav"
          value={pattern}
          onChange={setPattern}
          action={
            <Button
              type="submit"
              icon={Plus}
              aria-label={global ? t`Add global exclusion` : undefined}
              isDisabled={isDisabled || !pattern.trim()}
            >
              <Trans comment="Add the exclusion pattern">Add</Trans>
            </Button>
          }
        />
      </form>
    </>
  );
}

function ExclusionSyntax() {
  const { t } = useLingui();
  return (
    <aside className="rounded-control border border-accent-alt/25 bg-accent-alt/5 p-3 text-xs text-fg-muted">
      <div className="mb-2 font-mono tracking-wider text-accent-alt uppercase">
        <Trans comment="The exclusion patterns' syntax">Syntax</Trans>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5">
        {exclusionSyntax.map((syntax) => (
          <div key={syntax.pattern} className="contents">
            <dt>
              <code className="font-mono text-fg">{syntax.pattern}</code>
            </dt>
            <dd>{t(syntax.meaning)}</dd>
          </div>
        ))}
      </dl>
    </aside>
  );
}

const globalExclusionsKey = [...emulationKey, "exclusions"] as const;

/** Exclusions every scanner applies (§6.3); saved as soon as they change. */
function GlobalExclusions() {
  const { t } = useLingui();
  const api = useEmulationApi();
  const client = useQueryClient();
  const exclusions = useQuery({
    queryKey: globalExclusionsKey,
    queryFn: () => api.globalExclusions(),
  });
  const save = useMutation({
    mutationFn: (patterns: string[]) => api.setGlobalExclusions(patterns),
    onSuccess: (saved) => client.setQueryData(globalExclusionsKey, saved),
  });
  return (
    <SettingsSection
      icon={FilterX}
      title={t`Global exclusions`}
      description={t`Files and folders every scanner skips, from its next scan.`}
    >
      {exclusions.error ? (
        <p role="alert" className="mt-3 text-sm text-danger">
          {exclusions.error.message}
        </p>
      ) : exclusions.isPending ? (
        <p role="status" className="mt-3 text-sm text-fg-muted">
          <Trans>Loading global exclusions…</Trans>
        </p>
      ) : (
        <div className="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1fr)_18rem]">
          <div>
            <ExclusionList
              global
              patterns={(save.isPending ? save.variables : exclusions.data) ?? []}
              onChange={(patterns) => save.mutate(patterns)}
              isDisabled={save.isPending}
            />
          </div>
          <ExclusionSyntax />
        </div>
      )}
      {save.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {save.error.message}
        </p>
      )}
    </SettingsSection>
  );
}

/** The latest scan of a scanner: progress, its review list or its outcome. */
function ScanStatus({ job, snapshot }: { job: ScanJob; snapshot: EmulationSnapshot }) {
  const { t } = useLingui();
  if (job.status === "review")
    return <ScanReview key={job.id} job={job} platforms={snapshot.platforms} />;
  return (
    <p role="status" data-part="scan-status" className="text-sm text-fg-muted">
      {job.status === "running"
        ? job.progress
          ? t({
              message: plural(
                { files: job.progress.files },
                { one: "Scanning… # file", other: "Scanning… # files" },
              ),
            })
          : t`Scanning…`
        : job.status === "done" && job.summary
          ? t`Last scan: ${{ added: job.summary.added }} added, ${{ updated: job.summary.updated }} updated, ${{ missing: job.summary.missing }} missing.`
          : job.status === "cancelled"
            ? t`Scan cancelled.`
            : job.error
              ? t`Scan failed: ${{ error: job.error }}`
              : t`Scan failed: unknown error`}
    </p>
  );
}

const emptyScanner = (snapshot: EmulationSnapshot, name: string): RomScanner => {
  const first = profileOptions(snapshot)[0];
  return {
    id: "",
    name,
    folder: "",
    platform: null,
    profile: { emulatorId: first?.emulatorId ?? "", profileId: first?.profileId ?? "" },
    launchWith: { type: "platform_default" },
    recursive: true,
    scanArchives: false,
    skipCloudPlaceholders: true,
    exclusions: [],
    inLibraryUpdate: true,
  };
};

export function ScannersSection({ snapshot }: { snapshot: EmulationSnapshot }) {
  const { t } = useLingui();
  const api = useEmulationApi();
  const client = useQueryClient();
  const scanners = useScanners();
  const jobs = useScanJobs();
  const [selectedId, select] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const scan = useMutation({
    mutationFn: (id: string) => api.startScan(id, "review"),
    onSuccess: (job) => client.setQueryData<ScanJob[]>(scanJobsKey, (old) => mergeScan(old, job)),
  });
  const toggle = useMutation({
    mutationFn: (scanner: RomScanner) => api.saveScanner(scanner),
    onSuccess: () => client.invalidateQueries({ queryKey: emulationKey }),
  });
  const open = (id: string) => {
    setAdding(false);
    select(id);
  };
  const startScan = (id: string) => {
    open(id);
    scan.mutate(id);
  };
  // A right click on a scanner offers its row's actions.
  const menus = useActionMenus();
  if (scanners.isPending)
    return (
      <p role="status" className="p-5 text-sm text-fg-muted">
        <Trans>Loading ROM scanners…</Trans>
      </p>
    );
  if (scanners.error)
    return (
      <div role="alert" className="rounded-panel border border-border p-5 text-sm text-fg-muted">
        <p>
          <Trans>ROM scanners could not be loaded.</Trans>
        </p>
        <p className="mt-2">{scanners.error.message}</p>
        <Button className="mt-3" onPress={() => void scanners.refetch()}>
          <Trans>Try again</Trans>
        </Button>
      </div>
    );
  const rows = scanners.data ?? [];
  const selected = adding
    ? undefined
    : (rows.find((row) => row.scanner.id === selectedId) ?? rows[0]);
  const latest = (id: string) => jobs.data?.find((job) => job.scannerId === id);
  const profiles = profileOptions(snapshot);
  const profileName = (row: ScannerRow) =>
    profiles.find(
      (option) => option.id === ref(row.scanner.profile.emulatorId, row.scanner.profile.profileId),
    )?.label;
  const current = selected && latest(selected.scanner.id);
  return (
    <div data-cs="rom-scanners" className="flex flex-col gap-4">
      <SettingsSection
        icon={FolderSearch}
        title={t`ROM Scanners`}
        description={t`Saved scans that turn ROM folders into games. Multi-disc games and playlists import as one game.`}
        aside={
          <Button
            size="sm"
            icon={Plus}
            onPress={() => {
              setAdding(true);
              select(null);
            }}
          >
            <Trans>Add scanner</Trans>
          </Button>
        }
      >
        {rows.length === 0 ? (
          <p className="mt-3 rounded-control border border-dashed border-border p-5 text-sm text-fg-subtle">
            <Trans>No ROM scanners yet. Add one for each ROM folder.</Trans>
          </p>
        ) : (
          <div className="mt-3 overflow-x-auto rounded-control border border-border">
            <table className="w-full text-left text-sm">
              <thead className="bg-surface-overlay font-mono text-[11px] tracking-wider text-fg-subtle uppercase">
                <tr>
                  <th className="px-4 py-2.5 font-medium">
                    <Trans comment="ROM scanners table column">Scanner</Trans>
                  </th>
                  <th className="px-4 py-2.5 font-medium">
                    <Trans comment="ROM scanners table column">Folder</Trans>
                  </th>
                  <th className="px-4 py-2.5 font-medium">
                    <Trans comment="ROM scanners table column">Platform</Trans>
                  </th>
                  <th className="px-4 py-2.5 font-medium">
                    <Trans comment="ROM scanners table column">Emulator · Profile</Trans>
                  </th>
                  <th className="px-4 py-2.5 font-medium">
                    <Trans comment="ROM scanners table column">Launch with</Trans>
                  </th>
                  <th className="px-4 py-2.5 text-right font-medium">
                    <Trans comment="ROM scanners table column">Games</Trans>
                  </th>
                  <th className="px-4 py-2.5 font-medium">
                    <Trans comment="ROM scanners table column">Last scan</Trans>
                  </th>
                  <th className="px-4 py-2.5 font-medium">
                    <Trans comment="ROM scanners table column: whether library updates run the scanner">
                      Auto
                    </Trans>
                  </th>
                  <th className="px-4 py-2.5">
                    <span className="sr-only">
                      <Trans comment="ROM scanners table column">Actions</Trans>
                    </span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const { scanner } = row;
                  const running = latest(scanner.id)?.status === "running";
                  const launchMode = launchModes.find(
                    (option) => option.id === scanner.launchWith.type,
                  );
                  return (
                    <tr
                      key={scanner.id}
                      {...menus.target(scanner.name, [
                        {
                          id: "scan",
                          label: t({ message: "Scan", comment: "Run the ROM scanner" }),
                          icon: Play,
                          isDisabled: running || Boolean(row.problem) || scan.isPending,
                          onAction: () => startScan(scanner.id),
                        },
                        {
                          id: "edit",
                          label: t`Edit scanner`,
                          icon: Pencil,
                          onAction: () => open(scanner.id),
                        },
                        {
                          id: "auto",
                          label: scanner.inLibraryUpdate
                            ? t`Leave out of library updates`
                            : t`Include in library updates`,
                          icon: RefreshCw,
                          isDisabled: toggle.isPending,
                          onAction: () =>
                            toggle.mutate({
                              ...scanner,
                              inLibraryUpdate: !scanner.inLibraryUpdate,
                            }),
                        },
                      ])}
                      data-part="scanner-row"
                      data-selected={scanner.id === selected?.scanner.id || undefined}
                      className="border-t border-border transition-colors duration-(--cs-motion-duration-fast) data-selected:bg-accent/10"
                    >
                      <td className="px-4 py-2.5">
                        <AriaButton
                          onPress={() => open(scanner.id)}
                          aria-pressed={scanner.id === selected?.scanner.id}
                          className="rounded-[4px] text-left font-medium data-focus-visible:focus-ring data-hovered:text-accent-alt"
                        >
                          {scanner.name}
                        </AriaButton>
                      </td>
                      <td className="max-w-56 truncate px-4 py-2.5 font-mono text-xs text-fg-muted">
                        {scanner.folder}
                      </td>
                      <td className="px-4 py-2.5 text-xs">
                        {row.platforms.map((id) => platformShort(snapshot, id)).join(", ") || "—"}
                      </td>
                      <td className="px-4 py-2.5">
                        {row.problem ? (
                          <StatusDot tone="danger">{row.problem}</StatusDot>
                        ) : (
                          profileName(row)
                        )}
                      </td>
                      <td className="px-4 py-2.5 text-xs text-fg-muted">
                        {launchMode && t(launchMode.label)}
                      </td>
                      <td className="px-4 py-2.5 text-right font-mono">{row.games}</td>
                      <td className="px-4 py-2.5 text-xs whitespace-nowrap text-fg-muted">
                        {running ? (
                          <StatusDot tone="warning">
                            <Trans>Scanning…</Trans>
                          </StatusDot>
                        ) : row.lastScanMs ? (
                          formatDateTime(row.lastScanMs)
                        ) : (
                          t({ message: "Never", comment: "A scanner's last scan" })
                        )}
                      </td>
                      <td className="px-4 py-2.5">
                        <Switch
                          size="sm"
                          aria-label={t`Include ${{ name: scanner.name }} in library update`}
                          isSelected={scanner.inLibraryUpdate}
                          isDisabled={toggle.isPending}
                          onChange={(on) => toggle.mutate({ ...scanner, inLibraryUpdate: on })}
                        />
                      </td>
                      <td className="px-4 py-2.5 text-right">
                        <Button
                          size="sm"
                          icon={Play}
                          isDisabled={running || Boolean(row.problem) || scan.isPending}
                          onPress={() => startScan(scanner.id)}
                          aria-label={t`Scan ${{ name: scanner.name }}`}
                        >
                          <Trans comment="Run the ROM scanner">Scan</Trans>
                        </Button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        {menus.menu}
        {(scan.error || toggle.error) && (
          <p role="alert" className="mt-3 text-sm text-danger">
            {(scan.error ?? toggle.error)?.message}
          </p>
        )}
      </SettingsSection>
      {current && <ScanStatus job={current} snapshot={snapshot} />}
      {adding ? (
        <ScannerEditor
          key="new"
          initial={emptyScanner(
            snapshot,
            t({ message: "New scanner", comment: "A new ROM scanner's name until it's renamed" }),
          )}
          snapshot={snapshot}
          saved={(scanner) => {
            void client.invalidateQueries({ queryKey: emulationKey });
            // A new scanner's first run is a review (§6.1).
            startScan(scanner.id);
          }}
          removed={() => setAdding(false)}
        />
      ) : (
        selected && (
          <ScannerEditor
            key={selected.scanner.id}
            initial={selected.scanner}
            row={selected}
            snapshot={snapshot}
            saved={() => void client.invalidateQueries({ queryKey: emulationKey })}
            removed={() => {
              select(null);
              void client.invalidateQueries({ queryKey: emulationKey });
            }}
          />
        )
      )}
      <GlobalExclusions />
    </div>
  );
}
