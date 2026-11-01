/** Console mode's shell (#148): the same routes and views as the desktop,
 * with the console sidebar and top bar and the button-hint bar. The window
 * is fullscreen; the pointer hides when it rests. */
import { useLingui } from "@lingui/react/macro";
import { Outlet } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { useInputAction } from "../../input";
import { ConsoleSidebar } from "./ConsoleSidebar";
import { ConsoleTopbar } from "./ConsoleTopbar";
import { HintBar } from "./HintBar";

/** How long the pointer stays after it last moved. */
const CURSOR_IDLE_MS = 2500;

/** Whether the pointer has rested for a while. */
function useIdlePointer(): boolean {
  const [idle, setIdle] = useState(true);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const moved = () => {
      setIdle(false);
      clearTimeout(timer);
      timer = setTimeout(() => setIdle(true), CURSOR_IDLE_MS);
    };
    window.addEventListener("pointermove", moved);
    window.addEventListener("pointerdown", moved);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("pointermove", moved);
      window.removeEventListener("pointerdown", moved);
    };
  }, []);
  return idle;
}

/** Hints for what confirm and back always do, menus and dialogs included;
 * they leave the work to the defaults (press the focused control, go back
 * or close). */
function ShellBindings() {
  const { t } = useLingui();
  useInputAction("confirm", () => false, {
    label: t({
      message: "Select",
      context: "pad action",
      comment: "Hint bar: confirm the focused item",
    }),
    global: true,
  });
  useInputAction("back", () => false, {
    label: t({ message: "Back", context: "pad action" }),
    global: true,
  });
  // LT/RT page whatever list or view focus is in; lists bind their own.
  useInputAction("page.prev", () => false, { label: t`Previous page` });
  useInputAction("page.next", () => false, { label: t`Next page` });
  return null;
}

export function ConsoleShell({ pathname }: { pathname: string }) {
  const idle = useIdlePointer();
  return (
    <div
      data-cs="console-shell"
      data-cursor={idle ? "hidden" : "visible"}
      className="relative flex h-full flex-col overflow-clip data-[cursor=hidden]:cursor-none data-[cursor=hidden]:**:cursor-none"
    >
      <ShellBindings />
      <div data-cs="backdrop" aria-hidden />
      <div className="flex min-h-0 flex-1">
        <ConsoleSidebar />
        <div data-part="main" className="relative flex min-w-0 flex-1 flex-col">
          <ConsoleTopbar />
          <main
            data-cs="view"
            data-focus-section="main"
            className="min-h-0 flex-1 overflow-y-auto px-8 pb-8"
          >
            <div key={pathname} className="flex min-h-full animate-enter flex-col">
              <Outlet />
            </div>
          </main>
        </div>
      </div>
      <HintBar />
    </div>
  );
}
