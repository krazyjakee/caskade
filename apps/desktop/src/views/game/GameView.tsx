import { GameArt, Select, StatusBadge, Button as UiButton } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useIsMutating } from "@tanstack/react-query";
import { Clock, ExternalLink, Pencil, Play, Square } from "lucide-react";
import { type Ref, useRef } from "react";
import { Button } from "react-aria-components";
import { useGameEditApi } from "../../api/gameEdit";
import { useGameEmulation, useGameEmulations } from "../../api/gameEmulation";
import {
  useGame,
  usePageImages,
  useSessions,
  useSetDefaultEntry,
  useStopGame,
} from "../../api/hooks";
import { gameIgdbChangeKey, useGameAgeRatings } from "../../api/igdb";
import {
  formatDate,
  formatDuration,
  formatInstallState,
  formatPlaytime,
  formatSize,
  formatSource,
} from "../../api/presentation";
import { useUnavailableStorageEntries } from "../../api/StorageProvider";
import { streamReleaseName, useStreamHosts } from "../../api/streaming";
import type { GameEntry } from "../../api/types";
import { AgeRatingLabel } from "../../components/AgeRatingLabel";
import {
  AchievementSummary,
  GameAchievements,
} from "../../components/achievements/GameAchievements";
import { PlayChoice } from "../../components/gameActions/PlayChoice";
import { playableEntry } from "../../components/gameMenu/GameMenu";
import { useEditGame, useGameMenus } from "../../components/gameMenu/GameMenuHost";
import { MoveGameAction } from "../../components/storage/MoveGameAction";
import { useDefaultFocus } from "../../input";
import { PluginGameDetails } from "../../plugins/GameSlots";
import { AgeLock, useAgeRestriction } from "./AgeGate";
import { GameBackLink } from "./BackLink";
import { canStart, EmulatedEntry } from "./EmulatedEntry";
import { EntryUpdate } from "./EntryUpdate";
import { IgdbPanel } from "./IgdbPanel";
import { useGameLaunch } from "./launch";
import { OwnedReleases } from "./OwnedReleases";
import { useReplaceGame } from "./origin";
import { PlaytimeSources } from "./PlaytimeSources";
import { Screenshots } from "./Screenshots";
import { SeparateEntry } from "./SeparateEntry";
import { GameVideos } from "./Videos";

/** The game's age rating (#204), from its IGDB match, and whether the
 * signed-in account needs an admin's PIN to play it (#209). */
function GameAgeRating({ gameId }: { gameId: string }) {
  const ratings = useGameAgeRatings(gameId).data?.ratings ?? [];
  const locked = Boolean(useAgeRestriction(gameId).data);
  return (
    <>
      {ratings.length > 0 && (
        <>
          {" · "}
          <AgeRatingLabel ratings={ratings} />
        </>
      )}
      {locked && (
        <>
          {" · "}
          <AgeLock gameId={gameId} />
        </>
      )}
    </>
  );
}

/** Play; an emulated entry also needs its emulator ready (or to ask). */
function PlayButton({
  entryId,
  emulated,
  isDisabled,
  label,
  onPress,
  ref,
  primary = false,
  named = false,
}: {
  /** Nothing to play: the button stays, disabled. */
  entryId: string | undefined;
  emulated: boolean;
  isDisabled: boolean;
  label: string;
  onPress: () => void;
  ref?: Ref<HTMLButtonElement>;
  /** The game's own Play, at the top of the page: larger, and where a pad
   * starts. */
  primary?: boolean;
  /** One of several releases: an emulated one names its platform. */
  named?: boolean;
}) {
  const emulation = useGameEmulation(entryId ?? "", Boolean(entryId) && emulated);
  const platform = named ? emulation.data?.platform?.short : undefined;
  return (
    <Button
      ref={ref}
      data-focus-default={primary || undefined}
      isDisabled={!entryId || isDisabled || (emulated && !canStart(emulation.data))}
      onPress={onPress}
      className={`flex shrink-0 items-center justify-center gap-2 rounded-control bg-accent font-semibold text-accent-fg data-disabled:opacity-50 ${primary ? "px-8 py-3 text-lg" : "px-5 py-2"}`}
    >
      <Play className={primary ? "size-5" : "size-4"} />
      {label}
      {platform && <span className="font-mono text-xs">{platform}</span>}
    </Button>
  );
}

/** What each of a game's entries is called: an emulated release by its
 * platform, others by their source. Names two entries share add the
 * source. */
