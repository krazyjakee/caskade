/** The guide quick menu (#149): pressing a pad's guide button raises Caskade
 * (in Rust); while a game runs, this menu then offers going back to it,
 * stopping it, or its details. A stream from another device (#264) ends
 * instead, and its game stops there. */
import { Button } from "@caskade/ui";
import { t as translate } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { isTauri } from "@tauri-apps/api/core";
import { useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { commands } from "../api/bindings";
import { errorMessage, unwrap } from "../api/reply";
import { useInputAction } from "./hooks";

export interface RunningGame {
  sessionId: string;
  gameId: string;
  title: string;
  /** Streamed from another device. */
  stream?: boolean;
}

export interface GuideApi {
  /** Running games, the latest first. */
  running(): Promise<RunningGame[]>;
  /** Steps aside so the game has the screen again. */
  returnToGame(): Promise<void>;
  /** Ends the game; its session records the playtime. */
  stop(sessionId: string): Promise<void>;
}

export const nativeGuideApi: GuideApi = {
  async running() {
    if (!isTauri()) return [];
    const tickets = await unwrap(commands.runningGames());
    const games = await Promise.all(
      tickets.map(async (ticket) => {
        const game = await unwrap(commands.getGame(ticket.gameId)).catch(() => null);
        return {
          sessionId: ticket.sessionId,
          gameId: ticket.gameId,
          title: game?.game.title ?? translate`Your game`,
          stream: Boolean(
            game?.entries
              .find((entry) => entry.id === ticket.entryId)
              ?.sourceId.startsWith("stream:"),
          ),
        };
      }),
    );
    return games.reverse();
  },
  async returnToGame() {
    await unwrap(commands.guideReturnToGame());
  },
  async stop(sessionId) {
    await unwrap(commands.guideStopGame(sessionId));
  },
};

export function GuideMenu({
  api = nativeGuideApi,
  openDetails,
}: {
  api?: GuideApi;
  openDetails: (gameId: string) => void;
}) {
  const { t } = useLingui();
  const [game, setGame] = useState<RunningGame | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useInputAction(
    "guide",
    () => {
      // Pressed again, it closes the menu.
      if (game) {
        if (!busy) close();
        return;
      }
      api
        .running()
        .then((running) => {
          setConfirming(false);
          setError(null);
          setGame(running[0] ?? null);
        })
        .catch((error: unknown) => console.error("Couldn't list running games", error));
    },
    // The guide button works over menus and dialogs too.
    { label: t`Quick menu`, global: true },
  );
  const close = () => {
    setGame(null);
    setConfirming(false);
    setError(null);
  };
  const act = async (run: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await run();
      close();
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  };
  if (!game) return null;
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6">
        <Dialog data-cs="guide-menu" aria-label={t`Quick menu`} className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {game.title}
          </Heading>
          <p className="mt-1 text-sm text-fg-muted">
            {confirming
              ? game.stream
                ? t`End the stream? The game stops on the other device, and progress it hasn't saved is lost.`
                : t`Stop the game? Progress it hasn't saved is lost.`
              : game.stream
                ? t({
                    message: "is streaming from another device.",
                    comment: "Follows the streamed game's title, shown as a heading above",
                  })
                : t({
                    message: "is running.",
                    comment: "Follows the running game's title, shown as a heading above",
                  })}
          </p>
          {error && (
            <p role="alert" className="mt-3 text-sm text-danger">
              {error}
            </p>
          )}
          <div className="mt-6 flex flex-col gap-2">
            {confirming ? (
              <>
                <Button
                  key="stop"
                  variant="danger"
                  autoFocus
                  isDisabled={busy}
                  onPress={() => act(() => api.stop(game.sessionId))}
                >
                  {game.stream ? (
                    <Trans>End stream</Trans>
                  ) : (
                    <Trans>Stop {{ title: game.title }}</Trans>
                  )}
                </Button>
                <Button key="keep" isDisabled={busy} onPress={() => setConfirming(false)}>
                  <Trans>Keep playing</Trans>
                </Button>
              </>
            ) : (
              <>
                <Button
                  key="return"
                  variant="primary"
                  autoFocus
                  isDisabled={busy}
                  onPress={() => act(() => api.returnToGame())}
                >
                  <Trans>Return to game</Trans>
                </Button>
                <Button key="ask" isDisabled={busy} onPress={() => setConfirming(true)}>
                  {game.stream ? <Trans>End stream</Trans> : <Trans>Stop game</Trans>}
                </Button>
                <Button
                  variant="ghost"
                  isDisabled={busy}
                  onPress={() => {
                    openDetails(game.gameId);
                    close();
                  }}
                >
                  <Trans>Game details</Trans>
                </Button>
              </>
            )}
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
