-- older Caskade: incompatible
-- Older binaries cannot decode the new 'title' match source.
CREATE TABLE igdb_matches_new (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    igdb_id INTEGER CHECK(igdb_id IS NULL OR igdb_id > 0),
    source TEXT NOT NULL CHECK(source IN ('romm','hasheous','store','title','device','manual','none')),
    pending INTEGER NOT NULL CHECK(pending IN (0,1)),
    checked_at_ms INTEGER NOT NULL,
    dumps TEXT NOT NULL,
    applied_json TEXT NOT NULL CHECK(json_valid(applied_json))
);
INSERT INTO igdb_matches_new(game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json)
SELECT game_id,igdb_id,source,pending,checked_at_ms,dumps,applied_json FROM igdb_matches;
DROP TABLE igdb_matches;
ALTER TABLE igdb_matches_new RENAME TO igdb_matches;
CREATE INDEX igdb_matches_retry ON igdb_matches(source, pending);
-- Try existing unmatched web games on the next metadata run.
UPDATE igdb_matches SET checked_at_ms=0
WHERE source='none' AND game_id IN (
    SELECT game_id FROM game_entries WHERE source_id='web' OR launch_kind='web'
);
