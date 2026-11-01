import { GameArt } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { CalendarDays, Clock, History, type LucideIcon, PackagePlus } from "lucide-react";
import { useRef } from "react";
import { useGames, useLibraryStats, useRunningCount } from "../../api/hooks";
import { formatDuration, formatPlaytime } from "../../api/presentation";
import type { GameSummary, LibrarySort } from "../../api/types";
import { useGameMenus } from "../../components/gameMenu/GameMenuHost";
import { ShelfRow } from "../../components/ShelfRow";
import { useDefaultFocus } from "../../input";
import { PluginHomeSections } from "../../plugins/GameSlots";
import { useOpenGame } from "../game/origin";
import { LibraryCard } from "../library/LibraryCard";

const SHELF_SIZE = 12;

export function HomeView() {
  const { t } = useLingui();
  const running = useRunningCount();
  // The hero and the shelf's 12.
  const recent = useGames({ filter: "recent", sort: "recent" }, { pageSize: SHELF_SIZE + 1 });
  const stats = useLibraryStats();
  const games = recent.data?.pages.flatMap((page) => page.items) ?? [];
  // The backend projects recent games from persisted PlaySessions.
  const continueGame = games[0];
  // The shelf leaves out the hero's game.
  const shelf = games.slice(1, SHELF_SIZE + 1);
  const shelfShown = games.length !== 1 || recent.isError || running.isError;
  const active = running.data ?? 0;
  const total = formatDuration(stats.data?.playtimeSeconds ?? 0);
  // With a pad, Home opens on the hero's action.
  const resume = useRef<HTMLAnchorElement>(null);
  useDefaultFocus(resume, Boolean(continueGame));
  const menus = useGameMenus();
  // Its page, which goes back to Home (#195).
  const openGame = useOpenGame();
  return (
    <div data-view="home" className="mt-4">
      <div className="mb-5 flex items-center justify-between gap-4">
        <h1 className="font-display text-2xl font-semibold">
          <Trans>Welcome back</Trans>
        </h1>
        <p className="flex items-center gap-2 font-mono text-xs text-fg-muted">
          <Clock className="size-4" />
          <Trans comment="Home's header: the library's total playtime and the games running now">
            {total} total · {active} running
          </Trans>
        </p>
      </div>
      {continueGame && (
        <section
          {...menus({ gameId: continueGame.id, title: continueGame.title })}
          data-cs="hero"
          className="relative flex min-h-64 overflow-hidden rounded-panel border border-border bg-surface-raised"
        >
          <GameArt
            part="art"
            game={continueGame}
            image={continueGame.coverGrid}
            className="w-1/3 shrink-0"
          />
          <div data-part="content" className="flex flex-col justify-center p-8">
            <p
              data-part="eyebrow"
              className="font-mono text-xs tracking-widest text-accent-alt uppercase"
            >
              <Trans>Continue Playing</Trans>
            </p>
            <h2 data-part="title" className="mt-3 font-display text-3xl font-semibold">
              {continueGame.title}
            </h2>
            <p data-part="meta" className="mt-3 text-sm text-fg-muted">
              {formatPlaytime(continueGame.playtimeSeconds)}
            </p>
            <Link
              ref={resume}
              to="/games/$gameId"
              params={{ gameId: continueGame.id }}
              onClick={(event) => {
                if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey) return;
                event.preventDefault();
                openGame(continueGame.id);
              }}
              data-part="actions"
              data-focus-default
              className="mt-5 w-fit rounded-control bg-accent px-5 py-2 font-semibold text-accent-fg"
            >
              <Trans>Resume · View details</Trans>
            </Link>
          </div>
        </section>
      )}
      {shelfShown && (
        <ShelfRow
          id="recent"
          title={t`Recently Played`}
          icon={History}
          viewAll={
            <Link
              to="/library"
              search={{ filter: "recent", sort: "recent" }}
              data-part="view-all"
              className="text-sm text-accent-alt"
            >
              <Trans>View all →</Trans>
            </Link>
          }
          notice={
            <>
              {recent.isPending && (
                <p role="status" className="text-fg-muted">
                  <Trans>Loading recent games…</Trans>
                </p>
              )}
              {(recent.isError || running.isError) && (
                <p role="alert" className="text-fg-muted">
                  <Trans>Could not load your recent activity.</Trans>
                </p>
              )}
              {recent.isSuccess && games.length === 0 && (
                <div className="rounded-panel border border-dashed border-border p-10 text-center text-fg-muted">
                  <p>
                    <Trans>No sessions yet. Pick something from your library.</Trans>
                  </p>
                  <Link
                    to="/library"
                    search={{ filter: "all" }}
                    className="mt-3 inline-block text-accent-alt"
                  >
                    <Trans>Browse library →</Trans>
                  </Link>
                </div>
              )}
            </>
          }
        >
          {shelf.map((game) => (
            <LibraryCard key={game.id} game={game} />
          ))}
        </ShelfRow>
      )}
      <NewGamesShelf id="added" title={t`Recently Added`} icon={PackagePlus} sort="added" />
      <NewGamesShelf
        id="released"
        title={t`Recently Released`}
        icon={CalendarDays}
        sort="released"
        // Pre-orders and announced games wait for their date.
        pick={(games) => games.filter((game) => game.releaseDate && game.releaseDate <= today())}
        // Room for the undated and the unreleased the shelf leaves out.
        pageSize={SHELF_SIZE * 2}
      />
      <PluginHomeSections />
    </div>
  );
}

/** The local calendar date, `YYYY-MM-DD` like release dates. */
function today() {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** A shelf of the whole library's first games in `sort`, left out until
 * it has some. */
function NewGamesShelf({
  id,
  title,
  icon,
  sort,
  pick,
  pageSize = SHELF_SIZE,
}: {
  id: string;
  title: string;
  icon: LucideIcon;
  sort: LibrarySort;
  /** The games to show of those fetched. */
  pick?: (games: GameSummary[]) => GameSummary[];
  pageSize?: number;
}) {
  const result = useGames({ filter: "all", sort }, { pageSize });
  const all = result.data?.pages.flatMap((page) => page.items) ?? [];
  const games = (pick ? pick(all) : all).slice(0, SHELF_SIZE);
  if (!result.isError && games.length === 0) return null;
  return (
    <ShelfRow
      id={id}
      title={title}
      icon={icon}
      viewAll={
        <Link
          to="/library"
          search={{ filter: "all", sort }}
          data-part="view-all"
          className="text-sm text-accent-alt"
        >
          <Trans>View all →</Trans>
        </Link>
      }
      notice={
        result.isError && (
          <p role="alert" className="text-fg-muted">
            <Trans>Could not load these games.</Trans>
          </p>
        )
      }
    >
      {games.map((game) => (
        <LibraryCard key={game.id} game={game} />
      ))}
    </ShelfRow>
  );
}
