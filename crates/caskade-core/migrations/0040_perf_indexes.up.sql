-- older Caskade: compatible
-- Indexes and trigger narrowing for large libraries and sync performance.

CREATE INDEX IF NOT EXISTS entries_game_flags
    ON game_entries(game_id, state_kind, launch_kind, source_id);

CREATE INDEX IF NOT EXISTS igdb_matches_igdb_id
    ON igdb_matches(igdb_id)
    WHERE igdb_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS sync_changes_identity
    ON sync_changes(origin, entity, key, field, wall DESC, counter DESC)
    WHERE entity = 'game' AND field = 'identity';

CREATE INDEX IF NOT EXISTS sync_changes_pending_clock
    ON sync_changes(applied, wall, counter, origin)
    WHERE applied = 0;

CREATE INDEX IF NOT EXISTS artwork_user
    ON artwork(id)
    WHERE record_json->>'$.source' = 'user';

CREATE INDEX IF NOT EXISTS sessions_entry
    ON play_sessions(entry_id);

DROP TRIGGER IF EXISTS games_search_update;
CREATE TRIGGER games_search_update
AFTER UPDATE OF title, sort_title, aliases_json, tags_json ON games
WHEN old.title IS NOT new.title
  OR old.sort_title IS NOT new.sort_title
  OR old.aliases_json IS NOT new.aliases_json
  OR old.tags_json IS NOT new.tags_json
BEGIN
    INSERT INTO game_search(game_search,rowid,title,sort_title,aliases_json,tags_json)
    VALUES('delete',old.rowid,old.title,old.sort_title,old.aliases_json,old.tags_json);
    INSERT INTO game_search(rowid,title,sort_title,aliases_json,tags_json)
    VALUES(new.rowid,new.title,new.sort_title,new.aliases_json,new.tags_json);
END;
