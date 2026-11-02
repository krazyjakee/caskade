import { originalPath, thumbnailPath } from "./artwork";
import type {
  CollectionKind,
  CollectionSummary,
  GameDetails,
  GameSummary,
  LibraryQuery,
  PlatformSummary,
  PlaySession,
} from "./types";

type ArtworkRecord = {
  kind?: string;
  cachedPath?: string;
  thumbnails: { size: number; path: string }[];
};
type GameRecord = {
  id: string;
  title: string;
  genres: string[];
  platforms: string[];
  favorite: boolean;
  releaseDate?: string | null;
  defaultEntry?: string | null;
};
type SummaryRecord = GameRecord & {
  adminPinNeeded?: boolean;
  installed: boolean;
  cover: ArtworkRecord | null;
  icon?: ArtworkRecord | null;
  playtimeMs: number;
  lastPlayedAtMs: number | null;
};
type EntryRecord = {
  id: string;
  sourceId: string;
  state: { type: string };
  size: number | null;
  location:
    | { type: "external"; path: string }
    | { type: "managed"; rootId: string; relDir: string }
    | null;
  /** `url` for a web game's page. */
  launch: { type: string; url?: string } | null;
};
/** Structural projections are checked against generated DTOs by native.ts. */
const INSTALLED_STATES = new Set(["installed", "updating", "broken"]);
function date(ms: number | null): string | null {
  if (ms === null) return null;
  const value = new Date(ms);
  return Number.isNaN(value.getTime()) ? null : value.toISOString();
}
function cover(
  artwork: ArtworkRecord | null | undefined,
  size: 96 | 320 | 960,
  assetUrl: (path: string) => string,
) {
  const path = thumbnailPath(artwork, size);
  return path ? assetUrl(path) : null;
}
export function mapSummary(record: SummaryRecord, assetUrl: (path: string) => string): GameSummary {
  return {
    adminPinNeeded: record.adminPinNeeded ?? false,
    id: record.id,
    title: record.title,
    genre: record.genres.join(" · "),
    platform: record.platforms.join(" · "),
    favorite: record.favorite,
    status: record.installed ? "installed" : "not-installed",
    playtimeSeconds: record.playtimeMs / 1000,
    lastPlayedAt: date(record.lastPlayedAtMs),
    releaseDate: record.releaseDate ?? null,
    sizeBytes: null,
    coverGrid: cover(record.cover, 320, assetUrl),
    coverList: cover(record.cover, 96, assetUrl),
    icon: cover(record.icon, 96, assetUrl),
  };
}
export function mapDetails(
  record: {
    game: GameRecord & { tags: string[]; description: string };
    entries: EntryRecord[];
    artwork: ArtworkRecord[];
    playtimeMs: number;
    lastPlayedAtMs: number | null;
  },
  assetUrl: (path: string) => string,
): GameDetails {
  const artwork = record.artwork.find((art) => art.kind === "cover");
  const hero = record.artwork.find((art) => art.kind === "hero") ?? artwork;
  const emulated = record.entries.some((entry) => entry.launch?.type === "emulator");
  return {
    ...mapSummary(
      {
        ...record.game,
        // As the core counts a game installed (`db/query.rs` `INSTALLED`):
        // a release being updated or with broken files is still on disk.
        installed: record.entries.some((entry) => INSTALLED_STATES.has(entry.state.type)),
        cover: artwork ?? null,
        icon: record.artwork.find((art) => art.kind === "icon") ?? null,
        playtimeMs: record.playtimeMs,
        lastPlayedAtMs: record.lastPlayedAtMs,
      },
      assetUrl,
    ),
    ...(emulated ? { status: "emulated" as const } : {}),
    description: record.game.description,
    tags: record.game.tags,
    defaultEntryId: record.game.defaultEntry ?? null,
    website: record.entries.find((entry) => entry.launch?.type === "web")?.launch?.url ?? null,
    hero: cover(hero, 960, assetUrl),
    screenshots: record.artwork
      .filter((art) => art.kind === "screenshot")
      .flatMap((art) => {
        const thumb = cover(art, 320, assetUrl);
        if (!thumb) return [];
        const original = originalPath(art);
        const full = original ? assetUrl(original) : (cover(art, 960, assetUrl) ?? thumb);
        return [{ thumb, full }];
      }),
    sizeBytes: record.entries.reduce<number | null>(
      (max, entry) => (entry.size === null ? max : Math.max(max ?? 0, entry.size)),
      null,
    ),
    entries: record.entries.map((entry) => ({
      id: entry.id,
      source: entry.sourceId,
      state: entry.state.type,
      location:
        entry.location?.type === "external"
          ? entry.location.path
          : entry.location?.type === "managed"
            ? `Storage root ${entry.location.rootId} · ${entry.location.relDir}`
            : null,
      canLaunch:
        entry.state.type === "installed" && entry.launch !== null && entry.launch.type !== "plugin",
      emulated: entry.launch?.type === "emulator",
    })),
  };
}
export function mapSession(record: {
  id: string;
  gameId: string;
  entryId: string;
  startedAtMs: number;
  endedAtMs: number | null;
  durationMs: number;
}): PlaySession {
  return {
    id: record.id,
    gameId: record.gameId,
    entryId: record.entryId,
    startedAt: date(record.startedAtMs) ?? "invalid",
    endedAt: record.endedAtMs === null ? null : (date(record.endedAtMs) ?? ""),
    durationSeconds: record.durationMs / 1000,
  };
}
export function mapPlatform(record: {
  name: string;
  games: number;
  playtimeMs: number;
  lastPlayedAtMs: number | null;
}): PlatformSummary {
  return {
    name: record.name,
    games: record.games,
    playtimeSeconds: record.playtimeMs / 1000,
    lastPlayedAt: date(record.lastPlayedAtMs),
  };
}

type ListGamesRecord = {
  query?: string | null;
  filter?: LibraryQuery["filter"];
  platform?: string | null;
  sort?: LibraryQuery["sort"];
};
type CollectionKindRecord = { type: "manual" } | { type: "smart"; query: ListGamesRecord };
export function mapCollectionKind(kind: CollectionKindRecord): CollectionKind {
  if (kind.type === "manual") return kind;
  const { query, filter, platform, sort } = kind.query;
  return {
    type: "smart",
    query: {
      filter: filter ?? "all",
      sort: sort ?? "title",
      ...(query && { query }),
      ...(platform && { platform }),
    },
  };
}
/** The core's form of a collection's kind; a smart query keeps no page. */
export function collectionKindRecord(kind: CollectionKind) {
  if (kind.type === "manual") return kind;
  const { filter, sort, query, platform } = kind.query;
  return {
    type: "smart" as const,
    query: {
      filter,
      sort,
      query: query?.trim() || null,
      platform: platform || null,
      offset: 0,
      limit: 50,
    },
  };
}
export function mapCollection(
  record: {
    id: string;
    name: string;
    description: string;
    pinned: boolean;
    kind: CollectionKindRecord;
    updatedAtMs: number;
    games: number;
    playtimeMs: number;
    covers: ArtworkRecord[];
  },
  assetUrl: (path: string) => string,
): CollectionSummary {
  return {
    id: record.id,
    name: record.name,
    description: record.description,
    pinned: record.pinned,
    kind: mapCollectionKind(record.kind),
    // Collections from before the time was kept say 0.
    updatedAt: record.updatedAtMs > 0 ? date(record.updatedAtMs) : null,
    games: record.games,
    playtimeSeconds: record.playtimeMs / 1000,
    covers: record.covers.flatMap((art) => cover(art, 320, assetUrl) ?? []),
  };
}
