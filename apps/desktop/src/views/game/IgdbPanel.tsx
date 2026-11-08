import { Button, TextField } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Cloud, ExternalLink, RefreshCw, Search, Sparkles, X } from "lucide-react";
import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import {
  type IgdbGame,
  type MatchSource,
  useChangeGameIgdb,
  useGameIgdb,
  useIgdbSearch,
} from "../../api/igdb";
import { formatReleaseDate } from "../../api/presentation";
import { useHasSettings, useSettingsSection } from "../settings/useSettings";

const sources: Record<MatchSource, MessageDescriptor> = {
  romm: msg`Matched by your RomM server`,
  hasheous: msg`Matched by name (Hasheous)`,
  store: msg`Matched by store id`,
  manual: msg({ message: "Chosen by you", comment: "How the game's IGDB match was made" }),
  none: msg`No match found`,
};

/** Typing pauses this long before a search goes out. */
const SEARCH_DELAY_MS = 300;

/** The game's IGDB match: what IGDB says about it, and the controls to
 * change, refresh or drop the match. */
export function IgdbPanel({
  gameId,
  title,
  onMerged,
}: {
  gameId: string;
  title: string;
  /** A new match merged the game into another that is the same game. */
  onMerged?: (gameId: string) => void;
}) {
  const { t } = useLingui();
  const match = useGameIgdb(gameId);
  const change = useChangeGameIgdb(gameId, onMerged);
  // A title or an IGDB id, while changing the match.
  const [draft, setDraft] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  useEffect(() => {
    const timer = setTimeout(() => setQuery(draft ?? ""), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [draft]);
  const search = useIgdbSearch(query);
  const data = match.data;
  const game = data?.game ?? null;
  const id = draft?.trim() ?? "";
  const valid = /^[1-9]\d{0,18}$/.test(id);
  const choose = (igdbId: string) =>
    change.mutate({ type: "set", igdbId }, { onSuccess: () => setDraft(null) });
  const igdbId = data?.igdbId;
  const source =
    data &&
    [
      t(sources[data.source]),
      igdbId && t`IGDB ID ${igdbId}`,
      data.pending &&
        t({ message: "not filled in yet", comment: "The IGDB match's details aren't applied yet" }),
    ]
      .filter(Boolean)
      .join(" · ");
  const searched = query.trim();
  const rating = game?.totalRating == null ? null : Math.round(game.totalRating);
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (valid) choose(id);
  };
  return (
    <section data-part="igdb" className="mt-8" aria-labelledby="igdb-heading">
      <h2 id="igdb-heading" className="font-display text-xl font-semibold">
        IGDB
      </h2>
      <div className="mt-4 rounded-panel border border-border bg-surface-raised p-4">
        {match.isPending && (
          <p role="status" className="text-fg-muted">
            <Trans>Loading the IGDB match…</Trans>
          </p>
        )}
        {match.isError && <p className="text-fg-muted">{match.error.message}</p>}
        {data === null && (
          <OnlineLookups>
            <p className="text-fg-muted">
              <Trans>Not matched yet. Caskade matches games with IGDB in the background.</Trans>
            </p>
          </OnlineLookups>
        )}
        {data && game && (
          <dl data-part="igdb-facts" className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
            <dt className="text-fg-subtle">
              <Trans comment="The IGDB game the library game is matched to">Game</Trans>
            </dt>
            <dd>
              {game.url ? (
                <a
                  href={game.url}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1.5 text-accent-alt hover:underline"
                >
                  {game.title}
                  <ExternalLink aria-hidden="true" className="size-3.5" />
                </a>
              ) : (
                game.title
              )}
            </dd>
            {game.releaseDate && (
              <>
                <dt className="text-fg-subtle">
                  <Trans>Released</Trans>
                </dt>
                <dd>{formatReleaseDate(game.releaseDate)}</dd>
              </>
            )}
            {game.totalRating !== null && (
              <>
                <dt className="text-fg-subtle">
                  <Trans>IGDB rating</Trans>
                </dt>
                <dd className="font-mono">
                  <Trans comment="A rating out of 100">{rating} / 100</Trans>
                </dd>
              </>
            )}
            {game.genres.length > 0 && (
              <>
                <dt className="text-fg-subtle">
                  <Trans>Genres</Trans>
                </dt>
                <dd>{game.genres.join(" · ")}</dd>
              </>
            )}
            {game.platforms.length > 0 && (
              <>
                <dt className="text-fg-subtle">
                  <Trans>Platforms</Trans>
                </dt>
                <dd>
                  {game.platforms
                    .map((platform) => platform.name ?? platform.abbreviation)
                    .filter(Boolean)
                    .join(" · ")}
                </dd>
              </>
            )}
          </dl>
        )}
        {data && !game && data.igdbId && (
          <OnlineLookups>
            <p className="text-fg-muted">
              <Trans>
                IGDB game {igdbId}. Its details show once Caskade can reach the metadata server.
              </Trans>
            </p>
          </OnlineLookups>
        )}
        {data && !data.igdbId && data.source === "manual" && (
          <p className="text-fg-muted">
            <Trans>You marked this game as not on IGDB.</Trans>
          </p>
        )}
        {data && !data.igdbId && data.source !== "manual" && (
          <OnlineLookups>
            <p className="text-fg-muted">
              <Trans>Caskade found no IGDB game for it.</Trans>
            </p>
          </OnlineLookups>
        )}
        {data && (
          <p data-part="igdb-source" className="mt-3 text-xs text-fg-subtle">
            {source}
          </p>
        )}
        {draft === null ? (
          <div className="mt-4 flex flex-wrap gap-2">
            {data?.igdbId && (
              <Button
                size="sm"
                icon={RefreshCw}
                isDisabled={change.isPending}
                onPress={() => change.mutate({ type: "refresh" })}
              >
                <Trans comment="Button: fetches the IGDB game's details again">Refresh</Trans>
              </Button>
            )}
            <Button
              size="sm"
              icon={Search}
              isDisabled={change.isPending || match.isPending}
              onPress={() => {
                change.reset();
                setDraft(title);
              }}
            >
              {data?.igdbId ? t`Change IGDB game` : t`Set IGDB game`}
            </Button>
            {data?.igdbId && (
              <Button
                size="sm"
                variant="ghost"
                icon={X}
                isDisabled={change.isPending}
                onPress={() => change.mutate({ type: "set", igdbId: null })}
              >
                <Trans>Not on IGDB</Trans>
              </Button>
            )}
            {(data === null || data?.source === "manual" || data?.source === "none") && (
              <Button
                size="sm"
                variant="ghost"
                icon={Sparkles}
                isDisabled={change.isPending}
                onPress={() => change.mutate({ type: "rematch" })}
              >
                {data?.source === "manual" ? t`Match automatically` : t`Match now`}
              </Button>
            )}
          </div>
        ) : (
          <form onSubmit={submit} className="mt-4">
            <div className="flex flex-wrap items-end gap-2">
              <TextField
                label={t`Search IGDB`}
                description={t`A title, or the number listed as IGDB ID on the game's igdb.com page.`}
                icon={Search}
                autoFocus
                value={draft}
                onChange={setDraft}
                className="w-96 max-w-full"
              />
              <Button size="sm" variant="ghost" onPress={() => setDraft(null)}>
                <Trans>Cancel</Trans>
              </Button>
            </div>
            <ul data-part="igdb-results" aria-label={t`IGDB games`} className="mt-3 space-y-1.5">
              {valid && (
                <ResultRow
                  title={t`IGDB ID ${id}`}
                  detail={t`The game with this id on igdb.com`}
                  current={id === data?.igdbId}
                  isDisabled={change.isPending}
                  onUse={() => choose(id)}
                />
              )}
              {search.data?.map((result) => (
                <ResultRow
                  key={result.id}
                  title={result.title}
                  detail={describe(result)}
                  url={result.url}
                  current={result.id === data?.igdbId}
                  isDisabled={change.isPending}
                  onUse={() => choose(result.id)}
                />
              ))}
            </ul>
            {(search.isFetching || change.isPending) && (
              <p role="status" className="mt-2 text-sm text-fg-muted">
                {change.isPending ? t`Checking the IGDB game…` : t`Searching IGDB…`}
              </p>
            )}
            {search.isError && <p className="mt-2 text-sm text-fg-muted">{search.error.message}</p>}
            {search.data?.length === 0 && !search.isFetching && (
              <p className="mt-2 text-sm text-fg-muted">
                <Trans>No IGDB games match “{searched}”.</Trans>
              </p>
            )}
          </form>
        )}
        {change.isError && (
          <p role="alert" className="mt-3 text-sm text-danger">
            {change.error.message}
          </p>
        )}
      </div>
    </section>
  );
}

/** `children`, or while online lookups are off on this device, why the
 * game isn't matched or filled in and the switch to turn them on (#75).
 * Outside a settings provider (views rendered on their own), `children`. */
function OnlineLookups({ children }: { children: ReactNode }) {
  return useHasSettings() ? <LookupsOff>{children}</LookupsOff> : children;
}

function LookupsOff({ children }: { children: ReactNode }) {
  const metadata = useSettingsSection<{ online: boolean }>("core.metadata");
  if (metadata.settings?.online !== false) return children;
  return (
    <div data-part="igdb-lookups-off">
      <p className="text-fg-muted">
        <Trans>
          Online lookups are off on this device, so Caskade doesn't match games with IGDB or fetch
          their details and artwork.
        </Trans>
      </p>
      <Button
        size="sm"
        icon={Cloud}
        className="mt-3"
        isDisabled={metadata.saving}
        onPress={() => metadata.setSettings((current) => ({ ...current, online: true }))}
      >
        <Trans>Turn on online lookups</Trans>
      </Button>
      {metadata.saveError && (
        <p role="alert" className="mt-2 text-sm text-danger">
          {metadata.saveError.message}
        </p>
      )}
    </div>
  );
}

/** A search result's year and platforms. */
function describe(game: IgdbGame): string {
  const platforms = game.platforms.map((platform) => platform.abbreviation ?? platform.name);
  return [game.releaseDate?.slice(0, 4), ...platforms].filter(Boolean).join(" · ");
}

function ResultRow({
  title,
  detail,
  url,
  current,
  isDisabled,
  onUse,
}: {
  title: string;
  detail: string;
  url?: string | null;
  current: boolean;
  isDisabled: boolean;
  onUse: () => void;
}) {
  const { t } = useLingui();
  return (
    <li className="flex items-center justify-between gap-4 rounded-control border border-border px-3 py-2 text-sm">
      <div className="min-w-0">
        <p className="truncate font-medium">
          {url ? (
            <a
              href={url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1.5 hover:text-accent-alt hover:underline"
            >
              {title}
              <ExternalLink aria-hidden="true" className="size-3.5" />
            </a>
          ) : (
            title
          )}
        </p>
        <p className="truncate text-xs text-fg-subtle">{detail}</p>
      </div>
      {current ? (
        <span className="shrink-0 text-xs text-fg-subtle">
          <Trans>Current match</Trans>
        </span>
      ) : (
        <Button size="sm" aria-label={t`Use ${title}`} isDisabled={isDisabled} onPress={onUse}>
          <Trans comment="Button: makes this the game's IGDB match">Use</Trans>
        </Button>
      )}
    </li>
  );
}
