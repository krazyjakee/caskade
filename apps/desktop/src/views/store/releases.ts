/** How the Store shows IGDB release dates (#163), honestly: only as much of
 * a date as IGDB knows. A release's `date` is the UTC day IGDB files it
 * under; for month, quarter and year precision it only stands in for the
 * span, so the label shows the span. Days are shown as the UTC day, never
 * shifted into the local time zone. */
import type { MessageDescriptor } from "@lingui/core";
import { msg, t } from "@lingui/core/macro";
import type { CatalogPage, IgdbGame, IgdbReleaseDate, ListedRelease } from "../../api/igdb";
import { formatMonth, formatReleaseDate } from "../../api/presentation";

const tbd = () =>
  t({
    message: "TBD",
    comment: "A release date that isn't known yet (to be determined)",
  });

/** When the release is, in the app's language: "14 Mar 2024", "Jun 2027",
 * "Q1 2027", "2026" or "TBD". */
export function releaseWhen(release: IgdbReleaseDate): string {
  const { year, month, quarter } = release;
  switch (release.precision) {
    case "day": {
      const [y, m, d] = (release.date ?? "").split("-").map(Number);
      if (y && m && d) return formatReleaseDate(release.date as string);
      break;
    }
    case "month":
      if (year && month && month >= 1 && month <= 12) return formatMonth(year, month);
      break;
    case "quarter":
      if (year && quarter)
        return t({
          message: `Q${quarter} ${year}`,
          comment: "A release date known to the quarter of a year: Q1 2027",
        });
      break;
    case "year":
      if (year) return String(year);
      break;
    case "tbd":
      return tbd();
  }
  return release.human?.trim() || tbd();
}

/** What the release's status says about it, where IGDB says anything. */
export type ReleaseKind = "released" | "early-access" | "cancelled" | "in-development" | "other";

export function releaseKind(release: IgdbReleaseDate): ReleaseKind | null {
  const status = release.status?.trim().toLowerCase();
  if (!status) return null;
  if (status.startsWith("cancel")) return "cancelled";
  if (status.includes("early access")) return "early-access";
  if (status === "full release") return "released";
  if (status === "alpha" || status === "beta") return "in-development";
  return "other";
}

const STATUSES: Record<string, MessageDescriptor> = {
  released: msg({ message: "Released", comment: "A game's or release's status on IGDB" }),
  "full release": msg({ message: "Full release", comment: "A game's or release's status on IGDB" }),
  alpha: msg({ message: "Alpha", comment: "A game's or release's status on IGDB" }),
  beta: msg({ message: "Beta", comment: "A game's or release's status on IGDB" }),
  "early access": msg({ message: "Early access", comment: "A game release's status" }),
  offline: msg({ message: "Offline", comment: "A game's or release's status on IGDB" }),
  cancelled: msg({ message: "Cancelled", comment: "A game release's status" }),
  rumored: msg({ message: "Rumoured", comment: "A game's or release's status on IGDB" }),
  delisted: msg({ message: "Delisted", comment: "A game's or release's status on IGDB" }),
};

/** IGDB's status in the language in use; one Caskade doesn't know as it came. */
export function statusName(status: string): string {
  const known = STATUSES[status.trim().toLowerCase()];
  return known ? t(known) : status;
}

const REGIONS: Record<string, MessageDescriptor> = {
  worldwide: msg({ message: "Worldwide", comment: "A game release's region" }),
  europe: msg({ message: "Europe", comment: "A game release's region" }),
  north_america: msg({ message: "North America", comment: "A game release's region" }),
  australia: msg({ message: "Australia", comment: "A game release's region" }),
  new_zealand: msg({ message: "New Zealand", comment: "A game release's region" }),
  japan: msg({ message: "Japan", comment: "A game release's region" }),
  china: msg({ message: "China", comment: "A game release's region" }),
  asia: msg({ message: "Asia", comment: "A game release's region" }),
  korea: msg({ message: "Korea", comment: "A game release's region" }),
  brazil: msg({ message: "Brazil", comment: "A game release's region" }),
};

export function regionName(region: string | null): string | null {
  if (!region) return null;
  const known = REGIONS[region];
  if (known) return t(known);
  return region
    .split("_")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

/** "PS5 · Europe": the platform and region a release is for, where known. */
export function releaseWhere(release: IgdbReleaseDate): string {
  const platform = release.platform?.abbreviation || release.platform?.name;
  const region = release.region === "worldwide" ? null : regionName(release.region);
  return [platform, region].filter(Boolean).join(" · ");
}

/** The platforms line of a card: abbreviations, at most three and a count. */
export function platformsLine(game: IgdbGame): string {
  const names = game.platforms.flatMap((p) => p.abbreviation || p.name || []);
  if (names.length <= 3) return names.join(" · ");
  return `${names.slice(0, 3).join(" · ")} +${names.length - 3}`;
}

/** The pages' games without repeats, each with the release that listed it
 * first: release pages count release rows, so a game can come up again on
 * a later page for another platform. */
export function listedGames(
  pages: CatalogPage[],
): { game: IgdbGame; release: ListedRelease["release"] | null }[] {
  const seen = new Set<string>();
  const out: { game: IgdbGame; release: ListedRelease["release"] | null }[] = [];
  for (const page of pages) {
    const releases = new Map<string, ListedRelease["release"]>();
    for (const listed of page.releases)
      if (!releases.has(listed.game)) releases.set(listed.game, listed.release);
    for (const game of page.games) {
      if (seen.has(game.id)) continue;
      seen.add(game.id);
      out.push({ game, release: releases.get(game.id) ?? null });
    }
  }
  return out;
}

/** A game's releases in the order to list them: dated ones by date, then
 * the undated. */
export function sortedReleases(game: IgdbGame): IgdbReleaseDate[] {
  return [...game.releaseDates].sort((a, b) => {
    if (a.date === b.date) return 0;
    if (a.date === null) return 1;
    if (b.date === null) return -1;
    return a.date < b.date ? -1 : 1;
  });
}
