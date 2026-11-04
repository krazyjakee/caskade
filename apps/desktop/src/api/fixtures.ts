/** Explicit dev/test data only; never used as a native IPC fallback. */
import { createAccountsFixtureApi, defaultFixtureAccounts } from "./accountsFixtures";
import { createFixtureActions } from "./actionsFixtures";
import { createFixtureGameEdits } from "./gameEditFixtures";
import { createFixtureManualGames } from "./manualGamesFixtures";
import { compareTitles, foldSearch, titleLetter } from "./presentation";
import { createFixtureSaves } from "./savesFixtures";
import { createFixtureScripts } from "./scriptsFixtures";
import type {
  CollectionKind,
  CollectionsApi,
  GameDetails,
  HostBusy,
  LaunchPrompt,
  LibraryApi,
  LibraryEvent,
  LibraryFilter,
  LibraryQuery,
  PlatformSummary,
  PlaySession,
  TitleLetter,
} from "./types";

export function createFixtureGames(count = 24): GameDetails[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `fixture-${index}`,
    title: `Fixture Game ${String(index + 1).padStart(4, "0")}`,
    genre: index % 2 ? "Adventure" : "Strategy",
    platform: "PC",
    status: index % 5 === 0 ? "not-installed" : "installed",
    favorite: index % 3 === 0,
    playtimeSeconds: index * 1200,
    lastPlayedAt: null,
    sizeBytes: 1024 ** 3,
    coverGrid: null,
    coverList: null,
    icon: null,
    hero: null,
    description: "Original development fixture. No personal library data.",
    tags: ["Fixture"],
    releaseDate: index % 4 === 0 ? null : "2024-02-29",
    screenshots: [],
    entries: [
      {
        id: `entry-${index}`,
        source: "fixture",
        state: index % 5 === 0 ? "not-installed" : "installed",
        location: "Development library",
        canLaunch: index % 5 !== 0,
        emulated: false,
      },
    ],
  }));
}
/** Emulated games on several consoles, for the Emulated filter's platforms. */
export function createEmulatedFixtureGames(): GameDetails[] {
  const [template] = createFixtureGames(1);
  if (!template) return [];
  const platforms: [string, number, number | null][] = [
    ["Super Nintendo Entertainment System", 6, 2],
    ["Nintendo 64", 4, 9],
    ["Sony PlayStation", 5, 30],
    ["Sega Genesis", 3, null],
    ["Nintendo Game Boy Advance", 4, 1],
    ["Sega Dreamcast", 2, null],
    ["Nintendo GameCube", 3, 60],
    ["Arcade", 2, null],
  ];
  return platforms.flatMap(([platform, count, daysAgo], p) =>
    Array.from({ length: count }, (_, index) => ({
      ...template,
      id: `fixture-emulated-${p}-${index}`,
      title: `${platform.split(" ").at(-1)} Fixture ${index + 1}`,
      platform,
      status: "emulated" as const,
      favorite: false,
      playtimeSeconds: daysAgo === null ? 0 : (index + 1) * 2700,
      lastPlayedAt:
        daysAgo === null || index > 0
          ? null
          : new Date(Date.now() - daysAgo * 86_400_000).toISOString(),
      entries: [
        {
          id: `fixture-emulated-entry-${p}-${index}`,
          source: "fixture",
          state: "installed",
          location: "Development library",
          canLaunch: true,
          emulated: true,
        },
      ],
    })),
  );
}
/** `prompts` makes an entry ask at launch until the choice answers it. */
export function createFixtureApi(
  games = createFixtureGames(),
  prompts: Record<string, LaunchPrompt> = {},
  popularity: Record<string, number> = {},
): LibraryApi & {
  emit(event: LibraryEvent): void;
  finishSession(id: string, durationSeconds: number): void;
  edits: ReturnType<typeof createFixtureGameEdits>;
  actions: ReturnType<typeof createFixtureActions>;
  scripts: ReturnType<typeof createFixtureScripts>;
  saves: ReturnType<typeof createFixtureSaves>;
  /** Streamed releases whose device plays another game (#264), by entry. */
  busyHosts: Map<string, Omit<HostBusy, "entryId">>;
} {
  const records = structuredClone(games);
  const busyHosts = new Map<string, Omit<HostBusy, "entryId">>();
  const hidden = new Set<string>();
  const visible = () => records.filter((g) => !hidden.has(g.id));
  const sessions: PlaySession[] = [];
  // Web games' pages, by game id.
  const pages = new Map<string, string>();
  const webEntry = (gameId: string) => {
    const entry = records
      .find((g) => g.id === gameId)
      ?.entries.find((candidate) => candidate.source === "web");
    if (!entry) throw new Error("That isn't a web game");
    return entry;
  };
  const listeners = new Set<(event: LibraryEvent) => void>();
  const emit = (event: LibraryEvent) => {
    for (const listener of listeners) listener(event);
  };
  const finish = (id: string, durationSeconds: number) => {
    const session = sessions.find((s) => s.id === id && !s.endedAt);
    if (!session) throw new Error("No active fixture session");
    session.endedAt = new Date().toISOString();
    session.durationSeconds = durationSeconds;
    const game = records.find((g) => g.id === session.gameId);
    if (game) game.playtimeSeconds += durationSeconds;
    emit({
      type: "game.exited",
      gameId: session.gameId,
      entryId: session.entryId,
      sessionId: id,
    });
  };
  // Web games alone show in the Web tab only, as in the core.
  const webOnly = (g: GameDetails) =>
    g.entries.length > 0 && g.entries.every((entry) => entry.source === "web");
  const inFilter = (g: GameDetails, filter: LibraryFilter) =>
    filter === "web"
      ? g.entries.some((entry) => entry.source === "web")
      : !webOnly(g) &&
        (filter === "all" ||
          (filter === "installed" && g.status === "installed") ||
          (filter === "owned" &&
            g.status === "not-installed" &&
            g.entries.some((entry) => entry.source.startsWith("store:"))) ||
          (filter === "recent" && g.lastPlayedAt !== null) ||
          (filter === "favorites" && g.favorite) ||
          (filter === "emulated" && g.status === "emulated"));
  const matching = (input: LibraryQuery) => {
    // Case and accents aside, as the core's search: "pokemon" finds "Pokémon".
    const query = foldSearch(input.query?.trim() ?? "");
    const filtered = visible().filter(
      (g) =>
        (!query || [g.title, g.genre, ...g.tags].some((s) => foldSearch(s).includes(query))) &&
        (!input.platform || g.platform.split(" · ").includes(input.platform)) &&
        inFilter(g, input.filter),
    );
    // The last record was added last.
    const added = (g: GameDetails) => records.indexOf(g);
    return filtered.sort((a, b) =>
      input.sort === "popular"
        ? Number(popularity[b.id] !== undefined) - Number(popularity[a.id] !== undefined) ||
          (popularity[b.id] ?? 0) - (popularity[a.id] ?? 0) ||
          compareTitles(a.title, b.title)
        : input.sort === "playtime"
          ? b.playtimeSeconds - a.playtimeSeconds
          : input.sort === "recent"
            ? (b.lastPlayedAt ?? "").localeCompare(a.lastPlayedAt ?? "") ||
              compareTitles(a.title, b.title)
            : input.sort === "added"
              ? added(b) - added(a)
              : input.sort === "released"
                ? (b.releaseDate ?? "").localeCompare(a.releaseDate ?? "") ||
                  compareTitles(a.title, b.title)
                : compareTitles(a.title, b.title),
    );
  };
  const collections = fixtureCollections(records, (query) => matching(query), emit);
  const actions = createFixtureActions(records);
  return {
    actions,
    busyHosts,
    saves: createFixtureSaves(),
    scripts: createFixtureScripts((id) => records.find((game) => game.id === id)?.title),
    collections,
    accounts: createAccountsFixtureApi(defaultFixtureAccounts, emit),
    edits: createFixtureGameEdits(records, emit, (query) => matching(query)),
    manual: createFixtureManualGames(records, emit),
    async listGames(input) {
      const filtered = matching(input);
      return structuredClone({
        total: filtered.length,
        items: filtered.slice(input.offset, input.offset + input.limit),
      });
    },
    async titleLetters(input) {
      const letters: TitleLetter[] = [];
      matching({ ...input, sort: "title" }).forEach((game, offset) => {
        const letter = titleLetter(game.title);
        if (!letters.some((seen) => seen.letter === letter)) letters.push({ letter, offset });
      });
      return letters;
    },
    async getGame(id) {
      return structuredClone(records.find((g) => g.id === id) ?? null);
    },
    async libraryPlatforms(filter) {
      const platforms = new Map<string, PlatformSummary>();
      for (const game of visible().filter((g) => inFilter(g, filter))) {
        for (const name of game.platform.split(" · ")) {
          const platform = platforms.get(name) ?? {
            name,
            games: 0,
            playtimeSeconds: 0,
            lastPlayedAt: null,
          };
          platform.games += 1;
          platform.playtimeSeconds += game.playtimeSeconds;
          if ((game.lastPlayedAt ?? "") > (platform.lastPlayedAt ?? ""))
            platform.lastPlayedAt = game.lastPlayedAt;
          platforms.set(name, platform);
        }
      }
      return [...platforms.values()].sort((a, b) => a.name.localeCompare(b.name));
    },
    async libraryStats() {
      const shown = visible();
      return {
        total: shown.filter((g) => inFilter(g, "all")).length,
        installed: shown.filter((g) => inFilter(g, "installed")).length,
        recent: shown.filter((g) => inFilter(g, "recent")).length,
        emulated: shown.filter((g) => inFilter(g, "emulated")).length,
        web: shown.filter((g) => inFilter(g, "web")).length,
        favorites: shown.filter((g) => inFilter(g, "favorites")).length,
        owned: shown.filter((g) => inFilter(g, "owned")).length,
        playtimeSeconds: shown.reduce((sum, g) => sum + g.playtimeSeconds, 0),
      };
    },
    async setHidden(gameId, hide) {
      if (!records.some((g) => g.id === gameId)) throw new Error("No such fixture game");
      if (hide) hidden.add(gameId);
      else hidden.delete(gameId);
      emit({ type: "library.changed" });
    },
    async addWebGame(url) {
      const page = new URL(url.includes("://") ? url : `https://${url}`);
      page.hash = "";
      const existing = [...pages].find(([, href]) => href === page.href);
      if (existing) return existing[0];
      const [template] = createFixtureGames(1);
      if (!template) throw new Error("No fixture template");
      const id = `fixture-web-${records.length}`;
      records.push({
        ...template,
        id,
        title: page.hostname.replace(/^www\./, ""),
        platform: "Browser (Flash/HTML5)",
        status: "installed",
        favorite: false,
        playtimeSeconds: 0,
        entries: [
          {
            id: `${id}-entry`,
            source: "web",
            state: "installed",
            location: null,
            canLaunch: true,
            emulated: false,
          },
        ],
      });
      pages.set(id, page.href);
      emit({ type: "library.changed" });
      return id;
    },
    async saveWebGameOffline(gameId, rootId) {
      const entry = webEntry(gameId);
      if (entry.location) throw new Error("This game is already saved for offline play");
      entry.location = `Storage root ${rootId} · ${records.find((g) => g.id === gameId)?.title}`;
      emit({ type: "library.changed" });
    },
    async removeWebGame(gameId) {
      const game = records.find((g) => g.id === gameId);
      const entry = webEntry(gameId);
      if (!game) throw new Error("No such fixture game");
      game.entries = game.entries.filter((candidate) => candidate !== entry);
      if (game.entries.length === 0) records.splice(records.indexOf(game), 1);
      pages.delete(gameId);
      emit({ type: "library.changed" });
    },
    async removeGame(gameId) {
      const game = records.find((g) => g.id === gameId);
      if (!game) throw new Error("No such fixture game");
      const added = ["caskade-storage", "caskade-handoff", "manual"];
      if (game.entries.some((entry) => !added.includes(entry.source)))
        throw new Error("a scan found this game and would bring it back; hide it instead");
      records.splice(records.indexOf(game), 1);
      emit({ type: "library.changed" });
      return true;
    },
    async removeWebOffline(gameId) {
      webEntry(gameId).location = null;
      emit({ type: "library.changed" });
    },
    async setFavorite(gameId, favorite) {
      const game = records.find((g) => g.id === gameId);
      if (!game) throw new Error("No such fixture game");
      game.favorite = favorite;
      emit({ type: "library.changed" });
    },
    async setDefaultEntry(gameId, entryId) {
      const game = records.find((g) => g.id === gameId);
      if (!game) throw new Error("No such fixture game");
      if (entryId !== null && !game.entries.some((entry) => entry.id === entryId))
        throw new Error("No such fixture entry");
      game.defaultEntryId = entryId;
      emit({ type: "library.changed" });
    },
    async revealEntry(entryId) {
      if (!records.some((g) => g.entries.some((entry) => entry.id === entryId)))
        throw new Error("No such fixture entry");
    },
    async entryPath(entryId) {
      const entry = records.flatMap((g) => g.entries).find((each) => each.id === entryId);
      if (!entry?.location) throw new Error("No such fixture entry");
      return entry.location;
    },
    async hiddenGames() {
      return structuredClone(
        records.filter((g) => hidden.has(g.id)).toSorted((a, b) => compareTitles(a.title, b.title)),
      );
    },
    async listSessions(id) {
      return structuredClone(
        sessions
          .filter((s) => !id || s.gameId === id)
          .toSorted((a, b) => b.startedAt.localeCompare(a.startedAt)),
      );
    },
    async launchGame(entryId, choice) {
      // A busy device asks before its game is stopped.
      const busy = busyHosts.get(entryId);
      if (busy && !choice?.stopHost) return { type: "host_busy", busy: { entryId, ...busy } };
      busyHosts.delete(entryId);
      // Ask-at-launch's action question comes before any other.
      const asks = actions.prompt(entryId, choice?.action);
      if (asks) return { type: "choose", prompt: asks };
      const prompt = prompts[entryId];
      if (prompt) {
        const asked = {
          ...prompt,
          profiles: choice?.profile ? [] : prompt.profiles,
          discs: choice?.disc != null ? [] : prompt.discs,
        };
        if (asked.profiles.length > 0 || asked.discs.length > 0)
          return { type: "choose", prompt: structuredClone(asked) };
      }
      const game = records.find((g) =>
        g.entries.some((entry) => entry.id === entryId && entry.canLaunch),
      );
      if (!game) throw new Error("This game entry cannot be launched");
      if (sessions.some((s) => s.gameId === game.id && s.endedAt === null))
        throw new Error("Game is already running");
      const session = {
        id: `session-${sessions.length}`,
        gameId: game.id,
        entryId,
        startedAt: new Date().toISOString(),
        endedAt: null,
        durationSeconds: 0,
      };
      sessions.push(session);
      game.lastPlayedAt = session.startedAt;
      emit({ type: "game.started", gameId: game.id, entryId, sessionId: session.id });
      return { type: "started", ticket: { gameId: game.id, entryId, sessionId: session.id } };
    },
    async stopGame(sessionId) {
      finish(sessionId, 60);
    },
    async subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    emit,
    finishSession: finish,
  };
}

