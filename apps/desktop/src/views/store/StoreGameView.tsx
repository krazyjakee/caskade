/** A catalog game's page in the Store (#163): what IGDB knows of it, where
 * it is sold, its releases on each platform and region, and similar games.
 * Opening it creates no library entry. A game the library has (the same
 * IGDB match) links to its library page. */
import { GameArt } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { ExternalLink, Library, Sparkles } from "lucide-react";
import { type MouseEvent, useRef } from "react";
import { type IgdbGame, type IgdbWebsite, useCatalogGames, useCatalogOwned } from "../../api/igdb";
import type { Screenshot } from "../../api/types";
import { AgeRatingLabel } from "../../components/AgeRatingLabel";
import { ShelfRow } from "../../components/ShelfRow";
import { useDefaultFocus } from "../../input";
import { GameBackLink } from "../game/BackLink";
import { STORE_GAMES, useOpenGame, useOpenStoreGame } from "../game/origin";
import { Screenshots } from "../game/Screenshots";
import { Videos } from "../game/Videos";
import { CatalogCard } from "./CatalogCard";
import {
  platformsLine,
  regionName,
  releaseKind,
  releaseWhen,
  sortedReleases,
  statusName,
} from "./releases";
import { WhereToBuy } from "./WhereToBuy";

const SIMILAR = 12;

export function StoreGameView({ igdbId, from }: { igdbId: string; from?: string }) {
  const lookup = useCatalogGames([igdbId]);
  const game = lookup.data?.games[0];
  const reason = lookup.error?.message;
  const back = (
    <GameBackLink
      gameId={igdbId}
      from={from}
      base={STORE_GAMES}
      fallback="/store"
      className="text-sm text-fg-muted"
    />
  );
  if (!game) {
    return (
      <article data-view="store-game" className="mt-4">
        {back}
        <div className="mt-6 text-sm text-fg-muted">
          {lookup.isPending ? (
            <p role="status">
              <Trans>Loading game…</Trans>
            </p>
          ) : lookup.isError ? (
            <p role="alert">
              <Trans>Could not reach the game catalog: {reason}</Trans>
            </p>
          ) : lookup.data?.notFound.includes(igdbId) ? (
            <p role="alert">
              <Trans>IGDB has no game {igdbId}.</Trans>
            </p>
          ) : (
            <p role="alert">
              <Trans>The metadata server sent nothing for this game yet. Try again later.</Trans>
            </p>
          )}
        </div>
      </article>
    );
  }
  return (
    <article data-view="store-game" className="mt-4">
      {back}
      <CatalogGame game={game} />
    </article>
  );
}

function CatalogGame({ game }: { game: IgdbGame }) {
  const { t } = useLingui();
  const owned = useCatalogOwned([game.id]).data;
  const library = owned?.find((copy) => copy.installed) ?? owned?.[0];
  const openGame = useOpenGame();
  const first = useRef<HTMLAnchorElement>(null);
  useDefaultFocus(first, true, game.id);
  const art = game.images.hero?.url ?? game.images.screenshots[0]?.url ?? null;
  const year = game.releaseDate?.slice(0, 4);
  return (
    <>
      <section
        data-cs="hero"
        className="relative mt-4 overflow-hidden rounded-panel border border-border bg-surface-raised"
      >
        <GameArt part="art" game={game} image={art} className="h-56 w-full" />
        <div data-part="content" className="flex flex-wrap items-start gap-6 p-6">
          <GameArt
            game={game}
            image={game.images.cover?.url}
            className="aspect-(--cs-card-aspect) w-36 shrink-0 rounded-card"
          />
          <div className="min-w-0 flex-[1_1_28rem]">
            <p
              data-part="eyebrow"
              className="font-mono text-xs tracking-widest text-accent-alt uppercase"
            >
              {game.status && game.status !== "Released"
                ? statusName(game.status)
                : t({
                    message: "IGDB catalog",
                    comment: "The eyebrow above a store game's title: where the page comes from",
                  })}
            </p>
            <h1 data-part="title" className="mt-2 font-display text-4xl font-semibold">
              {game.title}
            </h1>
            <p
              data-part="meta"
              className="mt-2 flex flex-wrap items-center gap-x-2 text-sm text-fg-muted"
            >
              {[game.genres.join(", "), platformsLine(game), year].filter(Boolean).join(" · ")}
              <AgeRatingLabel ratings={game.ageRatings} />
            </p>
            {library && (
              <p className="mt-3 flex items-center gap-2 text-sm text-status-installed">
                <Library className="size-4" aria-hidden />
                {library.installed
                  ? t({ message: "Installed", context: "game status" })
                  : t({
                      message: "In your library",
                      comment: "A store game the library has but isn't installed",
                    })}{" "}
                ·{" "}
                <Link
                  to="/games/$gameId"
                  params={{ gameId: library.gameId }}
                  onClick={(event: MouseEvent<HTMLAnchorElement>) => {
                    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey)
                      return;
                    event.preventDefault();
                    openGame(library.gameId);
                  }}
                  className="text-accent-alt underline-offset-2 hover:underline"
                >
                  <Trans>Open in library</Trans>
                </Link>
              </p>
            )}
            <p
              data-part="description"
              className="mt-4 max-w-3xl text-sm leading-relaxed text-fg-muted"
            >
              {game.description || t`No description on IGDB.`}
            </p>
          </div>
          <div data-part="actions" className="flex w-full flex-col gap-3 sm:w-80">
            <h2 className="font-display text-lg font-semibold">
              <Trans>Where to buy</Trans>
            </h2>
            <WhereToBuy links={game.storeLinks} first={first} />
            {game.url && (
              <a
                href={game.url}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 text-sm text-fg-muted hover:text-fg"
              >
                <Trans>IGDB page</Trans> <ExternalLink className="size-3.5" aria-hidden />
              </a>
            )}
          </div>
        </div>
      </section>
      <Screenshots title={game.title} screenshots={screenshots(game)} />
      <Videos title={game.title} videos={game.videos} />
      <Releases game={game} />
      <Websites websites={game.websites} />
      <SimilarGames game={game} />
    </>
  );
}

