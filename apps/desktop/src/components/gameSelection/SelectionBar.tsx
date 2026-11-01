/** The bar of a view in selection mode (#173): how many games are chosen,
 * choosing the loaded ones or every game the view finds (with how many
 * that is), clearing, the bulk editor and leaving selection mode. */
import { Button } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { CheckCheck, Pencil, RefreshCw, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useGameEditApi } from "../../api/gameEdit";
import { errorMessage } from "../../api/reply";
import { BulkEditDialog } from "./BulkEditDialog";
import { RefreshDialog } from "./RefreshDialog";
import { type GameSelection, useSelectionVersion } from "./selection";

export function SelectionBar({
  selection,
  total,
  loadedIds,
  allIds,
}: {
  selection: GameSelection;
  /** How many games the view finds. */
  total: number;
  /** The view's games loaded so far. */
  loadedIds: () => string[];
  /** Every game the view finds; null where it can't list them. */
  allIds: (() => Promise<string[]>) | null;
}) {
  const { t } = useLingui();
  useSelectionVersion(selection);
  const editable = useGameEditApi() !== null;
  const [editing, setEditing] = useState<string[] | null>(null);
  const [refreshing, setRefreshing] = useState<string[] | null>(null);
  const [selecting, setSelecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    selection.onEdit = () => setEditing(selection.ids());
    return () => {
      selection.onEdit = null;
    };
  }, [selection]);
  if (!selection.active) return null;
  const count = selection.count;
  const selectAll = () => {
    if (!allIds) return;
    setSelecting(true);
    setError(null);
    allIds()
      .then((ids) => selection.add(ids))
      .catch((failure: unknown) => setError(errorMessage(failure)))
      .finally(() => setSelecting(false));
  };
  return (
    <div
      data-cs="selection-bar"
      role="toolbar"
      aria-label={t`Selected games`}
      className="sticky bottom-2 z-30 mx-auto mt-3 flex w-full max-w-4xl flex-wrap items-center gap-2 rounded-panel border border-accent-alt/50 bg-surface-overlay/95 px-4 py-3 shadow-lg backdrop-blur"
    >
      <p data-part="count" role="status" className="mr-auto text-sm font-medium">
        {count === 0
          ? t`Choose games: click them, Shift+click for a range.`
          : plural(count, { one: "# game selected", other: "# games selected" })}
        {error && <span className="ml-2 text-danger">{error}</span>}
      </p>
      <Button
        data-part="select-loaded"
        data-spatial="focusable"
        size="sm"
        variant="ghost"
        onPress={() => selection.add(loadedIds())}
      >
        <Trans>Select loaded</Trans>
      </Button>
      {allIds && (
        <Button
          data-part="select-all"
          data-spatial="focusable"
          size="sm"
          variant="ghost"
          icon={CheckCheck}
          isDisabled={selecting}
          onPress={selectAll}
        >
          {selecting
            ? t`Selecting…`
            : plural(total, { one: "Select all # game", other: "Select all # games" })}
        </Button>
      )}
      <Button
        data-part="clear"
        data-spatial="focusable"
        size="sm"
        variant="ghost"
        isDisabled={count === 0}
        onPress={() => selection.clear()}
      >
        <Trans>Clear</Trans>
      </Button>
      {editable && (
        <Button
          data-part="edit"
          data-spatial="focusable"
          size="sm"
          variant="primary"
          icon={Pencil}
          isDisabled={count === 0}
          onPress={() => setEditing(selection.ids())}
        >
          <Trans>Edit selected…</Trans>
        </Button>
      )}
      {editable && (
        <Button
          data-part="refresh"
          data-spatial="focusable"
          size="sm"
          variant="ghost"
          icon={RefreshCw}
          isDisabled={count === 0}
          onPress={() => setRefreshing(selection.ids())}
        >
          <Trans>Refresh details…</Trans>
        </Button>
      )}
      <Button
        data-part="done"
        data-spatial="focusable"
        size="sm"
        icon={X}
        onPress={() => selection.stop()}
      >
        <Trans>Done</Trans>
      </Button>
      {editing && <BulkEditDialog gameIds={editing} close={() => setEditing(null)} />}
      {refreshing && <RefreshDialog gameIds={refreshing} close={() => setRefreshing(null)} />}
    </div>
  );
}
