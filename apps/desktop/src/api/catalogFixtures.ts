/** Explicit dev and test data only (`VITE_CASKADE_FIXTURES=1`, Vitest,
 * Storybook): a small synthetic IGDB catalog. Titles and links are made up. */
import type {
  CatalogApi,
  CatalogPage,
  CatalogReleases,
  CatalogSearch,
  IgdbGame,
  IgdbOwned,
  IgdbReleaseDate,
} from "./igdb";
import { foldSearch } from "./presentation";

const PC = { id: "6", name: "PC (Microsoft Windows)", abbreviation: "PC" };
const SWITCH2 = { id: "508", name: "Nintendo Switch 2", abbreviation: "Switch 2" };
const PS5 = { id: "167", name: "PlayStation 5", abbreviation: "PS5" };
export const fixturePlatforms = [PC, PS5, SWITCH2];
export const fixtureGenres = [
  { id: "12", name: "Role-playing (RPG)", abbreviation: null },
  { id: "31", name: "Adventure", abbreviation: null },
  { id: "9", name: "Puzzle", abbreviation: null },
];

/** A release; dated to the day unless `precision` says otherwise. */
export function release(
  date: string | null,
  extra: Partial<IgdbReleaseDate> = {},
): IgdbReleaseDate {
  const [year, month] = date ? date.split("-").map(Number) : [];
  return {
    platform: PC,
    region: "worldwide",
    date,
    precision: date ? "day" : "tbd",
    year: year ?? null,
    month: month ?? null,
    quarter: null,
    human: date ?? "TBD",
    status: null,
    ...extra,
  };
}

/** A catalog game with sensible empties; `extra` fills in the rest. */
export function catalogGame(id: string, title: string, extra: Partial<IgdbGame> = {}): IgdbGame {
  return {
    id,
    slug: title.toLowerCase().replaceAll(/[^a-z0-9]+/g, "-"),
    url: `https://www.igdb.com/games/${id}`,
    title,
    description: `${title} is a synthetic catalog game.`,
    storyline: null,
    genres: ["Adventure"],
    tags: [],
    platforms: [PC],
    releaseDate: null,
    aliases: [],
    totalRating: null,
    externalIds: [],
    gameType: 0,
    versionParent: null,
    images: { cover: null, hero: null, logo: null, artworks: [], screenshots: [] },
    ageRatings: [],
    releaseDates: [],
    storeLinks: [],
    websites: [],
    similarGames: [],
    status: null,
    videos: [],
    ...extra,
  };
}

const lantern = catalogGame("1001", "Lantern Tide", {
  genres: ["Role-playing (RPG)", "Adventure"],
  platforms: [PC, PS5],
  releaseDate: "2024-03-14",
  totalRating: 88,
  images: {
    cover: null,
    hero: null,
    logo: null,
    artworks: [],
    screenshots: ["sc1001a", "sc1001b"].map((imageId) => ({
      imageId,
      width: 1920,
      height: 1080,
      alphaChannel: null,
      imageType: null,
      url: `https://images.igdb.com/igdb/image/upload/t_screenshot_huge/${imageId}.jpg`,
    })),
  },
  videos: [
    { videoId: "lanternTr01", name: "Launch Trailer" },
    { videoId: "lanternGp02", name: null },
  ],
  ageRatings: [
    { board: "pegi", rating: "16" },
    { board: "esrb", rating: "T" },
  ],
  releaseDates: [
    release("2024-03-14"),
    release("2024-03-14", { platform: PS5, region: "europe" }),
    release("2027-01-01", {
      platform: SWITCH2,
      precision: "quarter",
      quarter: 1,
      human: "Q1 2027",
    }),
  ],
  storeLinks: [
    {
      store: "steam",
      name: "Steam",
      url: "https://store.steampowered.com/app/1001",
      platforms: ["PC (Microsoft Windows)"],
      countries: [],
    },
    {
      store: "playstation",
      name: "PlayStation Store",
      url: "https://store.playstation.com/product/1001",
      platforms: ["PlayStation 5"],
      countries: [826],
    },
  ],
  websites: [
    { kind: "official", name: "Official Website", url: "https://lantern.example" },
    { kind: "wiki", name: "Fandom", url: "https://lantern.fandom.example" },
  ],
  similarGames: ["1002", "1003", "1001", "9999"],
});

/** Every game the fixture catalog knows, most-rated first. */
export const fixtureCatalog: IgdbGame[] = [
  lantern,
  catalogGame("1002", "Copper Orchard", {
    genres: ["Puzzle"],
    releaseDate: "2026-09-30",
    releaseDates: [release("2026-09-30")],
    storeLinks: [
      {
        store: "gog",
        name: "GOG",
        url: "https://www.gog.example/game/copper",
        platforms: [],
        countries: [],
      },
    ],
  }),
  catalogGame("1003", "Starfall Ledger", {
    genres: ["Role-playing (RPG)"],
    platforms: [SWITCH2],
    releaseDates: [
      release("2027-06-01", { platform: SWITCH2, precision: "month", month: 6, human: "Jun 2027" }),
    ],
    status: "Alpha",
  }),
  catalogGame("1004", "Quiet Harbour", {
    platforms: [PC, SWITCH2],
    releaseDates: [release(null, { platform: SWITCH2 })],
  }),
  catalogGame("1005", "Ashen Relay", {
    releaseDates: [release("2026-12-01", { precision: "year", year: 2026, human: "2026" })],
    status: "Cancelled",
  }),
  ...Array.from({ length: 40 }, (_, i) =>
    catalogGame(String(2000 + i), `Fixture Game ${i + 1}`, {
      genres: [i % 2 ? "Puzzle" : "Adventure"],
      platforms: i % 3 ? [PC] : [PS5],
      // Within the year before 2026-10-30, the Store tests' today.
      releaseDate: `${i % 12 < 2 ? 2025 : 2026}-${String(((i + 10) % 12) + 1).padStart(2, "0")}-15`,
    }),
  ),
];