/** IGDB's screenshots for the gallery: the server's full-size copy, and
 * its smaller size for the row. */
function screenshots(game: IgdbGame): Screenshot[] {
  return game.images.screenshots.map((shot) => ({
    full: shot.url,
    thumb: shot.url.replace("/t_screenshot_huge/", "/t_screenshot_med/"),
  }));
}

/** Each release by platform and region, as precisely as IGDB dates it. */
function Releases({ game }: { game: IgdbGame }) {
  const { t } = useLingui();
  const releases = sortedReleases(game);
  return (
    <section className="mt-8">
      <h2 className="mb-3 font-display text-xl font-semibold">
        <Trans>Releases</Trans>
      </h2>
      {releases.length === 0 ? (
        <p className="text-sm text-fg-muted">
          <Trans>IGDB has no release dates for this game.</Trans>
        </p>
      ) : (
        <table className="w-full max-w-3xl text-left text-sm">
          <thead className="text-xs text-fg-subtle">
            <tr>
              <th className="py-1 font-normal">
                {t({ message: "Platform", comment: "A column of a store game's releases" })}
              </th>
              <th className="py-1 font-normal">
                {t({ message: "Region", comment: "A column of a store game's releases" })}
              </th>
              <th className="py-1 font-normal">
                {t({ message: "Date", comment: "A column of a store game's releases" })}
              </th>
              <th className="py-1 font-normal">
                {t({ message: "Status", comment: "A column of a store game's releases" })}
              </th>
            </tr>
          </thead>
          <tbody>
            {releases.map((release) => (
              <tr
                // Releases have no id; their content is their identity.
                key={`${release.platform?.id ?? release.platform?.name}-${release.region}-${release.date}-${release.precision}-${release.status}`}
                data-release={releaseKind(release) ?? undefined}
                className="border-t border-border"
              >
                <td className="py-1.5">{release.platform?.name ?? "—"}</td>
                <td className="py-1.5 text-fg-muted">{regionName(release.region) ?? "—"}</td>
                <td className="py-1.5 font-mono">{releaseWhen(release)}</td>
                <td className="py-1.5 text-fg-muted">
                  {release.status ? statusName(release.status) : ""}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

const WEBSITE_KINDS: Record<string, MessageDescriptor> = {
  official: msg({ message: "Official site", comment: "A link to a game's website" }),
  wiki: msg({ message: "Wiki", comment: "A link to a game's website" }),
  wikipedia: msg({ message: "Wikipedia", comment: "A link to a game's website" }),
  social: msg({ message: "Social", comment: "A link to a game's social media page" }),
  video: msg({ message: "Video", comment: "A link to a game's video page" }),
  community: msg({ message: "Community", comment: "A link to a game's community page" }),
};

/** The game's other websites, apart from where to buy it. */
function Websites({ websites }: { websites: IgdbWebsite[] }) {
  const { t } = useLingui();
  if (websites.length === 0) return null;
  return (
    <section className="mt-8">
      <h2 className="mb-3 font-display text-xl font-semibold">
        <Trans>Links</Trans>
      </h2>
      <ul className="flex flex-wrap gap-2">
        {websites.map((site) => {
          const kind = WEBSITE_KINDS[site.kind];
          return (
            <li key={site.url}>
              <a
                href={site.url}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 rounded-full border border-border px-3 py-1 text-xs text-fg-muted hover:text-fg focus-visible:focus-ring"
              >
                {kind
                  ? t(kind)
                  : (site.name ?? t({ message: "Website", comment: "A link to a game's website" }))}
                <ExternalLink className="size-3" aria-hidden />
              </a>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** IGDB's similar games, without the game itself or repeats. */
function SimilarGames({ game }: { game: IgdbGame }) {
  const { t } = useLingui();
  const ids = [...new Set(game.similarGames)].filter((id) => id !== game.id).slice(0, SIMILAR);
  const similar = useCatalogGames(ids);
  const owned = useCatalogOwned(ids).data ?? [];
  const openGame = useOpenStoreGame();
  const games = similar.data?.games ?? [];
  return (
    <ShelfRow
      id="store-similar"
      title={t`Similar games`}
      icon={Sparkles}
      notice={
        ids.length === 0 ? (
          <p className="text-sm text-fg-muted">
            <Trans>IGDB lists no similar games.</Trans>
          </p>
        ) : similar.isPending ? (
          <p role="status" className="text-sm text-fg-muted">
            <Trans>Loading similar games…</Trans>
          </p>
        ) : similar.isError ? (
          <p role="alert" className="text-sm text-fg-muted">
            <Trans>Could not load similar games.</Trans>
          </p>
        ) : games.length === 0 ? (
          <p className="text-sm text-fg-muted">
            <Trans>The similar games aren't available yet.</Trans>
          </p>
        ) : null
      }
    >
      {games.map((other) => (
        <CatalogCard
          key={other.id}
          game={other}
          href={`${STORE_GAMES}/${encodeURIComponent(other.id)}`}
          onOpen={openGame}
          owned={owned.find((copy) => copy.igdbId === other.id)}
          layout="shelf"
        />
      ))}
    </ShelfRow>
  );
}
