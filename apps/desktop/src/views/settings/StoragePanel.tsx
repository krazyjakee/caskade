import { Button, Select, SettingRow, SettingsSection, Switch, TextField } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderPlus, HardDrive, RefreshCw, Star, Trash2 } from "lucide-react";
import { useState } from "react";
import { Button as AriaButton } from "react-aria-components";
import { formatPath } from "../../api/presentation";
import { formatBytes, type RootStatus, type RootSummary, type StorageApi } from "../../api/storage";
import { useActionMenus } from "../../components/ActionMenu";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { usePathPicker } from "../../components/pathBrowser/PathPicker";
import { useDropZone } from "../../drop/DropProvider";
import { UnsavedHint, useUnsavedChanges } from "./unsavedChanges";
import { SettingsFeedback, useSettingsSection } from "./useSettings";

const statuses: Record<RootStatus, MessageDescriptor> = {
  online: msg({ message: "online", context: "storage location status" }),
  offline: msg({ message: "offline", context: "storage location status" }),
  read_only: msg({ message: "read only", context: "storage location status" }),
  identity_mismatch: msg({
    message: "identity mismatch",
    context: "storage location status",
    comment: "The folder at the location's path isn't the location Caskade registered",
  }),
};

const locationName = ({ root }: RootSummary) => root.label ?? formatPath(root.path);

