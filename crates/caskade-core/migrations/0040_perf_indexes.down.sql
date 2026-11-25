DROP TRIGGER IF EXISTS games_search_update;
CREATE TRIGGER games_search_update AFTER UPDATE ON games BEGIN
    INSERT INTO game_search(game_search,rowid,title,sort_title,aliases_json,tags_json)
    VALUES('delete',old.rowid,old.title,old.sort_title,old.aliases_json,old.tags_json);
    INSERT INTO game_search(rowid,title,sort_title,aliases_json,tags_json)
    VALUES(new.rowid,new.title,new.sort_title,new.aliases_json,new.tags_json);
END;

DROP INDEX IF EXISTS sessions_entry;
DROP INDEX IF EXISTS artwork_user;
DROP INDEX IF EXISTS sync_changes_pending_clock;
DROP INDEX IF EXISTS sync_changes_identity;
DROP INDEX IF EXISTS igdb_matches_igdb_id;
DROP INDEX IF EXISTS entries_game_flags;
