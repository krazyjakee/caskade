import { msg } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import { CircleCheck, CircleSlash, Gamepad2, ShoppingCart } from "lucide-react";

/** A game's install state, as `data-status` on cards and badges. */
export type GameStatus = "installed" | "not-installed" | "available" | "emulated";

const config = {
  installed: { label: msg`Installed`, icon: CircleCheck, className: "bg-status-installed/85" },
  "not-installed": {
    label: msg`Not Installed`,
    icon: CircleSlash,
    className: "bg-status-missing/85",
  },
  available: {
    label: msg({ message: "Available to Buy", comment: "A game that can be bought in a store" }),
    icon: ShoppingCart,
    className: "bg-status-available/90",
  },
  emulated: {
    label: msg({ message: "Emulator", comment: "A game played through an emulator" }),
    icon: Gamepad2,
    className: "bg-status-emulated/90",
  },
} satisfies Record<GameStatus, unknown>;

export function StatusBadge({
  status,
  part,
}: {
  status: GameStatus;
  /** The `data-part` it plays in a parent component. */
  part?: string;
}) {
  const { t } = useLingui();
  const { label, icon: Icon, className } = config[status];
  return (
    <span
      data-cs="status-badge"
      data-part={part}
      data-status={status}
      className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium text-white backdrop-blur ${className}`}
    >
      <Icon data-part="icon" className="size-3" />
      <span data-part="label">{t(label)}</span>
    </span>
  );
}