function useReleaseNames(entries: readonly GameEntry[]) {
  const emulated = entries.filter((entry) => entry.emulated);
  const emulations = useGameEmulations(emulated.map((entry) => entry.id));
  const hosts = useStreamHosts().data ?? [];
  const base = entries.map(
    (entry) =>
      streamReleaseName(entry.source, hosts) ??
      emulations[emulated.indexOf(entry)]?.data?.platform?.name ??
      formatSource(entry.source),
  );
  return new Map(
    entries.map((entry, index) => {
      const source = formatSource(entry.source);
      const name = base[index] ?? source;
      const shared = base.filter((other) => other === name).length > 1;
      return [entry.id, shared && name !== source ? `${name} · ${source}` : name];
    }),
  );
}

/** Ends the running game (#150): the pad's way out of a game it came back
 * from with guide. */
function StopButton({ sessionId, ref }: { sessionId: string; ref?: Ref<HTMLButtonElement> }) {
  const { t } = useLingui();
  const stop = useStopGame();
  return (
    <div className="flex flex-col items-stretch gap-1">
      <span
        data-part="playing"
        className="font-mono text-xs tracking-widest text-accent-alt uppercase"
      >
        <Trans comment="The game is running">Playing</Trans>
      </span>
      <Button
        ref={ref}
        data-focus-default
        isDisabled={stop.isPending}
        onPress={() => stop.mutate(sessionId)}
        className="flex items-center justify-center gap-2 rounded-control border border-danger px-8 py-3 text-lg font-semibold text-danger data-disabled:opacity-50 data-hovered:bg-danger/15"
      >
        <Square className="size-5" />
        {stop.isPending
          ? t`Stopping…`
          : t({ message: "Stop", comment: "Button: ends the running game" })}
      </Button>
      {stop.isError && (
        <p role="alert" className="text-xs text-danger">
          {stop.error.message}
        </p>
      )}
    </div>
  );
}

/** `https://www.armorgames.com/play/1716` → `armorgames.com`. */
function hostOf(url: string) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

