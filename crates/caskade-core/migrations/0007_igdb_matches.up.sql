-- The IGDB game each library game is matched to (crate::metadata): how it
-- was found, and what the last apply wrote into the game, so the next one
-- replaces only that. One row per game that matching has looked at.
CREATE TABLE igdb_matches (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    -- NULL when there is none (source 'none', or 'manual' for "no game").
    igdb_id INTEGER CHECK(igdb_id IS NULL OR igdb_id > 0),
    source TEXT NOT NULL CHECK(source IN ('romm','hasheous','store','manual','none')),
    -- 1 until the IGDB game is applied to the library game.
    pending INTEGER NOT NULL CHECK(pending IN (0,1)),
    checked_at_ms INTEGER NOT NULL,
    -- The Hasheous dumps a 'none' answer was given against.
    dumps TEXT NOT NULL,
    applied_json TEXT NOT NULL CHECK(json_valid(applied_json))
);
CREATE INDEX igdb_matches_retry ON igdb_matches(source, pending);
