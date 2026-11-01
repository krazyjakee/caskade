import { useLingui } from "@lingui/react/macro";
import { Gamepad2 } from "lucide-react";
import { Button } from "react-aria-components";
import { useMode } from "./mode";

/** Enters console mode from the desktop topbar (#148), next to the window's
 * maximize button. Console mode leaves from its sidebar or the View button. */
export function ModeToggle({ className }: { className: string }) {
  const { t } = useLingui();
  const { setMode } = useMode();
  return (
    <Button
      data-part="console"
      aria-label={t`Console mode`}
      onPress={() => setMode("console")}
      className={className}
    >
      <Gamepad2 className="size-4" strokeWidth={1.75} />
    </Button>
  );
}
