import { isTauri } from "@tauri-apps/api/core";
import { nativeAccounts } from "./accountsNative";
import { nativeActions } from "./actions";
import { assetUrl } from "./artwork";
import { commands, events } from "./bindings";
import { nativeGameEdits } from "./gameEdit";
import { nativeManualGames } from "./manualGames";
import {
  collectionKindRecord,
  mapCollection,
  mapDetails,
  mapPlatform,
  mapSession,
  mapSummary,
} from "./mapping";
import { unwrap } from "./reply";
import { nativeScripts } from "./scripts";
import type { CollectionsApi, LibraryApi } from "./types";

function native() {
  if (!isTauri()) throw new Error("Open Caskade desktop to access your library.");
}
/** The core's page limit; Settings lists this many hidden games. */
const HIDDEN_LIMIT = 1000;

/** Every native operation is a generated typed app-level command/event. */
export function createNativeApi(): LibraryApi {
  return {
    collections: nativeCollections,
    accounts: nativeAccounts,
    edits: nativeGameEdits,
    manual: nativeManualGames,
    actions: nativeActions,
    scripts: nativeScripts,
    async listGames(input) {
      native();
      const page = await unwrap(
        commands.listGames({
          ...input,
          query: input.query ?? null,
          platform: input.platform ?? null,
        }),
      );
      return {
        total: page.total,
        items: page.items.map((game) => {
          const summary = mapSummary(game, assetUrl);
          return input.filter === "emulated"
            ? { ...summary, status: "emulated" as const }
            : summary;
        }),
      };
    },
    async getGame(id) {
      native();
      const game = await unwrap(commands.getGame(id));
      return game ? mapDetails(game, assetUrl) : null;
    },
    async pageImages(gameId) {
      native();
      await unwrap(commands.gamePageImages(gameId));
    },
    async stopGame(sessionId) {
      native();
      await unwrap(commands.guideStopGame(sessionId));
    },
    async libraryPlatforms(filter) {
      native();
      return (await unwrap(commands.libraryPlatforms(filter))).map(mapPlatform);
    },
    async titleLetters(input) {
      native();
      return unwrap(
        commands.titleLetters({
          ...input,
          query: input.query ?? null,
          platform: input.platform ?? null,
        }),
      );
    },
    async setHidden(gameId, hidden) {
      native();
      await unwrap(commands.setGameHidden(gameId, hidden));
    },
    async setFavorite(gameId, favorite) {
      native();
      await unwrap(commands.setGameFavorite(gameId, favorite));
    },
    async setDefaultEntry(gameId, entryId) {
      native();
      await unwrap(commands.setGameDefaultEntry(gameId, entryId));
    },
    async revealEntry(entryId) {
      native();
      await unwrap(commands.revealGameEntry(entryId));
    },
    async hiddenGames() {
      native();
      const page = await unwrap(commands.hiddenGames(0, HIDDEN_LIMIT));
      return page.items.map((game) => mapSummary(game, assetUrl));
    },
    async libraryStats() {
      native();
      const stats = await unwrap(commands.libraryStats());
      return {
        total: stats.total,
        installed: stats.installed,
        favorites: stats.favorites,
        recent: stats.recent,
        emulated: stats.emulated,
        web: stats.web,
        owned: stats.owned,
        playtimeSeconds: stats.playtimeMs / 1000,
      };
    },
    async listSessions(gameId) {
      native();
      return (await unwrap(commands.listSessions(gameId ?? null, 1000))).map(mapSession);
    },
    async runningCount() {
      native();
      return (await unwrap(commands.runningGames())).length;
    },
    async addWebGame(url) {
      native();
      return unwrap(commands.addWebGame(url));
    },
    async saveWebGameOffline(gameId, rootId) {
      native();
      await unwrap(commands.saveWebGameOffline(gameId, rootId));
    },
    async removeWebGame(gameId) {
      native();
      await unwrap(commands.removeWebGame(gameId));
    },
    async removeGame(gameId) {
      native();
      return (await unwrap(commands.removeGame(gameId))) === null;
    },
    async removeWebOffline(gameId) {
      native();
      await unwrap(commands.removeWebOffline(gameId));
    },
    async launchGame(entryId, choice) {
      native();
      return unwrap(
        commands.launchGameWith(
          entryId,
          choice
            ? {
                profile: choice.profile ?? null,
                disc: choice.disc ?? null,
                action: choice.action ?? null,
              }
            : null,
        ),
      );
    },
    async ageRestriction(gameId) {
      native();
      return unwrap(commands.gameAgeCheck(gameId));
    },
    async approvePlay(gameId, accountId, pin, permanent) {
      native();
      return unwrap(commands.gameAgeApprove(gameId, accountId, pin, permanent));
    },
    async subscribe(listener) {
      if (!isTauri()) return () => {};
      const results = await Promise.allSettled([
        events.libraryChanged.listen(() => listener({ type: "library.changed" })),
        events.accountChanged.listen(() => listener({ type: "account.changed" })),
        events.gameStarted.listen(({ payload }) =>
          listener({
            type: "game.started",
            gameId: payload.gameId,
            entryId: payload.entryId,
            sessionId: payload.sessionId,
          }),
        ),
        events.gameExited.listen(({ payload }) =>
          listener({
            type: "game.exited",
            gameId: payload.gameId,
            entryId: payload.entryId,
            sessionId: payload.sessionId,
          }),
        ),
      ]);
      const cleanup = () => {
        for (const result of results) if (result.status === "fulfilled") result.value();
      };
      if (results.some((result) => result.status === "rejected")) {
        cleanup();
        throw new Error("Could not subscribe to library updates");
      }
      return cleanup;
    },
  };
}

const nativeCollections: CollectionsApi = {
  async list() {
    native();
    return (await unwrap(commands.listCollections())).map((record) =>
      mapCollection(record, assetUrl),
    );
  },
  async memberIds(id) {
    native();
    return (await unwrap(commands.getCollection(id)))?.gameIds ?? [];
  },
  async games(id, offset, limit) {
    native();
    const page = await unwrap(commands.collectionGames(id, offset, limit));
    return { total: page.total, items: page.items.map((game) => mapSummary(game, assetUrl)) };
  },
  async save(input) {
    native();
    const saved = await unwrap(
      commands.saveCollection({
        id: input.id ?? null,
        name: input.name,
        description: input.description,
        kind: collectionKindRecord(input.kind),
      }),
    );
    return saved.id;
  },
  async setPinned(id, pinned) {
    native();
    await unwrap(commands.setCollectionPinned(id, pinned));
  },
  async delete(id) {
    native();
    await unwrap(commands.deleteCollection(id));
  },
  async addGames(id, gameIds) {
    native();
    await unwrap(commands.addToCollection(id, gameIds));
  },
  async removeGames(id, gameIds) {
    native();
    await unwrap(commands.removeFromCollection(id, gameIds));
  },
  async ofGame(gameId) {
    native();
    return unwrap(commands.gameCollections(gameId));
  },
};
