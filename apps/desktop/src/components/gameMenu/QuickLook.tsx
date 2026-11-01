import { Button, GameArt, StatusBadge } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Info, Play, Square, X } from "lucide-react";
import { type ReactNode, type Ref, useEffect, useRef } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { useGame, useSessions, useStopGame } from "../../api/hooks";
import { useGameAgeRatings } from "../../api/igdb";
import {
  formatDate,
  formatPlaytime,
  formatReleaseDate,
  formatSize,
  platformLine,
} from "../../api/presentation";
import { useUnavailableStorageEntries } from "../../api/StorageProvider";
import { useInputAction } from "../../input";
import { AgeLock } from "../../views/game/AgeGate";
import { useOpenGame } from "../../views/game/origin";
import { AgeRatingLabel } from "../AgeRatingLabel";
import { playableEntry } from "./GameMenu";
import type { GameMenuTarget } from "./GameMenuHost";

/** A game at a glance, over the view it was opened from: its art,
 * platforms, playtime, last play, size, release, genres and description,
 * with Play (X again, so a double X plays) and View more beneath it, which
 * opens its page. In desktop mode it is a column on the right that a click
 * on a game opens (#196): the view stays usable beside it, and choosing
 * another game shows that one in it. In console mode it is a modal from a
 * pad's X on a card. B, Escape and Close close it. */
