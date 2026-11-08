-- Back to the sources before 'device': those games are matched again.
CREATE TABLE igdb_matches_old (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    igdb_id INTEGER CHECK(igdb_id IS NULL OR igdb_id > 0),
    source TEXT NOT NULL CHECK(source IN ('romm','hasheous','store','manual','none')),
    pending INTEGER NOT NULL CHECK(pending IN (0,1)),
    checked_at_ms INTEGER NOT NULL,
    dumps TEXT NOT NULL,
    applied_json TEXT NOT NULL CHECK(json_valid(applied_json))
);
INSERT INTO igdb_matches_old(game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json)
SELECT game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json FROM igdb_matches
WHERE source<>'device';
DROP TABLE igdb_matches;
ALTER TABLE igdb_matches_old RENAME TO igdb_matches;
CREATE INDEX igdb_matches_retry ON igdb_matches(source, pending);