/** Upcoming releases, soonest first; the TBD ones apart. */
const upcomingReleases = [
  {
    game: "1005",
    release: release("2026-12-01", {
      precision: "year",
      year: 2026,
      human: "2026",
      status: "Cancelled",
    }),
  },
  {
    game: "1001",
    release: release("2027-01-01", {
      platform: SWITCH2,
      precision: "quarter",
      quarter: 1,
      human: "Q1 2027",
    }),
  },
  {
    game: "1003",
    release: release("2027-06-01", {
      platform: SWITCH2,
      precision: "month",
      month: 6,
      human: "Jun 2027",
    }),
  },
];

const recentReleases = [{ game: "1002", release: release("2026-09-30") }];

export interface CatalogFixtureOptions {
  /** IGDB ids in the library, and whether installed. */
  owned?: IgdbOwned[];
  /** Visits shares by IGDB id; missing games sort last, by title. */
  popularity?: Record<string, number>;
  /** Every call fails, as when the server can't be reached. */
  offline?: boolean;
  /** Pages answer as the last copy. */
  stale?: boolean;
  /** The server has no sorts yet (#253): searches answer in its usual
   * order, flagged. */
  sortsUnavailable?: boolean;
  /** Each call waits this long (ms). */
  delay?: number;
}

export function createCatalogFixtureApi(options: CatalogFixtureOptions = {}) {
  const calls: string[] = [];
  /** Every search request, in order. */
  const searches: CatalogSearch[] = [];
  const wait = async (call: string) => {
    calls.push(call);
    if (options.delay) await new Promise((resolve) => setTimeout(resolve, options.delay));
    if (options.offline) throw new Error("can't reach the metadata server");
  };
  const byId = new Map(fixtureCatalog.map((game) => [game.id, game]));
  const page = (games: IgdbGame[], offset: number, limit: number): CatalogPage => ({
    games: games.slice(offset, offset + limit),
    releases: [],
    nextOffset: offset + limit < games.length ? offset + limit : null,
    stale: options.stale ?? false,
    sortUnavailable: false,
  });
  const releases = (
    listed: { game: string; release: IgdbReleaseDate }[],
    request: CatalogReleases,
  ): CatalogPage => {
    const platform = request.platforms?.[0];
    const kept = listed.filter((entry) => !platform || entry.release.platform?.id === platform);
    const offset = request.offset ?? 0;
    const limit = request.limit || 20;
    const slice = kept.slice(offset, offset + limit);
    return {
      games: slice.flatMap((entry) => byId.get(entry.game) ?? []),
      releases: slice,
      nextOffset: offset + limit < kept.length ? offset + limit : null,
      stale: options.stale ?? false,
      sortUnavailable: false,
    };
  };
  const api: CatalogApi = {
    async search(search: CatalogSearch) {
      searches.push(search);
      await wait(`search:${search.query ?? ""}:${search.offset ?? 0}`);
      const query = foldSearch(search.query?.trim() ?? "");
      const platform = search.platforms?.[0];
      const genre = fixtureGenres.find((g) => g.id === search.genres?.[0])?.name;
      const from = search.releasedFrom;
      const to = search.releasedTo;
      const found = fixtureCatalog.filter(
        (game) =>
          foldSearch(game.title).includes(query) &&
          (!platform || game.platforms.some((p) => p.id === platform)) &&
          (!genre || game.genres.includes(genre)) &&
          (!from || (game.releaseDate !== null && game.releaseDate >= from)) &&
          (!to || (game.releaseDate !== null && game.releaseDate <= to)),
      );
      const sorted = search.sort && search.sort !== "relevance";
      if (sorted && options.sortsUnavailable) {
        return {
          ...page(found, search.offset ?? 0, search.limit || 20),
          sortUnavailable: true,
        };
      }
      if (search.sort === "popular") {
        const scores = options.popularity ?? { "1002": 0.006, "1001": 0.003 };
        found.sort((a, b) => {
          const left = scores[a.id];
          const right = scores[b.id];
          return (
            Number(right !== undefined) - Number(left !== undefined) ||
            (right ?? 0) - (left ?? 0) ||
            a.title.localeCompare(b.title)
          );
        });
      } else if (search.sort === "newest") {
        found.sort(
          (a, b) =>
            (b.releaseDate ?? "").localeCompare(a.releaseDate ?? "") ||
            a.title.localeCompare(b.title),
        );
      }
      return page(found, search.offset ?? 0, search.limit || 20);
    },
    async recent(request) {
      await wait("recent");
      return releases(recentReleases, request);
    },
    async upcoming(request) {
      await wait(request.tbd ? "tbd" : "upcoming");
      const tbd = [{ game: "1004", release: release(null, { platform: SWITCH2 }) }];
      return releases(request.tbd ? tbd : upcomingReleases, request);
    },
    async filters() {
      await wait("filters");
      return { platforms: fixturePlatforms, genres: fixtureGenres, stale: false };
    },
    async games(ids) {
      await wait(`games:${ids.join(",")}`);
      return {
        games: ids.flatMap((id) => byId.get(id) ?? []),
        notFound: ids.filter((id) => !byId.has(id)),
        pending: [],
      };
    },
    async owned(ids) {
      calls.push(`owned:${ids.join(",")}`);
      return (options.owned ?? []).filter((owned) => ids.includes(owned.igdbId));
    },
  };
  return Object.assign(api, { calls, searches });
}
