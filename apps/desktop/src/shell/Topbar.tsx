import { useLingui } from "@lingui/react/macro";
import { Copy, Minus, Square, X } from "lucide-react";
import { useActionMenus } from "../components/ActionMenu";
import { PluginSlot } from "../plugins/PluginSlot";
import { ActivityLink } from "./ActivityLink";
import { AccountMenu } from "./accounts/AccountMenu";
import { useAppWindow } from "./appWindow";
import { ModeToggle } from "./console/ModeToggle";
import { SearchBox } from "./SearchBox";
import { useWindowDrag } from "./useWindowDrag";
import { controlClass, useMaximized, WindowControls } from "./WindowControls";

/** What keeps its own right click inside the title bar: the search box's
 * text menu, the buttons and links. */
const CONTROLS = 'input, textarea, button, a, [role="button"], [data-cs="search"]';

export function Topbar() {
  const { t } = useLingui();
  const appWindow = useAppWindow();
  useWindowDrag(appWindow);
  const maximized = useMaximized(appWindow);
  // A right click on the bar itself, not on a control, offers the window's
  // own menu, as a native title bar does.
  const menus = useActionMenus();
  const windowMenu = appWindow
    ? menus.target(t({ message: "Window", comment: "The title bar's right-click menu" }), [
        {
          id: "minimize",
          label: t({ message: "Minimize", context: "window" }),
          icon: Minus,
          onAction: () => void appWindow.minimize(),
        },
        {
          id: "maximize",
          label: maximized
            ? t({ message: "Restore", context: "window" })
            : t({ message: "Maximize", context: "window" }),
          icon: maximized ? Copy : Square,
          onAction: () => void appWindow.toggleMaximize(),
        },
        {
          id: "close",
          label: t({ message: "Close", context: "window" }),
          icon: X,
          onAction: () => void appWindow.close(),
        },
      ])
    : {};
  return (
    // The title bar: dragging anywhere but a control moves the window, a
    // double click maximizes it.
    // biome-ignore lint/a11y/noStaticElementInteractions: the window menu repeats the window controls, which keyboards and pads reach.
    <header
      onContextMenu={(event) => {
        if (event.target instanceof Element && event.target.closest(CONTROLS)) return;
        windowMenu.onContextMenu?.(event);
      }}
      data-cs="topbar"
      data-window-drag
      className="relative z-10 flex h-topbar shrink-0 items-center gap-4 pr-4 pl-6"
    >
      <SearchBox />

      <div data-part="actions" className="ml-auto flex items-center gap-4">
        <PluginSlot slot="topbar.items">
          {({ contribution: { component: Item } }) => <Item />}
        </PluginSlot>
        <ActivityLink />
        <AccountMenu size="sm" />
        {appWindow?.frame !== "custom" && <ModeToggle className={controlClass} />}
      </div>
      {appWindow?.frame === "custom" && (
        <WindowControls window={appWindow}>
          <ModeToggle className={controlClass} />
        </WindowControls>
      )}
      {menus.menu}
    </header>
  );
}
