import { useLingui } from "@lingui/react/macro";
import { Copy, Minus, Square, X } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Button } from "react-aria-components";
import type { AppWindow } from "./appWindow";

const control =
  "grid size-8 place-items-center rounded-control text-fg-muted transition-colors duration-(--cs-motion-duration-fast) data-hovered:text-fg data-focus-visible:focus-ring";
const neutral = `${control} data-hovered:bg-surface-hover data-pressed:bg-surface-hover`;
/** The look of a control in this group, for buttons placed in it. */
export const controlClass = neutral;

/** Whether the window is maximized, kept up to date as it resizes. */
export function useMaximized(appWindow: AppWindow | null) {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!appWindow) return;
    let live = true;
    const sync = () => {
      appWindow
        .isMaximized()
        .then((value) => {
          if (live) setMaximized(value);
        })
        // The icon stays as it was; the button still toggles.
        .catch(() => {});
    };
    sync();
    const unlisten = appWindow.onResized(sync);
    return () => {
      live = false;
      void unlisten.then((stop) => stop());
    };
  }, [appWindow]);
  return maximized;
}

/** Minimize, maximize/restore and close, at the end of the topbar. Like a
 * native title bar's, they are not tab stops, so neither Tab nor a pad's
 * spatial navigation lands on Close by accident. `children` (the console
 * mode button) sit between minimize and maximize. */
export function WindowControls({
  window: appWindow,
  children,
}: {
  window: AppWindow;
  children?: ReactNode;
}) {
  const { t } = useLingui();
  const maximized = useMaximized(appWindow);
  return (
    <div
      data-cs="window-controls"
      data-status={maximized ? "maximized" : "restored"}
      className="flex items-center gap-1 border-l border-border pl-3"
    >
      <Button
        data-part="minimize"
        aria-label={t({ message: "Minimize", context: "window" })}
        excludeFromTabOrder
        onPress={() => void appWindow.minimize()}
        className={neutral}
      >
        <Minus className="size-4" strokeWidth={1.75} />
      </Button>
      {children}
      <Button
        data-part="maximize"
        aria-label={
          maximized
            ? t({ message: "Restore", context: "window" })
            : t({ message: "Maximize", context: "window" })
        }
        excludeFromTabOrder
        onPress={() => void appWindow.toggleMaximize()}
        className={neutral}
      >
        {maximized ? (
          <Copy className="size-3.5 -scale-x-100" strokeWidth={1.75} />
        ) : (
          <Square className="size-3.5" strokeWidth={1.75} />
        )}
      </Button>
      <Button
        data-part="close"
        aria-label={t({ message: "Close", context: "window" })}
        excludeFromTabOrder
        onPress={() => void appWindow.close()}
        className={`${control} data-hovered:bg-danger data-pressed:bg-danger`}
      >
        <X className="size-4" strokeWidth={1.75} />
      </Button>
    </div>
  );
}
