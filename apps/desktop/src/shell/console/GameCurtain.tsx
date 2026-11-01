/** Console mode's curtain over a running game: when a game starts, Caskade
 * fades to black instead of minimizing, and the window manager gives the
 * game's window focus over it. It lifts when Caskade has focus again after
 * the game had it (the game's exit, guide, switching windows), when the last
 * game exits, or on back. While it is down, it takes the pad, so nothing
 * behind it is pressed; guide still opens the quick menu over it. */
import { Trans, useLingui } from "@lingui/react/macro";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Dialog, Modal, ModalOverlay } from "react-aria-components";
import { useGame } from "../../api/hooks";
import { useLibraryApi } from "../../api/provider";
import { effectSubscription } from "../../api/subscription";
import { useAppWindow } from "../appWindow";
import { glyph } from "./glyphs";
import { useHintFamily } from "./HintBar";

interface Curtain {
  /** Brings it down for the latest running game. */
  cover(): void;
}

const CurtainContext = createContext<Curtain>({ cover: () => {} });

export function useGameCurtain(): Curtain {
  return useContext(CurtainContext);
}

/** Follows game sessions. `active` (console mode) decides whether a game
 * that starts brings the curtain down. */
export function GameCurtainProvider({
  active,
  children,
}: {
  active: boolean;
  children: ReactNode;
}) {
  const api = useLibraryApi();
  const appWindow = useAppWindow();
  // Running sessions and their games, the latest last.
  const running = useRef(new Map<string, string>());
  const activeRef = useRef(active);
  activeRef.current = active;
  // The game the curtain is down for.
  const [gameId, setGameId] = useState<string | null>(null);
  // Caskade lost focus while the curtain was down.
  const left = useRef(false);
  const cover = useCallback((id: string | null) => {
    left.current = false;
    setGameId(id);
  }, []);
  useEffect(() => {
    if (!active) setGameId(null);
  }, [active]);
  useEffect(
    () =>
      effectSubscription(api.subscribe, (event) => {
        if (event.type === "game.started") {
          running.current.set(event.sessionId, event.gameId);
          if (activeRef.current) cover(event.gameId);
        } else if (event.type === "game.exited") {
          running.current.delete(event.sessionId);
          if (running.current.size === 0) setGameId(null);
        }
      }),
    [api, cover],
  );
  useEffect(() => {
    if (!gameId || !appWindow?.onFocusChanged) return;
    return effectSubscription(appWindow.onFocusChanged, (focused) => {
      if (!focused) left.current = true;
      else if (left.current) setGameId(null);
    });
  }, [appWindow, gameId]);
  const curtain = useMemo<Curtain>(
    () => ({ cover: () => cover(Array.from(running.current.values()).at(-1) ?? null) }),
    [cover],
  );
  return (
    <CurtainContext value={curtain}>
      {children}
      {active && <GameCurtain gameId={gameId} lift={() => setGameId(null)} />}
    </CurtainContext>
  );
}

function GameCurtain({ gameId, lift }: { gameId: string | null; lift: () => void }) {
  const { t } = useLingui();
  // The last game stays on the curtain while it fades out.
  const [shown, setShown] = useState(gameId);
  if (gameId && gameId !== shown) setShown(gameId);
  const covered = gameId !== null;
  return (
    <>
      <div
        data-cs="game-curtain"
        data-state={covered ? "covered" : "open"}
        aria-hidden
        className="pointer-events-none fixed inset-0 z-50 grid place-items-center bg-black opacity-0 transition-opacity duration-[calc(var(--cs-motion-duration-slow)*2)] data-[state=covered]:opacity-100"
      >
        {shown && <CurtainText gameId={shown} />}
      </div>
      {/* Takes the pad and the pointer while the curtain is down; gone the
          moment it lifts, so the view answers while it fades back in. */}
      <ModalOverlay
        isOpen={covered}
        isDismissable={false}
        onOpenChange={(open) => {
          if (!open) lift();
        }}
        className="fixed inset-0 z-50 cursor-none"
      >
        <Modal className="h-full">
          {/* Back (Escape) dismisses it. */}
          <Dialog
            aria-label={t({ message: "Playing", comment: "The black curtain over a running game" })}
            className="h-full outline-none"
          />
        </Modal>
      </ModalOverlay>
    </>
  );
}

function CurtainText({ gameId }: { gameId: string }) {
  const { t } = useLingui();
  const title = useGame(gameId).data?.title;
  const guide = glyph("guide", useHintFamily()) ?? t({ message: "Guide", context: "pad button" });
  return (
    <div className="flex flex-col items-center gap-4 text-center">
      <p data-part="title" className="font-display text-4xl font-semibold text-fg">
        {title ??
          t({ message: "Starting…", comment: "A game starting, before its title is known" })}
      </p>
      <p data-part="hint" className="text-sm text-fg-muted">
        <Trans comment="The guide button's name comes first">
          <kbd className="font-mono font-semibold text-fg">{guide}</kbd> opens the quick menu
        </Trans>
      </p>
    </div>
  );
}
