import { Button } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { ArrowDown, ArrowUp, Trash2 } from "lucide-react";
import { useState } from "react";

/** Move up, Move down and Delete… for the saved item an ordered list's
 * editor shows (launch actions, scripts). Delete asks on the button itself
 * first; give each item its own `key` so the question doesn't carry over. */
export function OrderedItemControls({
  item,
  order,
  dirty,
  ordering,
  removing,
  reorder,
  remove,
}: {
  item: { id: string; name: string };
  /** The movable items' ids, in order. */
  order: readonly string[];
  dirty: boolean;
  ordering: boolean;
  removing: boolean;
  reorder: (ids: string[]) => void;
  remove: () => void;
}) {
  const { t } = useLingui();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const move = (by: -1 | 1) => {
    const ids = [...order];
    const at = ids.indexOf(item.id);
    const to = at + by;
    if (at < 0 || to < 0 || to >= ids.length) return;
    ids.splice(to, 0, ...ids.splice(at, 1));
    reorder(ids);
  };
  return (
    <>
      <Button
        data-spatial="focusable"
        aria-label={t`Move up`}
        icon={ArrowUp}
        isDisabled={dirty || order[0] === item.id || ordering}
        onPress={() => move(-1)}
      />
      <Button
        data-spatial="focusable"
        aria-label={t`Move down`}
        icon={ArrowDown}
        isDisabled={dirty || order[order.length - 1] === item.id || ordering}
        onPress={() => move(1)}
      />
      <Button
        data-spatial="focusable"
        data-part="delete"
        variant="danger"
        icon={Trash2}
        isDisabled={removing}
        onPress={() => {
          if (!confirmDelete) {
            setConfirmDelete(true);
            return;
          }
          remove();
        }}
      >
        {confirmDelete ? t`Delete “${item.name}”?` : t`Delete…`}
      </Button>
    </>
  );
}
