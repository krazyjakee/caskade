-- SQLite can't drop a column with a reference, so collections are rebuilt
-- as 0013 left them; their members move to a table pointing at the new one
-- first, so dropping the old table cascades to nothing.
CREATE TABLE collections_v13 (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('manual','smart')),
    query_json TEXT CHECK(query_json IS NULL OR json_valid(query_json)),
    description TEXT NOT NULL DEFAULT '',
    pinned INTEGER NOT NULL DEFAULT 0,
    updated_at_ms INTEGER NOT NULL DEFAULT 0,
    CHECK((kind='manual' AND query_json IS NULL) OR (kind='smart' AND query_json IS NOT NULL))
);
INSERT INTO collections_v13(id,name,kind,query_json,description,pinned,updated_at_ms)
SELECT id,name,kind,query_json,description,pinned,updated_at_ms FROM collections;
CREATE TABLE collection_members_v13 (
    collection_id TEXT NOT NULL REFERENCES collections_v13(id) ON DELETE CASCADE,
    game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position>=0),
    PRIMARY KEY(collection_id,game_id)
);
INSERT INTO collection_members_v13(collection_id,game_id,position)
SELECT collection_id,game_id,position FROM collection_members;
DROP TABLE collection_members;
DROP TABLE collections;
-- Renaming also points collection_members_v13's reference at `collections`.
ALTER TABLE collections_v13 RENAME TO collections;
ALTER TABLE collection_members_v13 RENAME TO collection_members;
CREATE INDEX collection_members_game ON collection_members(game_id);
DROP TABLE accounts;