export function StoragePanel({ api }: { api: StorageApi }) {
  const { t } = useLingui();
  const statusLabel = (status: RootStatus) => t(statuses[status]);
  const gameCount = (total: number) => plural({ total }, { one: "# game", other: "# games" });
  const client = useQueryClient();
  const picker = usePathPicker();
  const query = useQuery({
    queryKey: ["storage", "roots"],
    queryFn: () => api.list(),
    refetchOnWindowFocus: true,
  });
  const prefs = useSettingsSection<{ alwaysUseDefault: boolean }>("core.storage");
  const [selected, setSelected] = useState<string | null>(null);
  const [path, setPath] = useState("");
  const [label, setLabel] = useState("");
  // A folder dropped on "Add a location" fills in its path.
  const addDrop = useDropZone<HTMLDivElement>((paths) => setPath(paths[0] ?? ""));
  const [target, setTarget] = useState("");
  const [remove, setRemove] = useState(false);
  const [confirmMove, setConfirmMove] = useState(false);
  const [message, setMessage] = useState("");
  const roots = query.data ?? [];
  const active = roots.find((r) => r.root.id === selected) ?? roots[0];
  const operation = useMutation({
    mutationFn: async (run: () => Promise<void>) => run(),
    // A move that stopped part way changed the locations too.
    onSettled: () => {
      void client.invalidateQueries({ queryKey: ["storage"] });
      void client.invalidateQueries({ queryKey: ["library"] });
    },
  });
  const run = (action: () => Promise<void>) => {
    setMessage("");
    operation.mutate(action);
  };
  const destinations = roots.filter((r) => r.root.id !== active?.root.id && r.status === "online");
  const destination = destinations.some((r) => r.root.id === target)
    ? target
    : (destinations[0]?.root.id ?? "");
  const destinationRoot = destinations.find((r) => r.root.id === destination);
  const error = query.error ?? operation.error;
  // A right click on a location offers its buttons.
  const menus = useActionMenus();
  const choose = (id: string) => {
    setSelected(id);
    setRemove(false);
  };
  return (
    <div data-cs="storage-settings" className="flex flex-col gap-6">
      <SettingsFeedback error={error} saving={operation.isPending} />
      <SettingsSection
        icon={HardDrive}
        title={t`Storage locations`}
        description={t`Choose where Caskade installs games. Disconnected locations remain in your library.`}
      >
        <div className="flex items-center justify-between gap-3 py-3">
          <p className="text-sm text-fg-muted">
            {query.isError
              ? t`Locations unavailable`
              : plural(
                  { locations: roots.length },
                  { one: "# registered location", other: "# registered locations" },
                )}
          </p>
          <Button
            isDisabled={query.isFetching}
            onPress={() => {
              void query.refetch();
            }}
          >
            <RefreshCw className="size-4" /> <Trans>Refresh</Trans>
          </Button>
        </div>
        {query.isPending && (
          <p role="status">
            <Trans>Checking locations…</Trans>
          </p>
        )}
        {!query.isPending && !query.isError && roots.length === 0 && (
          <p className="p-3 text-sm text-fg-muted">
            <Trans>Add a folder below to choose your first install location.</Trans>
          </p>
        )}
        <div className="grid gap-3 md:grid-cols-2">
          {roots.map((root) => (
            // A card, not a `Button`: that one's own height, centring and
            // nowrap would squash the card into one overflowing line.
            <AriaButton
              key={root.root.id}
              {...menus.target(root.root.label ?? formatPath(root.root.path), [
                {
                  id: "default",
                  label: t({ message: "Make default", context: "storage location" }),
                  icon: Star,
                  isDisabled:
                    root.root.isDefault || root.status !== "online" || operation.isPending,
                  onAction: () =>
                    run(async () => {
                      await api.setDefault(root.root.id);
                    }),
                },
                {
                  id: "remove",
                  label: t`Remove location…`,
                  icon: Trash2,
                  tone: "danger",
                  isDisabled: root.root.isDefault || operation.isPending,
                  description: root.root.isDefault ? t`Choose another default first` : undefined,
                  onAction: () => {
                    choose(root.root.id);
                    setRemove(true);
                  },
                },
              ])}
              data-status={root.status}
              data-selected={root.root.id === active?.root.id || undefined}
              onPress={() => choose(root.root.id)}
              className="block w-full min-w-0 rounded-panel border border-border p-4 text-left outline-none transition-colors data-hovered:border-accent-alt/50 data-focus-visible:focus-ring data-selected:border-accent-alt"
            >
              <div className="flex justify-between gap-3">
                <strong>{root.root.label ?? t`Game storage`}</strong>
                <span className="text-xs text-fg-muted">
                  {root.root.isDefault
                    ? t({
                        message: `Default · ${{ status: statusLabel(root.status) }}`,
                        comment: "The default storage location and its status",
                      })
                    : statusLabel(root.status)}
                </span>
              </div>
              <p className="mt-1 break-all font-mono text-xs text-fg-muted">
                {formatPath(root.root.path)}
              </p>
              <Capacity root={root} />
              <p className="mt-2 text-xs text-fg-muted">
                {t({
                  message: `${{ games: gameCount(root.games.length) }} · ${{ free: formatBytes(root.capabilities.freeBytes) }} free / ${{
                    size: formatBytes(root.capabilities.totalBytes),
                  }} total`,
                  comment: "A storage location's games, free space and size",
                })}
                {root.capabilities.filesystem ? ` · ${root.capabilities.filesystem}` : ""}
                {root.capabilities.volume ? ` · ${root.capabilities.volume}` : ""}
              </p>
              {root.capabilities.warnings.map((warning) => (
                <p key={warning} className="mt-1 text-xs text-warning">
                  {warning}
                </p>
              ))}
            </AriaButton>
          ))}
        </div>
        {menus.menu}
        <div
          ref={addDrop.ref}
          data-part="add-root"
          className="mt-4 flex flex-col gap-3 rounded-panel border border-border p-4"
        >
          <h3 className="font-medium">
            <Trans>Add a location</Trans>
          </h3>
          <TextField
            label={t`Folder`}
            value={path}
            onChange={setPath}
            mono
            action={
              <Button
                isDisabled={operation.isPending}
                onPress={() =>
                  run(async () => {
                    const chosen = await picker.pick(
                      {
                        kind: "folder",
                        title: t`Add a storage location`,
                        startAt: path.trim() || null,
                      },
                      () => api.chooseDirectory(),
                    );
                    if (chosen) setPath(chosen);
                  })
                }
              >
                <Trans>Browse…</Trans>
              </Button>
            }
          />
          <TextField
            label={t({ message: "Label", comment: "A storage location's name" })}
            value={label}
            onChange={setLabel}
            placeholder={t({
              message: "Games SSD",
              comment: "An example name for a storage location",
            })}
          />
          <Button
            isDisabled={!path.trim() || operation.isPending}
            onPress={() =>
              run(async () => {
                const root = await api.add(path.trim(), label.trim() || undefined);
                setSelected(root.root.id);
                setPath("");
                setLabel("");
                setMessage(t`Location added. Caskade looked there for games it installed before.`);
              })
            }
          >
            <FolderPlus className="size-4" /> <Trans>Add location</Trans>
          </Button>
        </div>
      </SettingsSection>
      {/* Beside the locations it's about (#54). */}
      {message && (
        <p role="status" className="text-sm text-fg-muted">
          {message}
        </p>
      )}
      {active && (
        <SettingsSection
          title={active.root.label ?? t`Selected location`}
          description={formatPath(active.root.path)}
        >
          <RootEditor
            key={active.root.id}
            root={active}
            disabled={operation.isPending}
            onSave={async (name, template) => {
              setMessage("");
              try {
                await operation.mutateAsync(async () => {
                  await api.edit(active.root.id, name, template);
                });
                return true;
              } catch {
                // The failure shows with the location's other operations.
                return false;
              }
            }}
          />
          <div className="my-4 flex flex-wrap gap-2">
            <Button
              isDisabled={
                active.root.isDefault || active.status !== "online" || operation.isPending
              }
              onPress={() =>
                run(async () => {
                  await api.setDefault(active.root.id);
                })
              }
            >
              <Trans context="storage location">Make default</Trans>
            </Button>
            <Button
              isDisabled={active.root.isDefault || operation.isPending}
              onPress={() => setRemove((value) => !value)}
            >
              <Trans>Remove location…</Trans>
            </Button>
          </div>
          {active.root.isDefault && (
            <p className="text-xs text-fg-muted">
              <Trans>Choose another default before removing this location.</Trans>
            </p>
          )}
          {remove && (
            <section
              data-part="remove-root"
              className="my-3 flex flex-col gap-2 rounded-panel border border-danger/40 p-3"
            >
              {/* What `StorageService::remove_keep_files` does (#64). */}
              {active.games.length === 0 ? (
                <p className="text-sm">
                  <Trans>Remove this location from the library? Files on disk are kept.</Trans>
                </p>
              ) : (
                <>
                  <p className="text-sm">
                    {plural(
                      { games: active.games.length },
                      {
                        one: "Remove this location from the library? Its game leaves your library, with its play time, favourite and collections. If you also have that game another way, it stays, without this copy's play time.",
                        other:
                          "Remove this location from the library? Its # games leave your library, with their play time, favourites and collections. A game you also have another way stays, without this copy's play time.",
                      },
                    )}
                  </p>
                  <p className="text-sm">
                    <Trans>
                      Files on disk are kept. Add this folder again and Caskade finds them, as new
                      games: what left the library doesn't come back.
                    </Trans>
                  </p>
                </>
              )}
              <Button
                variant="danger"
                isDisabled={operation.isPending}
                onPress={() =>
                  run(async () => {
                    await api.remove(active.root.id);
                    setSelected(null);
                    setRemove(false);
                  })
                }
              >
                <Trans>Keep files and remove from library</Trans>
              </Button>
              {active.games.length > 0 && destinations.length > 0 && (
                <div className="flex flex-wrap items-end gap-2">
                  <Select
                    label={t`Move games to`}
                    options={destinations.map((root) => ({
                      id: root.root.id,
                      label: t({
                        message: `${{ location: root.root.label ?? formatPath(root.root.path) }} · ${{
                          free: formatBytes(root.capabilities.freeBytes),
                        }} free`,
                        comment: "A storage location to move games to, and its free space",
                      }),
                    }))}
                    value={destination}
                    onChange={setTarget}
                    isDisabled={operation.isPending}
                  />
                  <Button
                    isDisabled={
                      operation.isPending || active.games.some((game) => game.moveBlocked !== null)
                    }
                    onPress={() => setConfirmMove(true)}
                  >
                    {plural(
                      { games: active.games.length },
                      {
                        one: "Move its game there, then remove…",
                        other: "Move its # games there, then remove…",
                      },
                    )}
                  </Button>
                </div>
              )}
              <Button onPress={() => setRemove(false)}>
                <Trans>Cancel</Trans>
              </Button>
              {confirmMove && destinationRoot && (
                <MoveAndRemoveDialog
                  root={active}
                  to={destinationRoot}
                  close={() => setConfirmMove(false)}
                  onConfirm={() => {
                    setConfirmMove(false);
                    // One job in Activity: this waits for it to end.
                    run(async () => {
                      await api.moveAndRemove(active.root.id, destinationRoot.root.id);
                      setSelected(null);
                      setRemove(false);
                      setMessage(t`Move completed. Library identity and playtime were preserved.`);
                    });
                  }}
                />
              )}
            </section>
          )}
          {operation.isPending && (
            <p role="status" className="mt-3 text-sm text-fg-muted">
              <Trans>Working… progress and cancellation are available in Activity.</Trans>
            </p>
          )}
        </SettingsSection>
      )}
      <SettingsSection
        title={t`Install location preference`}
        description={t`Free space and required capabilities are checked for every job.`}
      >
        <SettingRow
          label={t`Always use the default location`}
          description={t`Skip the picker when the default location is available. A single eligible location is selected automatically.`}
        >
          <Switch
            isDisabled={!prefs.settings || prefs.saving}
            isSelected={prefs.settings?.alwaysUseDefault ?? false}
            onChange={(value) =>
              prefs.setSettings((current) => ({ ...current, alwaysUseDefault: value }))
            }
          />
        </SettingRow>
        <SettingsFeedback error={prefs.error} saving={prefs.saving} />
      </SettingsSection>
    </div>
  );
}
/** Asks once before a location's games move and it goes (#64). */
function MoveAndRemoveDialog({
  root,
  to,
  onConfirm,
  close,
}: {
  root: RootSummary;
  to: RootSummary;
  onConfirm: () => void;
  close: () => void;
}) {
  const { t } = useLingui();
  const from = locationName(root);
  const target = locationName(to);
  const size = formatBytes(root.gamesBytes);
  return (
    <ConfirmDialog
      title={plural(
        { games: root.games.length },
        { one: `Move its game to ${target}?`, other: `Move its # games to ${target}?` },
      )}
      confirmLabel={t`Move and remove`}
      onConfirm={onConfirm}
      close={close}
    >
      <p>
        <Trans>{size} to move, as one job you can follow or cancel in Activity.</Trans>
      </p>
      <p>
        <Trans>
          {from} is removed once every game has moved. If you cancel, or a game can't move, it stays
          and the games already moved stay on {target}.
        </Trans>
      </p>
    </ConfirmDialog>
  );
}
function RootEditor({
  root,
  disabled,
  onSave,
}: {
  root: RootSummary;
  disabled: boolean;
  /** Resolves to whether the change was saved. */
  onSave: (label: string, template: string) => Promise<boolean>;
}) {
  const [label, setLabel] = useState(root.root.label ?? "");
  const [template, setTemplate] = useState(root.folderTemplate);
  const { t } = useLingui();
  const dirty = label !== (root.root.label ?? "") || template !== root.folderTemplate;
  // Label and template wait for Save location (#43): leaving the section asks first.
  useUnsavedChanges(dirty, {
    label: root.root.label ?? t`Selected location`,
    save: () => onSave(label, template),
    discard: () => {
      setLabel(root.root.label ?? "");
      setTemplate(root.folderTemplate);
    },
  });
  return (
    <div className="mt-3 grid items-end gap-3 md:grid-cols-3">
      <TextField label={t`Location label`} value={label} onChange={setLabel} />
      <TextField
        label={t`Folder-name template`}
        description={t({
          message: `A single folder name containing ${{ title: "{title}" }}.`,
          comment: "{title} is shown as is: the template's placeholder for the game's title",
        })}
        value={template}
        onChange={setTemplate}
      />
      <div className="flex flex-col items-start gap-1">
        {dirty && <UnsavedHint dirty />}
        <Button isDisabled={disabled} onPress={() => void onSave(label, template)}>
          <Trans>Save location</Trans>
        </Button>
      </div>
    </div>
  );
}
function Capacity({ root }: { root: RootSummary }) {
  const { t } = useLingui();
  const total = root.capabilities.totalBytes;
  const percentage = (value: number) =>
    total ? `${Math.max(0, Math.min(100, (value / total) * 100))}%` : "0%";
  return (
    <div
      data-part="capacity"
      role="img"
      aria-label={t({
        message: `Games ${{ games: formatBytes(root.gamesBytes) }}, other files ${{
          other: formatBytes(root.otherBytes),
        }}, free ${{ free: formatBytes(root.capabilities.freeBytes) }}`,
        comment: "How a storage location's space is used",
      })}
      className="mt-3 flex h-2 overflow-hidden rounded-full bg-surface-base"
    >
      <div
        data-part="capacity-games"
        className="bg-accent"
        style={{ width: percentage(root.gamesBytes) }}
      />
      <div
        data-part="capacity-other"
        className="bg-fg-subtle/40"
        style={{ width: percentage(root.otherBytes ?? 0) }}
      />
    </div>
  );
}
