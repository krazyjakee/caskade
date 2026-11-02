import { Button, Select, TextField } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { FilePlus, FolderPlus, RotateCcw, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import {
  type LocationSpec,
  type SaveMatch,
  type SavePreferences,
  useSaveLocations,
} from "../../api/saves";
import { useDropZone } from "../../drop/DropProvider";
import { usePathPicker } from "../pathBrowser/PathPicker";
import type { GameEditorTabProps } from "./types";

/** A dropped path's kind, until the core looks at it: the core scans what is
 * there, so a guess only labels the row before it is saved. */
const droppedKind = (path: string): LocationSpec["kind"] =>
  /\.[^./\\]+$/.test(path.split(/[/\\]/).pop() ?? "") ? "file" : "folder";

/** The game editor's Saves tab (#238): one release's save locations on this
 * device, automatic or the user's own, and the manifest game it matches. */
export function SavesTab({ details, onDirtyChange }: GameEditorTabProps) {
  const { t } = useLingui();
  const [selected, setSelected] = useState<string | null>(null);
  const entry =
    details.entries.find((entry) => entry.id === selected) ??
    details.entries.find((entry) => entry.id === details.defaultEntryId) ??
    details.entries[0];
  const { query, edit, available } = useSaveLocations(entry?.id);
  const picker = usePathPicker();
  // The revision the edit started from, so a change made elsewhere is refused.
  const [draft, setDraft] = useState<{ preferences: SavePreferences; revision: number } | null>(
    null,
  );
  const [pickerError, setPickerError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const state = query.data;
  const preferences = draft?.preferences ?? state?.preferences;
  const dirty =
    draft !== null && JSON.stringify(draft.preferences) !== JSON.stringify(state?.preferences);
  useEffect(() => onDirtyChange(dirty), [dirty, onDirtyChange]);
  const specs = preferences?.locations ?? state?.automatic.map((location) => location.spec) ?? [];
  // What the core found at each path: its kind and how many files it holds.
  const resolved = (spec: LocationSpec) =>
    [...(state?.locations ?? []), ...(state?.automatic ?? [])].find(
      (location) => location.spec.path === spec.path,
    );
  const change = (next: SavePreferences) => {
    if (!state) return;
    setDraft({ preferences: next, revision: draft?.revision ?? state.revision });
    setSaved(false);
    edit.reset();
  };
  const addPaths = (paths: { kind: LocationSpec["kind"]; path: string }[]) => {
    if (!preferences) return;
    const added = paths.filter(
      (spec, index) =>
        !specs.some((location) => location.path === spec.path) &&
        paths.findIndex((other) => other.path === spec.path) === index,
    );
    if (added.length > 0) change({ ...preferences, locations: [...specs, ...added] });
  };
  const add = async (kind: "file" | "folder") => {
    setPickerError(null);
    try {
      const path = await picker.pick({
        kind,
        title: kind === "folder" ? t`Choose a save folder` : t`Choose a save file`,
      });
      if (path) addPaths([{ kind, path }]);
    } catch (error) {
      setPickerError(error instanceof Error ? error.message : String(error));
    }
  };
  const busy = edit.isPending || !state;
  const drop = useDropZone<HTMLDivElement>(
    busy ? null : (paths) => addPaths(paths.map((path) => ({ kind: droppedKind(path), path }))),
  );
  const remove = (spec: LocationSpec) => {
    if (preferences)
      change({
        ...preferences,
        locations: specs.filter((location) => location.path !== spec.path),
      });
  };
  if (!entry)
    return (
      <p className="text-sm text-fg-muted">
        <Trans>This game has no releases.</Trans>
      </p>
    );
  if (!available)
    return (
      <p className="text-sm text-fg-muted">
        <Trans>Save locations are unavailable here.</Trans>
      </p>
    );
  const kindLabel = (kind: LocationSpec["kind"]) =>
    kind === "folder" ? t`Folder` : kind === "file" ? t`File` : t`Registry key`;
  const matchText = (matched: SaveMatch) => {
    if (matched.type === "unknown")
      return t`No automatic save locations are known for this release. Add its save folders or files below, or set the manifest game it matches.`;
    if (matched.type === "ambiguous") {
      const titles = matched.titles.join(", ");
      return t`More than one manifest game matches: ${titles}. Enter the right one's name below.`;
    }
    const title = matched.title;
    return t`Save locations from ${title}.`;
  };
  return (
    <div
      ref={drop.ref}
      data-cs="game-saves"
      data-choice={preferences?.locations == null ? "automatic" : "chosen"}
      className="flex flex-col gap-4"
    >
      {details.entries.length > 1 && (
        <Select
          label={t`Release`}
          value={entry.id}
          isDisabled={dirty || edit.isPending}
          options={details.entries.map((entry) => ({
            id: entry.id,
            label: `${entry.source} · ${entry.location?.split(/[/\\]/).filter(Boolean).pop() ?? entry.id}`,
          }))}
          onChange={(id) => {
            setSelected(id);
            setDraft(null);
            edit.reset();
            setSaved(false);
          }}
        />
      )}
      {query.isPending ? (
        <p role="status" className="text-sm text-fg-muted">
          <Trans>Finding save locations…</Trans>
        </p>
      ) : (
        state &&
        preferences && (
          <>
            <p data-part="match" data-match={state.matched.type} className="text-sm text-fg-muted">
              {matchText(state.matched)}
            </p>
            <TextField
              label={t`Manifest game name`}
              description={t`Leave blank to match automatically. A name from the Ludusavi manifest sets the match by hand.`}
              value={preferences.manifestTitle ?? ""}
              onChange={(value) =>
                change({ ...preferences, manifestTitle: value.trim() ? value : null })
              }
              isDisabled={edit.isPending}
            />
            <p data-part="mode" className="text-sm text-fg-muted">
              {preferences.locations == null
                ? t`Using automatic locations.`
                : t`Using your chosen locations.`}
            </p>
            <ul
              data-part="locations"
              aria-label={t`Save locations`}
              className="flex flex-col gap-2"
            >
              {specs.map((spec) => {
                const found = resolved(spec);
                const kind = found?.spec.kind ?? spec.kind;
                const files = found?.fileCount ?? 0;
                return (
                  <li
                    key={`${spec.kind}:${spec.path}`}
                    data-part="location"
                    data-kind={kind}
                    className="flex items-center gap-3 rounded-control border border-border p-3"
                  >
                    <div className="min-w-0 flex-1">
                      <span className="text-xs text-fg-muted">
                        {kindLabel(kind)}
                        {kind !== "registry" &&
                          ` · ${t({ message: plural({ count: files }, { one: "# save file", other: "# save files" }) })}`}
                      </span>
                      <p data-part="path" className="break-all font-mono text-sm">
                        {spec.path}
                      </p>
                    </div>
                    <Button
                      icon={Trash2}
                      aria-label={t`Remove ${spec.path}`}
                      isDisabled={edit.isPending}
                      onPress={() => remove(spec)}
                      data-spatial="focusable"
                    />
                  </li>
                );
              })}
            </ul>
            {specs.length === 0 && (
              <p data-part="empty" className="text-sm text-fg-muted">
                <Trans>No save files or folders. Add them here, or drop them on this tab.</Trans>
              </p>
            )}
            {state.warnings.length > 0 && (
              <ul data-part="warnings" className="flex flex-col gap-1 text-sm text-fg-muted">
                {state.warnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            )}
            <div className="flex flex-wrap gap-2">
              <Button
                icon={FolderPlus}
                isDisabled={edit.isPending}
                onPress={() => void add("folder")}
                data-spatial="focusable"
              >{t`Add folder`}</Button>
              <Button
                icon={FilePlus}
                isDisabled={edit.isPending}
                onPress={() => void add("file")}
                data-spatial="focusable"
              >{t`Add file`}</Button>
              <Button
                icon={RotateCcw}
                isDisabled={
                  edit.isPending ||
                  (preferences.locations == null && preferences.manifestTitle == null)
                }
                onPress={() => change({ locations: null, manifestTitle: null })}
                data-spatial="focusable"
              >{t`Reset to automatic`}</Button>
            </div>
            <div className="flex gap-2">
              <Button
                variant="primary"
                isDisabled={!dirty || edit.isPending}
                onPress={() =>
                  draft &&
                  edit.mutate(draft, {
                    onSuccess: () => {
                      setDraft(null);
                      setSaved(true);
                    },
                  })
                }
                data-spatial="focusable"
              >
                {edit.isPending ? t`Saving…` : t`Save locations`}
              </Button>
              {/* Only while there is something to undo: the other tabs stay
                  mounted beside this one, with Revert buttons of their own. */}
              {dirty && (
                <Button
                  isDisabled={edit.isPending}
                  onPress={() => {
                    setDraft(null);
                    edit.reset();
                  }}
                  data-spatial="focusable"
                >{t`Revert`}</Button>
              )}
            </div>
            {saved && (
              <p role="status" className="text-sm">
                <Trans>Save locations saved.</Trans>
              </p>
            )}
          </>
        )
      )}
      {(query.error || edit.error || pickerError) && (
        <p data-part="error" role="alert" className="text-sm text-danger">
          {query.error?.message ?? edit.error?.message ?? pickerError}
        </p>
      )}
    </div>
  );
}
