-- A game that came from another device can be matched by the IGDB id that
-- device sent with it (source 'device'), when nothing here finds one: a ROM
-- matched by name or by RomM there has no store id to ask the metadata
-- server about here (#75). SQLite can't change a CHECK, so the table is
-- rebuilt; nothing references it and it has no triggers.
CREATE TABLE igdb_matches_new (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    -- NULL when there is none (source 'none', or 'manual' for "no game").
    igdb_id INTEGER CHECK(igdb_id IS NULL OR igdb_id > 0),
    source TEXT NOT NULL CHECK(source IN ('romm','hasheous','store','device','manual','none')),
    -- 1 until the IGDB game is applied to the library game.
    pending INTEGER NOT NULL CHECK(pending IN (0,1)),
    checked_at_ms INTEGER NOT NULL,
    -- The Hasheous dumps a 'none' answer was given against.
    dumps TEXT NOT NULL,
    applied_json TEXT NOT NULL CHECK(json_valid(applied_json))
);
INSERT INTO igdb_matches_new(game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json)
SELECT game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json FROM igdb_matches;
DROP TABLE igdb_matches;
ALTER TABLE igdb_matches_new RENAME TO igdb_matches;
CREATE INDEX igdb_matches_retry ON igdb_matches(source, pending);
-- Games from another device recorded as not on IGDB: look at them again,
-- now with their devices' IGDB ids.
UPDATE igdb_matches SET checked_at_ms=0
WHERE source='none'
  AND game_id IN (SELECT game_id FROM sync_game_ids WHERE method='created');