export function GameView({
  gameId,
  from,
}: {
  gameId: string;
  /** The page it was opened from (the route's `from`, #195). */
  from?: string;
}) {
  const { t } = useLingui();
  const result = useGame(gameId);
  const imagesLoading = usePageImages(gameId);
  const sessions = useSessions(gameId);
  // With a pad, the page opens on the game's Play (or Stop).
  const first = useRef<HTMLButtonElement>(null);
  const running = sessions.data?.find((session) => session.endedAt === null);
  useDefaultFocus(first, Boolean(result.data && sessions.data), running?.id ?? "idle");
  const unavailable = useUnavailableStorageEntries();
  const names = useReleaseNames(result.data?.entries ?? []);
  const setDefault = useSetDefaultEntry();
  // Right clicks on the hero and on each entry open the game's menu.
  const menus = useGameMenus();
  const edit = useEditGame();
  const editable = useGameEditApi() !== null;
  // Play offers a newer release first.
  const updates = useGameLaunch(gameId);
  const { launch } = updates;
  // A new IGDB match can merge the game into another, whose page then
  // takes this one's place.
  const replaceGame = useReplaceGame();
  const matching = useIsMutating({ mutationKey: gameIgdbChangeKey(gameId) }) > 0;
  if (result.isPending || (!result.data && matching))
    return (
      <p role="status" className="py-12 text-fg-muted">
        <Trans>Loading game…</Trans>
      </p>
    );
  if (result.isError)
    return (
      <div role="alert" className="py-12">
        <h1>
          <Trans>Could not load this game</Trans>
        </h1>
        <p className="text-fg-muted">{result.error.message}</p>
        <Button
          onPress={() => {
            void result.refetch();
          }}
        >
          <Trans>Try again</Trans>
        </Button>
      </div>
    );
  const game = result.data;
  if (!game)
    return (
      <section className="py-12">
        <h1 className="font-display text-2xl">
          <Trans>Game not found</Trans>
        </h1>
        <GameBackLink gameId={gameId} from={from} />
      </section>
    );
  const several = game.entries.length > 1;
  // The game's Play starts the release the user chose, else the first
  // that can launch.
  const chosen =
    game.entries.find((entry) => entry.id === game.defaultEntryId) ??
    playableEntry(game.entries, unavailable);
  const busy = launch.isPending || updates.checking || Boolean(running);
  const playLabel = running
    ? t({ message: "Running", comment: "The Play button while the game runs" })
    : launch.isPending
      ? t`Starting…`
      : updates.checking
        ? t`Checking…`
        : t({ message: "Play", comment: "Button: starts the game" });
  const launchable = (entry: GameEntry) => entry.canLaunch && !unavailable.has(entry.id);
  /** A release in the Release picker, with why it can't start. */
  const releaseLabel = (entry: GameEntry) => {
    const name = names.get(entry.id) ?? formatSource(entry.source);
    if (launchable(entry)) return name;
    if (unavailable.has(entry.id))
      return t({
        message: `${name} (unavailable)`,
        comment: "A release whose storage isn't connected",
      });
    const state = entry.state;
    return t({ message: `${name} (${state})`, comment: "A release and its install state" });
  };
  return (
    <article data-view="game" data-status={running ? "running" : game.status} className="mt-4">
      <GameBackLink gameId={gameId} from={from} className="text-sm text-fg-muted" />
      <section
        {...menus({ gameId: game.id, title: game.title })}
        data-cs="hero"
        className="relative mt-4 overflow-hidden rounded-panel border border-border bg-surface-raised"
      >
        <GameArt part="art" game={game} image={game.hero} className="h-56 w-full" />
        <div data-part="content" className="flex flex-wrap items-start gap-6 p-6">
          <div className="min-w-0 flex-[1_1_32rem]">
            <StatusBadge part="badge" status={game.status} />
            <h1 data-part="title" className="mt-3 font-display text-4xl font-semibold">
              {game.title}
            </h1>
            <p data-part="meta" className="mt-2 text-sm text-fg-muted">
              {/* Only what the game has: no stray separators. */}
              {[
                game.genre,
                game.platform,
                formatSize(game.sizeBytes),
                game.releaseDate?.slice(0, 4),
              ]
                .filter(Boolean)
                .join(" · ")}
              <GameAgeRating gameId={game.id} />
            </p>
            <p data-part="meta" className="mt-3 flex items-center gap-2 text-fg-muted">
              <Clock className="size-4" />
              <span>{formatPlaytime(game.playtimeSeconds)}</span>
              <PlaytimeSources gameId={game.id} totalSeconds={game.playtimeSeconds} />
              <AchievementSummary gameId={game.id} />
            </p>
            <p
              data-part="description"
              className="mt-4 max-w-3xl text-sm leading-relaxed text-fg-muted"
            >
              {game.description || t`No description yet.`}
            </p>
            <div data-part="tags" className="mt-4 flex flex-wrap gap-2">
              {game.tags.map((tag) => (
                <span
                  key={tag}
                  className="rounded-full border border-border px-3 py-1 text-xs text-fg-muted"
                >
                  {tag}
                </span>
              ))}
            </div>
            {game.website && (
              <a
                data-part="website"
                href={game.website}
                target="_blank"
                rel="noreferrer"
                className="mt-4 inline-flex items-center gap-1.5 text-sm text-accent-alt hover:underline"
              >
                {t({ message: "Official site", comment: "A link to a game's website" })}
                <span className="text-fg-subtle">{hostOf(game.website)}</span>
                <ExternalLink aria-hidden="true" className="size-3.5" />
              </a>
            )}
          </div>
          <div data-part="actions" className="flex w-full flex-col gap-3 sm:w-80">
            {running ? (
              <StopButton sessionId={running.id} ref={first} />
            ) : (
              <PlayButton
                ref={first}
                primary
                named={several}
                entryId={chosen?.id}
                emulated={chosen?.emulated ?? false}
                isDisabled={!chosen || !launchable(chosen) || busy}
                onPress={() => chosen && updates.play(chosen.id)}
                label={playLabel}
              />
            )}
            {several && chosen && (
              <Select
                label={t({
                  message: "Release",
                  comment: "Picks which of the game's releases Play starts",
                })}
                options={game.entries.map((entry) => ({
                  id: entry.id,
                  label: releaseLabel(entry),
                }))}
                value={chosen.id}
                onChange={(entryId) => setDefault?.mutate({ gameId: game.id, entryId })}
                isDisabled={!setDefault || setDefault.isPending}
              />
            )}
            {chosen && !running && (
              <PlayChoice
                gameId={game.id}
                title={game.title}
                entryId={chosen.id}
                isDisabled={busy}
              />
            )}
            {!running && (
              <OwnedReleases
                gameId={game.id}
                status={game.status}
                primary={!game.entries.some(launchable)}
              />
            )}
            {edit && editable && (
              <UiButton
                data-part="edit"
                data-spatial="focusable"
                icon={Pencil}
                onPress={() => edit({ gameId: game.id, title: game.title })}
              >
                <Trans>Edit details</Trans>
              </UiButton>
            )}
            {setDefault?.isError && (
              <p role="alert" className="text-sm text-danger">
                {setDefault.error.message}
              </p>
            )}
            {launch.isError && !updates.asking && (
              <p role="alert" className="text-sm text-danger">
                {launch.error.message}
              </p>
            )}
          </div>
        </div>
      </section>
      <Screenshots title={game.title} screenshots={game.screenshots} loading={imagesLoading} />
      <GameVideos gameId={game.id} title={game.title} />
      <section className="mt-8" aria-labelledby="entries-heading">
        <h2 id="entries-heading" className="font-display text-xl font-semibold">
          <Trans>Play from your library</Trans>
        </h2>
        {game.entries.length === 0 && (
          <p className="mt-4 text-fg-muted">
            <Trans>
              Caskade can't start this game: it has nothing to launch. Add a program or URI in Edit
              details → Actions.
            </Trans>
          </p>
        )}
        <div className="mt-4 grid grid-cols-[repeat(auto-fit,minmax(min(100%,34rem),1fr))] gap-3">
          {game.entries.map((entry) => {
            const name = names.get(entry.id) ?? formatSource(entry.source);
            return (
              <div
                key={entry.id}
                {...menus({ gameId: game.id, title: game.title, entryId: entry.id })}
                data-part="entry"
                data-status={unavailable.has(entry.id) ? "unavailable" : entry.state}
                className="flex min-w-0 flex-col gap-3 rounded-panel border border-border bg-surface-raised p-4"
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0 flex-[1_1_16rem]">
                    <h3 className="flex flex-wrap items-center gap-2 font-semibold">
                      {name}
                      {several && entry.id === chosen?.id && (
                        <span
                          data-part="default"
                          className="rounded-full border border-accent px-2 py-0.5 text-xs font-normal text-accent-alt"
                        >
                          <Trans comment="The release the game's Play starts">Default</Trans>
                        </span>
                      )}
                    </h3>
                    <p className="text-xs text-fg-subtle">
                      {name === formatSource(entry.source)
                        ? formatInstallState(entry.state)
                        : `${formatSource(entry.source)} · ${formatInstallState(entry.state)}`}
                    </p>
                    <p className="mt-1 break-all font-mono text-xs text-fg-muted">
                      {entry.location ?? t`Install location unknown`}
                    </p>
                  </div>
                  <div className="flex flex-wrap items-start justify-end gap-2">
                    {entry.source === "caskade-storage" && (
                      <MoveGameAction
                        gameId={game.id}
                        entryId={entry.id}
                        isRunning={Boolean(running)}
                      />
                    )}
                    {several && (
                      <SeparateEntry entryId={entry.id} isRunning={running?.entryId === entry.id} />
                    )}
                    {several &&
                      (running?.entryId === entry.id ? (
                        <span
                          data-part="playing"
                          className="self-center font-mono text-xs tracking-widest text-accent-alt uppercase"
                        >
                          <Trans comment="The game is running">Playing</Trans>
                        </span>
                      ) : (
                        <PlayButton
                          named
                          entryId={entry.id}
                          emulated={entry.emulated}
                          isDisabled={!launchable(entry) || busy}
                          onPress={() => updates.play(entry.id)}
                          label={playLabel}
                        />
                      ))}
                  </div>
                </div>
                {entry.emulated && <EmulatedEntry entryId={entry.id} />}
                <EntryUpdate gameId={game.id} entryId={entry.id} isRunning={Boolean(running)} />
              </div>
            );
          })}
        </div>
        {updates.dialogs}
      </section>
      <GameAchievements gameId={game.id} />
      <PluginGameDetails gameId={game.id} />
      <section className="mt-8" aria-labelledby="sessions-heading">
        <h2 id="sessions-heading" className="font-display text-xl font-semibold">
          <Trans>Session history</Trans>
        </h2>
        {sessions.isError && (
          <p role="alert" className="mt-3 text-fg-muted">
            <Trans>Could not load session history.</Trans>
          </p>
        )}
        {sessions.isPending && (
          <p role="status" className="mt-3 text-fg-muted">
            <Trans>Loading sessions…</Trans>
          </p>
        )}
        {sessions.data?.length === 0 && (
          <p className="mt-3 text-fg-muted">
            <Trans>Your first session will appear here.</Trans>
          </p>
        )}
        <ol className="mt-4 space-y-2">
          {sessions.data?.map((session) => (
            <li
              key={session.id}
              className="flex justify-between gap-4 rounded-control border border-border px-4 py-3 text-sm"
            >
              <time dateTime={session.startedAt}>{formatDate(session.startedAt)}</time>
              <span className="font-mono text-fg-muted">
                {session.endedAt ? formatDuration(session.durationSeconds) : t`Playing now`}
              </span>
            </li>
          ))}
        </ol>
      </section>
      <IgdbPanel
        gameId={game.id}
        title={game.title}
        onMerged={(merged) => replaceGame(merged, from)}
      />
    </article>
  );
}
