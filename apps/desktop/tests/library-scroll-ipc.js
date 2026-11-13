// Original synthetic DTOs at the native IPC boundary. Loaded only by the CDP
// benchmark, never imported by the app or included in its production bundle.
(() => {
  const games = Array.from({ length: 1203 }, (_, index) => ({
    id: `fixture-${index}`,
    title: `Fixture Game ${String(index + 1).padStart(4, "0")}`,
    sortTitle: `Fixture Game ${String(index + 1).padStart(4, "0")}`,
    description: "Original benchmark fixture; no personal library data.",
    genres: [index % 2 ? "Adventure" : "Strategy"],
    tags: ["Fixture"],
    platforms: ["PC"],
    favorite: index % 3 === 0,
    hidden: false,
    rating: null,
    cover: null,
    installed: index % 5 !== 0,
    playtimeMs: index * 1200000,
    lastPlayedAtMs: null,
  }));
  const callbacks = new Map();
  let id = 0;
  globalThis.isTauri = true;
  globalThis.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  globalThis.__TAURI_INTERNALS__ = {
    transformCallback(callback) {
      callbacks.set(++id, callback);
      return id;
    },
    unregisterCallback(callbackId) {
      callbacks.delete(callbackId);
    },
    convertFileSrc(path, protocol) {
      return `${protocol}://localhost/${path}`;
    },
    async invoke(command, args = {}) {
      if (command === "list_games") {
        const query = args.query;
        let rows = games.filter(
          (game) =>
            query.filter === "all" ||
            (query.filter === "installed" && game.installed) ||
            (query.filter === "favorites" && game.favorite),
        );
        if (query.query)
          rows = rows.filter((game) =>
            game.title.toLowerCase().includes(query.query.toLowerCase()),
          );
        return { total: rows.length, items: rows.slice(query.offset, query.offset + query.limit) };
      }
      if (command === "library_stats")
        return {
          total: games.length,
          installed: games.filter((game) => game.installed).length,
          favorites: games.filter((game) => game.favorite).length,
          recent: 0,
          emulated: 0,
          playtimeMs: games.reduce((sum, game) => sum + game.playtimeMs, 0),
        };
      if (
        [
          "storage_list",
          "list_sessions",
          "jobs_list",
          "pending_install_requests",
          "running_games",
        ].includes(command)
      )
        return [];
      if (command === "plugin:event|listen") return ++id;
      if (command === "plugin:event|unlisten") return null;
      throw new Error(`Unexpected benchmark command: ${command}`);
    },
  };
})();
