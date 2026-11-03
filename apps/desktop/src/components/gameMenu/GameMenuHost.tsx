import { Button, contextMenuTarget, type MenuAnchor } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useRouterState } from "@tanstack/react-router";
import {
  createContext,
  type MouseEvent,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { useGame } from "../../api/hooks";
import { useUnavailableStorageEntries } from "../../api/StorageProvider";
import type { ShareItem } from "../../api/shares";
import { useMode } from "../../shell/console/mode";
import { useGameLaunch } from "../../views/game/launch";
import { useOpenGame } from "../../views/game/origin";
import { RemoveWebGameDialog } from "../../views/library/web/RemoveWebGame";
import { SaveOfflineDialog } from "../../views/library/web/SaveOffline";
import { CollectionPicker } from "../collections/CollectionPicker";
import { ToolRequest } from "../gameActions/ToolRequest";
import { GameEditor } from "../gameEditor/GameEditor";
import { ShareGameDialog } from "../shares/ShareGameDialog";
import { MoveGameDialog } from "../storage/MoveGameAction";
import { GameMenu, playableEntry } from "./GameMenu";
import { QuickLook } from "./QuickLook";
import { RemoveGameDialog } from "./RemoveGameDialog";
import { UninstallDialog } from "./UninstallDialog";

/** What a game's menu is for: the game, or one of its entries (a row on the
 * game page). */
export interface GameMenuTarget {
  gameId: string;
  title: string;
  entryId?: string;
}

/** What the menu opened, which outlives the menu itself. */
export type GameMenuDialog =
  | { type: "move"; entryId?: string }
  | { type: "uninstall"; entryId: string; location: string | null }
  /** Without an entry, the first playable one once the details load;
   * without an action, its default (#178). */
  | { type: "play"; entryId?: string; action?: string }
  /** One of a release's tools (#178). */
  | { type: "tool"; entryId: string; actionId: string; name: string }
  | { type: "collections" }
  /** The game's editor (#172). */
  | { type: "edit" }
  /** A web game's Play offline…: where to keep it. */
  | { type: "offline" }
  /** A web game's Remove…; `keepsGame` when the game has other entries. */
  | { type: "remove-web"; keepsGame: boolean }
  /** Remove from library… for a game Caskade added; `uninstalls` when it
   * has installed files to delete first. */
  | { type: "remove"; uninstalls: boolean }
  /** Share with someone… (#247): a link to the release `entryId`, or to
   * `item` (a web game's offline copy, a ROM, #248). */
  | { type: "share"; entryId: string; item?: ShareItem };

interface Host {
  open(target: GameMenuTarget, anchor: MenuAnchor): void;
  /** Opens the game's quick look (a pad's X; a click in desktop mode).
   * Focus goes back to `opener` (or the game's card) when it closes. */
  look(target: GameMenuTarget, opener?: HTMLElement | null): void;
  /** Plays the game as its menu's Play does. */
  play(target: GameMenuTarget): void;
  /** Opens the game's editor (#172). */
  edit(target: GameMenuTarget): void;
  /** Opens one of the menu's dialogs without the menu: a tool from the
   * game page (#178). */
  dialog(target: GameMenuTarget, dialog: GameMenuDialog): void;
  current: GameMenuTarget | null;
}

const HostContext = createContext<Host | null>(null);

type TargetProps = ReturnType<typeof contextMenuTarget>;

/** Opens a game's menu from anywhere the game shows: spread `targetProps`
 * on what is right-clicked, and call `open` for a pad's Start or an actions
 * button. Without a `GameMenuHost` (a view rendered alone) nothing opens. */
export function useGameMenu(target: GameMenuTarget) {
  const host = useContext(HostContext);
  const open = (anchor: MenuAnchor) => host?.open(target, anchor);
  /** A click on the game's link: see `useSelectGame`. */
  const onClick = useGameClick(target);
  return {
    open,
    look: () => host?.look(target),
    onClick,
    targetProps: (host ? contextMenuTarget(open) : {}) as Partial<TargetProps>,
    isOpen: host?.current?.gameId === target.gameId && host.current.entryId === target.entryId,
  };
}

/** Opens any game's editor, as its menu's Edit… does, or `null` without
 * a `GameMenuHost`. */
export function useEditGame() {
  const host = useContext(HostContext);
  return host ? (target: GameMenuTarget) => host.edit(target) : null;
}

/** The game menu host, to open its dialogs from elsewhere (#178), or
 * `null` without a `GameMenuHost`. */
export function useGameMenuHost() {
  const host = useContext(HostContext);
  return host ? { dialog: host.dialog } : null;
}

/** Plays any game as its menu's Play does (Play Random), or `null`
 * without a `GameMenuHost`. */
export function usePlayGame() {
  const host = useContext(HostContext);
  return host ? (target: GameMenuTarget) => host.play(target) : null;
}

/** What choosing a game does (a click on a card, a list row or a search
 * suggestion; a pad's A in console mode): in desktop mode its summary
 * panel over the view (the quick look, #196), whose View more opens its
 * page; in console mode, or without a `GameMenuHost`, its page. Either way
 * the page knows where it was opened from (#195). */
export function useSelectGame() {
  const host = useContext(HostContext);
  const openGame = useOpenGame();
  const desktop = useMode().mode === "desktop";
  return useCallback(
    (target: GameMenuTarget, opener?: HTMLElement | null) => {
      if (desktop && host) host.look(target, opener);
      else openGame(target.gameId);
    },
    [desktop, host, openGame],
  );
}

/** A game link's `onClick`: a plain click (or Enter, or a pad's A) chooses
 * the game (`useSelectGame`); a modified or middle click is left alone. */
export function useGameClick(target: GameMenuTarget) {
  const select = useSelectGame();
  return (event: MouseEvent<HTMLElement>) => {
    if (event.defaultPrevented || event.button !== 0) return;
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    select(target, event.currentTarget);
  };
}

/** Puts focus back where a quick look was opened from: the element, or the
 * game's card when a virtualized grid has redrawn it. */
function refocus(target: GameMenuTarget, opener: HTMLElement | null) {
  const card = document.querySelector<HTMLElement>(
    `[data-focus-key="game-card:${CSS.escape(target.gameId)}"]`,
  );
  const into = opener?.isConnected
    ? opener
    : (card?.querySelector<HTMLElement>("a[href], button") ?? card);
  into?.focus();
}

/** `useGameMenu` for any number of targets (a page's entry rows): returns
 * the props that open each one's menu. */
export function useGameMenus() {
  const host = useContext(HostContext);
  return (target: GameMenuTarget): Partial<TargetProps> =>
    host ? contextMenuTarget((anchor) => host.open(target, anchor)) : {};
}

/** Holds the one game menu that is open, and what it starts: the move,
 * uninstall and remove dialogs and a launch. They live here, not in the card that was
 * right-clicked, so a virtualized grid can drop the card. */
export function GameMenuHost({ children }: { children: ReactNode }) {
  const [menu, setMenu] = useState<{ target: GameMenuTarget; anchor: MenuAnchor } | null>(null);
  const [looking, setLooking] = useState<{
    target: GameMenuTarget;
    opener: HTMLElement | null;
  } | null>(null);
  // Leaving the view closes it; a search or filter change in place doesn't.
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  // biome-ignore lint/correctness/useExhaustiveDependencies: closes on each path change
  useEffect(() => setLooking(null), [pathname]);
  const panel = useMode().mode === "desktop";
  // Counts the games chosen, so a click that chose another one leaves the
  // panel open.
  const chosen = useRef(0);
  const panelRef = useRef<HTMLDivElement>(null);
  const showing = panel && looking !== null;
  // A click elsewhere in the view closes the panel without moving focus.
  // Menus, popovers and dialogs render outside the view and don't count.
  useEffect(() => {
    if (!showing) return;
    const outside = (at: EventTarget | null) => {
      const own = panelRef.current;
      const view = own?.closest("body > *");
      return (
        at instanceof Element &&
        !!view?.contains(at) &&
        !own?.contains(at) &&
        !at.closest('[data-cs="window-controls"]')
      );
    };
    let pressed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const down = (event: PointerEvent) => {
      pressed = event.button === 0 && outside(event.target);
    };
    // Capture, since React Aria stops a press's click from bubbling; the
    // check waits until the click has chosen a game or not.
    const click = (event: Event) => {
      if (!pressed || !outside(event.target)) return;
      pressed = false;
      const before = chosen.current;
      timer = setTimeout(() => {
        if (chosen.current === before) setLooking(null);
      });
    };
    document.addEventListener("pointerdown", down, true);
    document.addEventListener("click", click, true);
    return () => {
      document.removeEventListener("pointerdown", down, true);
      document.removeEventListener("click", click, true);
      clearTimeout(timer);
    };
  }, [showing]);
  const [dialog, setDialog] = useState<{
    target: GameMenuTarget;
    dialog: GameMenuDialog;
    count: number;
  } | null>(null);
  const host = useMemo<Host>(
    () => ({
      open: (target, anchor) => setMenu({ target, anchor }),
      look: (target, opener) => {
        chosen.current += 1;
        setLooking({
          target,
          opener:
            opener ??
            (document.activeElement instanceof HTMLElement &&
            document.activeElement !== document.body
              ? document.activeElement
              : null),
        });
      },
      play: (target) =>
        setDialog((current) => ({
          target,
          dialog: { type: "play" },
          count: (current?.count ?? 0) + 1,
        })),
      edit: (target) =>
        setDialog((current) => ({
          target,
          dialog: { type: "edit" },
          count: (current?.count ?? 0) + 1,
        })),
      dialog: (target, next) =>
        setDialog((current) => ({ target, dialog: next, count: (current?.count ?? 0) + 1 })),
      current: menu?.target ?? null,
    }),
    [menu],
  );
  const close = () => setDialog(null);
  return (
    <HostContext value={host}>
      {children}
      {looking && (
        <QuickLook
          target={looking.target}
          panel={panel}
          panelRef={panelRef}
          close={() => {
            setLooking(null);
            // A modal puts focus back itself.
            if (panel) refocus(looking.target, looking.opener);
          }}
          play={(entryId) =>
            setDialog({
              target: looking.target,
              dialog: { type: "play", entryId },
              count: (dialog?.count ?? 0) + 1,
            })
          }
        />
      )}
      {menu && (
        <GameMenu
          target={menu.target}
          anchor={menu.anchor}
          onClose={() => setMenu(null)}
          onDialog={(next) =>
            setDialog({ target: menu.target, dialog: next, count: (dialog?.count ?? 0) + 1 })
          }
        />
      )}
      {dialog?.dialog.type === "move" && (
        <MoveGameDialog
          gameId={dialog.target.gameId}
          entryId={dialog.dialog.entryId}
          isOpen
          onOpenChange={(open) => {
            if (!open) close();
          }}
        />
      )}
      {dialog?.dialog.type === "share" && (
        <ShareGameDialog
          target={
            dialog.dialog.item
              ? { kind: "item", item: dialog.dialog.item, title: dialog.target.title }
              : { kind: "game", entryId: dialog.dialog.entryId, title: dialog.target.title }
          }
          close={close}
        />
      )}
      {dialog?.dialog.type === "uninstall" && (
        <UninstallDialog
          title={dialog.target.title}
          entryId={dialog.dialog.entryId}
          location={dialog.dialog.location}
          close={close}
        />
      )}
      {dialog?.dialog.type === "offline" && (
        <SaveOfflineDialog
          gameId={dialog.target.gameId}
          title={dialog.target.title}
          close={close}
        />
      )}
      {dialog?.dialog.type === "remove-web" && (
        <RemoveWebGameDialog
          gameId={dialog.target.gameId}
          title={dialog.target.title}
          keepsGame={dialog.dialog.keepsGame}
          close={close}
        />
      )}
      {dialog?.dialog.type === "remove" && (
        <RemoveGameDialog
          gameId={dialog.target.gameId}
          title={dialog.target.title}
          uninstalls={dialog.dialog.uninstalls}
          close={close}
        />
      )}
      {dialog?.dialog.type === "edit" && (
        <GameEditor gameId={dialog.target.gameId} title={dialog.target.title} close={close} />
      )}
      {dialog?.dialog.type === "collections" && (
        <CollectionPicker gameId={dialog.target.gameId} title={dialog.target.title} close={close} />
      )}
      {dialog?.dialog.type === "play" && (
        <PlayRequest
          key={dialog.count}
          gameId={dialog.target.gameId}
          entryId={dialog.dialog.entryId}
          action={dialog.dialog.action}
          title={dialog.target.title}
          close={close}
        />
      )}
      {dialog?.dialog.type === "tool" && (
        <ToolRequest
          key={dialog.count}
          entryId={dialog.dialog.entryId}
          actionId={dialog.dialog.actionId}
          name={dialog.dialog.name}
          close={close}
        />
      )}
    </HostContext>
  );
}

/** Plays an entry once, as the game page's Play would, and shows what the
 * launch asks or why it failed. */
function PlayRequest({
  gameId,
  entryId,
  action,
  title,
  close,
}: {
  gameId: string;
  entryId?: string;
  action?: string;
  title: string;
  close: () => void;
}) {
  const { t } = useLingui();
  const flow = useGameLaunch(gameId);
  const game = useGame(gameId).data;
  const unavailable = useUnavailableStorageEntries();
  const entry =
    entryId ??
    (game ? playableEntry(game.entries, unavailable, game.defaultEntryId)?.id : undefined);
  const [nothing, setNothing] = useState(false);
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    if (entry) flow.play(entry, action);
    else if (game) setNothing(true);
    else return;
    started.current = true;
  });
  const failure = nothing
    ? t`It has nothing installed that can launch.`
    : flow.launch.isError && !flow.asking
      ? flow.launch.error.message
      : null;
  return (
    <>
      {flow.dialogs}
      {failure && (
        <ModalOverlay
          isOpen
          isDismissable
          onOpenChange={(open) => {
            if (!open) close();
          }}
          className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
        >
          <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
            <Dialog role="alertdialog" className="space-y-4 outline-none">
              <Heading slot="title" className="font-display text-xl font-semibold">
                <Trans>{title} didn't start</Trans>
              </Heading>
              <p className="text-sm text-fg-muted">{failure}</p>
              <div className="flex justify-end">
                <Button data-spatial="focusable" onPress={close}>
                  <Trans>OK</Trans>
                </Button>
              </div>
            </Dialog>
          </Modal>
        </ModalOverlay>
      )}
    </>
  );
}
