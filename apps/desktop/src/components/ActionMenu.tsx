import { ContextMenu, contextMenuTarget, type MenuAnchor, MenuItem } from "@caskade/ui";
import type { LucideIcon } from "lucide-react";
import { useState } from "react";

/** One item of `useActionMenus`: the same command as a button nearby. */
export interface MenuAction {
  id: string;
  label: string;
  icon?: LucideIcon;
  description?: string;
  tone?: "danger";
  isDisabled?: boolean;
  onAction: () => void;
}

type Actions = readonly (MenuAction | false | null | undefined)[];
type TargetProps = ReturnType<typeof contextMenuTarget>;

/** Right-click menus for a list's rows (a job, a settings row, the title
 * bar), offering each row's own buttons. Spread `target(label, actions)` on
 * a row and render `menu` once: the row that was right-clicked shows its
 * actions as they were then. Falsy actions are left out, and a row with
 * none opens nothing. */
export function useActionMenus() {
  const [open, setOpen] = useState<{
    label: string;
    actions: MenuAction[];
    anchor: MenuAnchor;
  } | null>(null);
  const target = (label: string, actions: Actions): Partial<TargetProps> => {
    const shown = actions.filter((action): action is MenuAction => Boolean(action));
    return shown.length
      ? contextMenuTarget((anchor) => setOpen({ label, actions: shown, anchor }))
      : {};
  };
  const menu = open && (
    <ContextMenu aria-label={open.label} anchor={open.anchor} onClose={() => setOpen(null)}>
      {open.actions.map(({ id, label, onAction, ...item }) => (
        <MenuItem key={id} id={id} {...item} onAction={onAction}>
          {label}
        </MenuItem>
      ))}
    </ContextMenu>
  );
  return { target, menu };
}
