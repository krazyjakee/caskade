/** The editor's Actions tab (#178): each release's launch actions. Its own
 * launch first, then the play actions and tools the user added. Add,
 * duplicate, reorder, rename and delete them, choose what Play starts or
 * have it ask, and save one action at a time. */
import { Button, Select } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { ArrowDown, ArrowUp, Copy, Gamepad2, Plus, Trash2, Wrench } from "lucide-react";
import { useEffect, useState } from "react";
import { ListBox, ListBoxItem } from "react-aria-components";
import {
  blankAction,
  type GameAction,
  isBuiltIn,
  type ReleaseActions,
  useActionChanges,
  useGameActions,
} from "../../api/actions";
import type { GameEntry } from "../../api/types";
import { ActionForm } from "../gameActions/ActionForm";
import type { GameEditorTabProps } from "./types";

const ASK = "ask";

/** A release's name in the picker: its folder's name, else its source. */
function releaseName(entry: GameEntry | undefined, fallback: string) {
  if (!entry) return fallback;
  const folder = entry.location?.split(/[/\\]/).filter(Boolean).pop();
  return folder ? `${folder} · ${entry.source}` : entry.source;
}

export function ActionsTab({ gameId, details, onDirtyChange }: GameEditorTabProps) {
  const { t } = useLingui();
  const query = useGameActions(gameId);
  const changes = useActionChanges(gameId);
  const releases = query.data ?? [];
  const [entryId, setEntryId] = useState<string | null>(null);
  const release: ReleaseActions | undefined =
    releases.find((each) => each.entryId === entryId) ??
    releases.find((each) => each.entryId === details.defaultEntryId) ??
    releases[0];
  // The action being edited: a saved one's id, or a new one's draft.
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<GameAction | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const actions = release?.actions ?? [];
  // The saved action shown: the one chosen, else the first.
  const shown = actions.find((action) => action.id === (selected ?? actions[0]?.id)) ?? null;
  const editing = draft ?? shown;
  // The saved version of what is being edited; none for a new action.
  const saved = draft && draft.id !== shown?.id ? null : shown;
  const dirty = draft !== null && JSON.stringify(draft) !== JSON.stringify(saved);
  useEffect(() => onDirtyChange(dirty), [dirty, onDirtyChange]);

  const choose = (id: string | null) => {
    setSelected(id);
    setDraft(null);
    setConfirmDelete(false);
    changes.save.reset();
  };
  const startNew = (kind: GameAction["kind"]) => {
    setSelected(null);
    setConfirmDelete(false);
    changes.save.reset();
    setDraft({
      ...blankAction(release?.entryId ?? "", kind),
      name: kind === "play" ? t`Play (modded)` : t`New tool`,
    });
  };
  const duplicate = (action: GameAction) => {
    setSelected(null);
    changes.save.reset();
    setDraft({
      ...structuredClone(action),
      id: "",
      name: t({ message: `${action.name} (copy)`, comment: "A duplicated launch action's name" }),
    });
  };
  const save = () => {
    if (!editing) return;
    changes.save.mutate(editing, {
      onSuccess: (action) => {
        setSelected(action.id);
        setDraft(null);
        if (!release && action.entryId) setEntryId(action.entryId);
      },
    });
  };
  const added = actions.filter((action) => !isBuiltIn(action));
  const move = (action: GameAction, by: -1 | 1) => {
    if (!release) return;
    const ids = added.map((each) => each.id);
    const at = ids.indexOf(action.id);
    const to = at + by;
    if (at < 0 || to < 0 || to >= ids.length) return;
    ids.splice(to, 0, ...ids.splice(at, 1));
    changes.order.mutate({ entryId: release.entryId, ids });
  };
  const play = actions.filter((action) => action.kind === "play");
  const settings = release?.settings;
  const current = settings?.ask
    ? ASK
    : ((play.find((action) => action.id === settings?.defaultAction) ?? play[0])?.id ?? null);
  const builtIn = editing ? isBuiltIn(editing) && editing.id !== "" : false;
  const failure =
    changes.save.error?.message ??
    changes.remove.error?.message ??
    changes.order.error?.message ??
    changes.settings.error?.message;

  if (query.isPending)
    return (
      <p role="status" className="text-sm text-fg-muted">
        <Trans>Loading launch actions…</Trans>
      </p>
    );
  if (query.isError)
    return (
      <p role="alert" className="text-sm text-danger">
        {query.error.message}
      </p>
    );
  return (
    <div data-cs="game-actions" className="flex flex-col gap-5">
      {releases.length > 1 && release && (
        <Select
          label={t({ message: "Release", comment: "Picks which release's actions to edit" })}
          options={releases.map((each) => ({
            id: each.entryId,
            label: releaseName(
              details.entries.find((entry) => entry.id === each.entryId),
              each.entryId,
            ),
          }))}
          value={release.entryId}
          onChange={(next) => {
            setEntryId(next);
            choose(null);
          }}
          isDisabled={dirty}
        />
      )}
      {release && play.length > 1 && current && (
        <Select
          data-part="default"
          label={t({
            message: "Play starts",
            comment: "Picks which of a release's launch actions Play starts",
          })}
          options={[
            ...play.map((action) => ({ id: action.id, label: action.name })),
            { id: ASK, label: t`Ask each time` },
          ]}
          value={current}
          onChange={(value) =>
            settings &&
            changes.settings.mutate({
              entryId: release.entryId,
              settings:
                value === ASK
                  ? { ...settings, ask: true }
                  : { ...settings, ask: false, defaultAction: value },
            })
          }
          isDisabled={changes.settings.isPending}
        />
      )}
      <div className="grid gap-5 md:grid-cols-[minmax(12rem,16rem)_1fr]">
        <div className="flex flex-col gap-3">
          {actions.length > 0 ? (
            <ListBox
              data-part="actions"
              aria-label={t`Launch actions`}
              selectionMode="single"
              disallowEmptySelection
              selectedKeys={editing?.id ? [editing.id] : []}
              onSelectionChange={(keys) => {
                const [key] = [...keys];
                if (typeof key === "string" && !dirty) choose(key);
              }}
              className="flex flex-col gap-1"
            >
              {actions.map((action) => (
                <ListBoxItem
                  key={action.id}
                  id={action.id}
                  textValue={action.name}
                  data-part="action"
                  data-kind={action.kind}
                  data-spatial="focusable"
                  isDisabled={dirty && action.id !== editing?.id}
                  className="flex cursor-pointer items-center gap-2 rounded-control border border-border px-3 py-2 text-sm outline-none data-disabled:opacity-50 data-focus-visible:focus-ring data-selected:border-accent-alt data-selected:bg-accent/10"
                >
                  {action.kind === "play" ? (
                    <Gamepad2 aria-hidden="true" className="size-4 shrink-0" />
                  ) : (
                    <Wrench aria-hidden="true" className="size-4 shrink-0" />
                  )}
                  <span className="min-w-0 flex-1 truncate">{action.name}</span>
                  {!settings?.ask && play.length > 1 && action.id === current && (
                    <span className="font-mono text-[10px] tracking-widest text-accent-alt uppercase">
                      {t({ message: "Default", context: "launch action" })}
                    </span>
                  )}
                </ListBoxItem>
              ))}
            </ListBox>
          ) : (
            <p className="text-sm text-fg-muted">
              <Trans>
                Nothing launches this game yet. Add a play action to start it from Caskade, or a
                tool to run alongside it.
              </Trans>
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            <Button
              data-spatial="focusable"
              icon={Plus}
              isDisabled={dirty}
              onPress={() => startNew("play")}
            >
              {t`Add play action`}
            </Button>
            <Button
              data-spatial="focusable"
              icon={Plus}
              isDisabled={dirty}
              onPress={() => startNew("utility")}
            >
              {t`Add tool`}
            </Button>
          </div>
        </div>
        {editing && (
          <div className="flex flex-col gap-4">
            <ActionForm
              action={editing}
              onChange={setDraft}
              targetLocked={builtIn && !release?.editableBuiltIn}
              isDisabled={changes.save.isPending}
            />
            {failure && (
              <p role="alert" data-part="error" className="text-sm text-danger">
                {failure}
              </p>
            )}
            <div className="flex flex-wrap items-center gap-2">
              <Button
                data-spatial="focusable"
                variant="primary"
                isDisabled={!dirty || changes.save.isPending}
                onPress={save}
              >
                {changes.save.isPending ? t`Saving…` : t`Save action`}
              </Button>
              <Button
                data-spatial="focusable"
                isDisabled={!dirty || changes.save.isPending}
                onPress={() => {
                  setDraft(null);
                  changes.save.reset();
                }}
              >
                {t`Revert`}
              </Button>
              {saved && (
                <Button
                  data-spatial="focusable"
                  icon={Copy}
                  isDisabled={dirty}
                  onPress={() => duplicate(saved)}
                >
                  {t`Duplicate`}
                </Button>
              )}
              {saved && !isBuiltIn(saved) && (
                <>
                  <Button
                    data-spatial="focusable"
                    aria-label={t`Move up`}
                    icon={ArrowUp}
                    isDisabled={dirty || added[0]?.id === saved.id || changes.order.isPending}
                    onPress={() => move(saved, -1)}
                  />
                  <Button
                    data-spatial="focusable"
                    aria-label={t`Move down`}
                    icon={ArrowDown}
                    isDisabled={
                      dirty || added[added.length - 1]?.id === saved.id || changes.order.isPending
                    }
                    onPress={() => move(saved, 1)}
                  />
                  <Button
                    data-spatial="focusable"
                    data-part="delete"
                    variant="danger"
                    icon={Trash2}
                    isDisabled={changes.remove.isPending}
                    onPress={() => {
                      if (!confirmDelete) {
                        setConfirmDelete(true);
                        return;
                      }
                      changes.remove.mutate(saved.id, { onSuccess: () => choose(null) });
                    }}
                  >
                    {confirmDelete ? t`Delete “${saved.name}”?` : t`Delete…`}
                  </Button>
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