interface FixtureCollection {
  id: string;
  name: string;
  description: string;
  pinned: boolean;
  kind: CollectionKind;
  gameIds: string[];
  updatedAt: string;
}
/** In-memory collections over the fixture games: a pinned smart one for the
 * favourites and a short manual list. */
function fixtureCollections(
  records: GameDetails[],
  matching: (query: LibraryQuery) => GameDetails[],
  emit: (event: LibraryEvent) => void,
): CollectionsApi {
  const now = new Date().toISOString();
  const stored: FixtureCollection[] = [
    {
      id: "collection-favourites",
      name: "Favourites",
      description: "The ones worth coming back to.",
      pinned: true,
      kind: { type: "smart", query: { filter: "favorites", sort: "title" } },
      gameIds: [],
      updatedAt: now,
    },
    {
      id: "collection-weekend",
      name: "Weekend Picks",
      description: "Short list for when there's finally time.",
      pinned: false,
      kind: { type: "manual" },
      gameIds: records.slice(1, 4).map((game) => game.id),
      updatedAt: now,
    },
  ];
  let next = 0;
  const find = (id: string) => {
    const collection = stored.find((c) => c.id === id);
    if (!collection) throw new Error(`collection ${id} was not found`);
    return collection;
  };
  const manual = (id: string) => {
    const collection = find(id);
    if (collection.kind.type === "smart")
      throw new Error("a smart collection chooses its own games");
    return collection;
  };
  const games = (collection: FixtureCollection) =>
    collection.kind.type === "smart"
      ? matching(collection.kind.query)
      : matching({ filter: "all", sort: "title" }).filter((game) =>
          collection.gameIds.includes(game.id),
        );
  const changed = (collection: FixtureCollection) => {
    collection.updatedAt = new Date().toISOString();
    emit({ type: "library.changed" });
  };
  return {
    async list() {
      return stored
        .toSorted((a, b) => Number(b.pinned) - Number(a.pinned) || a.name.localeCompare(b.name))
        .map((collection) => {
          const all = games(collection);
          return structuredClone({
            id: collection.id,
            name: collection.name,
            description: collection.description,
            pinned: collection.pinned,
            kind: collection.kind,
            updatedAt: collection.updatedAt,
            games: all.length,
            playtimeSeconds: all.reduce((sum, game) => sum + game.playtimeSeconds, 0),
            covers: all.slice(0, 4).flatMap((game) => game.coverGrid ?? []),
          });
        });
    },
    async memberIds(id) {
      return [...find(id).gameIds];
    },
    async games(id, offset, limit) {
      const all = games(find(id));
      return structuredClone({ total: all.length, items: all.slice(offset, offset + limit) });
    },
    async save(input) {
      if (!input.name.trim()) throw new Error("collection name is empty");
      const existing = input.id ? find(input.id) : undefined;
      const collection = existing ?? {
        id: `collection-${next++}`,
        name: "",
        description: "",
        pinned: false,
        kind: input.kind,
        gameIds: [],
        updatedAt: "",
      };
      collection.name = input.name;
      collection.description = input.description;
      collection.kind = structuredClone(input.kind);
      if (input.kind.type === "smart") collection.gameIds = [];
      if (!existing) stored.push(collection);
      changed(collection);
      return collection.id;
    },
    async setPinned(id, pinned) {
      find(id).pinned = pinned;
      emit({ type: "library.changed" });
    },
    async delete(id) {
      const index = stored.findIndex((c) => c.id === id);
      if (index >= 0) stored.splice(index, 1);
      emit({ type: "library.changed" });
    },
    async addGames(id, gameIds) {
      const collection = manual(id);
      for (const gameId of gameIds) {
        if (!records.some((game) => game.id === gameId)) throw new Error("No such fixture game");
        if (!collection.gameIds.includes(gameId)) collection.gameIds.push(gameId);
      }
      changed(collection);
    },
    async removeGames(id, gameIds) {
      const collection = manual(id);
      collection.gameIds = collection.gameIds.filter((gameId) => !gameIds.includes(gameId));
      changed(collection);
    },
    async ofGame(gameId) {
      return stored.filter((c) => c.gameIds.includes(gameId)).map((c) => c.id);
    },
  };
}
