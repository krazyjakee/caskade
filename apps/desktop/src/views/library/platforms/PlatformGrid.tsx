import { plural } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { Clock, Gamepad2, History } from "lucide-react";
import type { CSSProperties } from "react";
import { formatDuration, formatRelativeDay } from "../../../api/presentation";
import type {
  LibraryFilter,
  LibraryLayout,
  LibrarySort,
  PlatformSummary,
} from "../../../api/types";
import { presentPlatform } from "./catalog";

/** The library's games broken down by platform, for a filter whose games
 * span several (Emulated). A card opens the filter on that platform. */
export function PlatformGrid({
  platforms,
  filter,
  sort,
  layout,
}: {
  /** In the order to show them. */
  platforms: PlatformSummary[];
  filter: LibraryFilter;
  sort: LibrarySort;
  layout: LibraryLayout;
}) {
  const { t } = useLingui();
  return (
    <section
      aria-label={t({ message: "Platforms", comment: "The library's games grouped by platform" })}
      data-focus-section="platform-grid"
      className="grid min-h-0 grow basis-0 auto-rows-min grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-4 overflow-auto overscroll-contain pb-4"
    >
      {platforms.map((platform, index) => (
        <PlatformCard
          key={platform.name}
          platform={platform}
          index={index}
          search={{ filter, sort, layout, platform: platform.name }}
        />
      ))}
    </section>
  );
}

function PlatformCard({
  platform,
  index,
  search,
}: {
  platform: PlatformSummary;
  index: number;
  search: { filter: LibraryFilter; sort: LibrarySort; layout: LibraryLayout; platform: string };
}) {
  const { t } = useLingui();
  const look = presentPlatform(platform.name);
  const title = look.title;
  const games = gamesLabel(platform.games);
  const lastPlayed = platform.lastPlayedAt ? formatRelativeDay(platform.lastPlayedAt) : null;
  return (
    <Link
      to="/library"
      search={search}
      data-cs="platform-card"
      data-art={look.known ? "platform" : "generic"}
      data-focus-key={`platform-card:${platform.name}`}
      aria-label={t({
        message: `${title}, ${games}`,
        comment: "A platform's card: its name and how many games it has",
      })}
      style={{ "--i": Math.min(index, 12) } as CSSProperties}
      className="group relative flex animate-enter flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none transition-colors duration-(--cs-motion-duration-fast) hover:border-accent-alt/60 hover:bg-surface-hover focus-visible:focus-ring-over"
    >
      <div
        data-part="art"
        className="relative aspect-[3/2] overflow-hidden bg-surface-base bg-[radial-gradient(ellipse_at_50%_75%,color-mix(in_oklab,var(--cs-color-accent)_35%,transparent)_0%,transparent_70%)]"
      >
        <div
          aria-hidden
          className="absolute inset-x-6 bottom-3 h-6 rounded-[50%] bg-surface-base/70 blur-md"
        />
        {look.art ? (
          <img
            src={look.art}
            alt=""
            draggable={false}
            loading="lazy"
            decoding="async"
            className="relative size-full object-contain p-4 drop-shadow-[0_10px_14px_rgb(0_0_0/0.45)] transition-transform duration-(--cs-motion-duration-base) group-hover:scale-105 group-focus-visible:scale-105"
          />
        ) : (
          <Gamepad2 className="relative m-auto size-full p-10 text-fg-subtle" />
        )}
      </div>
      <div className="flex flex-col gap-1 px-3 pt-2.5 pb-3">
        <p data-part="title" className="truncate font-display text-base font-semibold">
          {look.title}
        </p>
        {look.subtitle && (
          <p data-part="subtitle" className="truncate text-xs text-fg-subtle">
            {look.subtitle}
          </p>
        )}
        <div className="mt-1.5 flex items-center justify-between gap-2 text-xs text-fg-muted">
          <span
            data-part="count"
            className="shrink-0 rounded-full border border-border bg-surface-base/60 px-2 py-0.5 font-mono whitespace-nowrap"
          >
            {games}
          </span>
          <span
            data-part="meta"
            className="flex min-w-0 items-center gap-2 overflow-hidden font-mono whitespace-nowrap"
          >
            {platform.playtimeSeconds > 0 && (
              <span data-part="playtime" className="flex shrink-0 items-center gap-1">
                <Clock className="size-3" aria-hidden />
                {formatDuration(platform.playtimeSeconds)}
              </span>
            )}
            {lastPlayed && (
              <span data-part="last-played" className="flex items-center gap-1 truncate">
                <History className="size-3" aria-hidden />
                {lastPlayed}
              </span>
            )}
          </span>
        </div>
      </div>
    </Link>
  );
}

const gamesLabel = (total: number) => plural({ total }, { one: "# game", other: "# games" });