export function QuickLook({
  target,
  panel = false,
  panelRef,
  close,
  play,
}: {
  target: GameMenuTarget;
  /** Desktop mode's side panel, not a modal. */
  panel?: boolean;
  /** The side panel's element. */
  panelRef?: Ref<HTMLDivElement>;
  close: () => void;
  /** Starts the game, as the menu's Play does. */
  play: (entryId?: string) => void;
}) {
  const { t } = useLingui();
  const openGame = useOpenGame();
  const dialog = useRef<HTMLElement>(null);
  const game = useGame(target.gameId).data;
  const sessions = useSessions(target.gameId).data;
  const stop = useStopGame();
  const ratings = useGameAgeRatings(target.gameId).data?.ratings ?? [];
  const unavailable = useUnavailableStorageEntries();
  const running = sessions?.find((session) => session.endedAt === null);
  const playable = game ? playableEntry(game.entries, unavailable, game.defaultEntryId) : undefined;
  // Enabled while the details load; the launch picks the entry then.
  const canPlay = !game || Boolean(playable);
  const start = () => {
    if (running) {
      stop.mutate(running.id);
      return;
    }
    if (!canPlay) return;
    close();
    play(playable?.id);
  };
  const stopLabel = t({ message: "Stop", context: "game action" });
  const playLabel = t({ message: "Play", context: "game action" });
  useInputAction("details", start, { label: running ? stopLabel : playLabel, owner: dialog });
  // Its page goes back to the view under the quick look (#195).
  const open = () => {
    close();
    openGame(target.gameId);
  };
  const platform = game ? platformLine(game.platform) : "";
  const facts = game
    ? [
        { part: "playtime", label: t`Playtime`, value: formatPlaytime(game.playtimeSeconds) },
        { part: "last-played", label: t`Last played`, value: formatDate(game.lastPlayedAt) },
        { part: "size", label: t`Size`, value: formatSize(game.sizeBytes) },
        ...(game.releaseDate
          ? [
              {
                part: "released",
                label: t`Released`,
                value: formatReleaseDate(game.releaseDate),
              },
            ]
          : []),
      ]
    : [];
  const content = (
    <Dialog
      ref={dialog}
      data-cs="quick-look"
      aria-label={target.title}
      className="relative flex min-h-0 w-full flex-col outline-none"
    >
      {/* Close stays put while the rest scrolls. */}
      <Button
        size="sm"
        icon={X}
        aria-label={t({ message: "Close", context: "dialog" })}
        data-part="close"
        onPress={close}
        className="absolute top-3 right-3 z-10"
      />
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <GameArt
          part="art"
          game={{ title: target.title }}
          image={game?.hero ?? game?.coverGrid ?? null}
          className="aspect-video w-full shrink-0"
        />
        <div className="flex flex-col gap-4 p-6">
          <div className="flex flex-wrap items-center gap-3">
            {game && <StatusBadge status={game.status} />}
            {platform && (
              <span data-part="platform" className="text-sm text-fg-muted">
                {platform}
              </span>
            )}
            {ratings.length > 0 && <AgeRatingLabel ratings={ratings} />}
            <AgeLock gameId={target.gameId} />
          </div>
          <Heading
            slot="title"
            data-part="title"
            className="font-display text-3xl font-semibold tracking-wide"
          >
            {target.title}
          </Heading>
          <div data-part="actions" className="flex flex-col gap-2">
            <Button
              variant="primary"
              icon={running ? Square : Play}
              data-part="play"
              data-focus-default
              isDisabled={!canPlay || stop.isPending}
              onPress={start}
            >
              {running ? (stop.isPending ? t`Stopping…` : stopLabel) : playLabel}
            </Button>
            <Button icon={Info} data-part="open" onPress={open}>
              <Trans>View more</Trans>
            </Button>
          </div>
          {stop.isError && (
            <p role="alert" className="text-sm text-danger">
              {stop.error.message}
            </p>
          )}
          {game && !canPlay && (
            <p data-part="notice" className="text-sm text-fg-muted">
              {game.entries.length ? t`Not installed or unavailable.` : t`Nothing to launch.`}
            </p>
          )}
          {facts.length > 0 && (
            <dl data-part="facts" className="grid grid-cols-2 gap-x-6 gap-y-3">
              {facts.map((fact) => (
                <div key={fact.part} data-part={fact.part}>
                  <dt className="font-mono text-xs tracking-widest text-fg-subtle uppercase">
                    {fact.label}
                  </dt>
                  <dd className="mt-0.5 text-base">{fact.value}</dd>
                </div>
              ))}
            </dl>
          )}
          {game && (game.genre || game.tags.length > 0) && (
            <ul data-part="genres" className="flex flex-wrap gap-2">
              {[...new Set([...(game.genre ? game.genre.split(" · ") : []), ...game.tags])].map(
                (name) => (
                  <li
                    key={name}
                    className="rounded-full border border-border px-3 py-1 text-sm text-fg-muted"
                  >
                    {name}
                  </li>
                ),
              )}
            </ul>
          )}
          {game?.description && (
            <p data-part="description" className="text-base leading-relaxed text-fg-muted">
              {game.description}
            </p>
          )}
        </div>
      </div>
    </Dialog>
  );
  if (panel)
    return (
      <SidePanel ref={panelRef} close={close}>
        {content}
      </SidePanel>
    );
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(isOpen) => {
        if (!isOpen) close();
      }}
      className="fixed inset-0 z-50 bg-surface-base/60 backdrop-blur-sm"
    >
      <Modal className="fixed inset-y-0 right-0 flex w-full max-w-xl border-l border-border-strong bg-surface-base shadow-xl data-entering:animate-enter">
        {content}
      </Modal>
    </ModalOverlay>
  );
}

/** Desktop mode's quick look: a column below the title bar that slides in
 * from the right over the view, which stays usable (a click on another
 * game shows that one here). Escape closes it, unless a menu, a list, a
 * field or another dialog takes the key. A click elsewhere in the view
 * closes it too, unless it chose a game (`GameMenuHost`). */
function SidePanel({
  ref,
  close,
  children,
}: {
  ref?: Ref<HTMLDivElement>;
  close: () => void;
  children: ReactNode;
}) {
  const onClose = useRef(close);
  onClose.current = close;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      const at = event.target instanceof Element ? event.target : null;
      if (
        at?.closest(
          '[role="menu"], [role="listbox"], [role="combobox"], [data-placement], [role="alertdialog"], [role="dialog"]:not([data-cs="quick-look"]), input, textarea',
        )
      )
        return;
      event.preventDefault();
      onClose.current();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div
      ref={ref}
      className="fixed top-topbar right-0 bottom-0 z-40 flex w-[min(28rem,100%)] border-t border-l border-border-strong bg-surface-base shadow-xl transition-[translate,opacity] duration-(--cs-motion-duration-base) ease-(--cs-motion-ease-out) starting:translate-x-full starting:opacity-0"
    >
      {children}
    </div>
  );
}
