-- Opaque IDs remain separate from searchable metadata. JSON retains the typed
-- domain payload; explicit columns support indexed queries and FK invariants.
CREATE TABLE games (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL CHECK(length(trim(title)) > 0),
    sort_title TEXT NOT NULL,
    aliases_json TEXT NOT NULL CHECK(json_valid(aliases_json)),
    tags_json TEXT NOT NULL CHECK(json_valid(tags_json)),
    favorite INTEGER NOT NULL CHECK(favorite IN (0,1)),
    hidden INTEGER NOT NULL CHECK(hidden IN (0,1)),
    record_json TEXT NOT NULL CHECK(json_valid(record_json))
);
CREATE INDEX games_order ON games(sort_title COLLATE NOCASE,id);

CREATE TABLE storage_roots (
    id TEXT PRIMARY KEY NOT NULL,
    path TEXT NOT NULL,
    is_default INTEGER NOT NULL CHECK(is_default IN (0,1)),
    record_json TEXT NOT NULL CHECK(json_valid(record_json))
);
CREATE UNIQUE INDEX one_default_root ON storage_roots(is_default) WHERE is_default=1;

CREATE TABLE game_entries (
    id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL,
    external_id TEXT NOT NULL,
    state_kind TEXT NOT NULL CHECK(state_kind IN ('installed','updating','partial','broken')),
    location_kind TEXT NOT NULL CHECK(location_kind IN ('none','managed','external')),
    root_id TEXT REFERENCES storage_roots(id) ON DELETE RESTRICT,
    rel_dir TEXT,
    external_path TEXT,
    launch_kind TEXT,
    size INTEGER CHECK(size IS NULL OR size>=0),
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    UNIQUE(source_id,external_id),
    UNIQUE(id,game_id),
    CHECK (
        (location_kind='none' AND root_id IS NULL AND rel_dir IS NULL AND external_path IS NULL)
        OR (location_kind='external' AND external_path IS NOT NULL AND root_id IS NULL AND rel_dir IS NULL)
        OR (location_kind='managed' AND root_id IS NOT NULL AND external_path IS NULL AND rel_dir IS NOT NULL
            AND length(rel_dir)>0 AND substr(rel_dir,1,1) NOT IN ('/','\')
            AND instr(rel_dir,':')=0 AND instr(rel_dir,'\')=0
            AND instr('/'||rel_dir||'/','/../')=0 AND rel_dir<>'.')
    )
);
CREATE INDEX entries_game ON game_entries(game_id);
CREATE INDEX entries_root ON game_entries(root_id);

CREATE TABLE play_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    entry_id TEXT NOT NULL,
    started_at_ms INTEGER NOT NULL,
    ended_at_ms INTEGER,
    duration_ms INTEGER NOT NULL CHECK(duration_ms>=0),
    heartbeat_at_ms INTEGER NOT NULL,
    tracking_json TEXT CHECK(tracking_json IS NULL OR json_valid(tracking_json)),
    FOREIGN KEY(entry_id,game_id) REFERENCES game_entries(id,game_id) ON UPDATE CASCADE ON DELETE CASCADE
);
CREATE UNIQUE INDEX one_open_session_per_game ON play_sessions(game_id) WHERE ended_at_ms IS NULL;
CREATE INDEX sessions_game_recent ON play_sessions(game_id,started_at_ms DESC,id);
CREATE INDEX sessions_recent ON play_sessions(started_at_ms DESC,id);

CREATE TABLE artwork (
    id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('cover','hero','logo','icon','screenshot')),
    content_hash TEXT NOT NULL,
    record_json TEXT NOT NULL CHECK(json_valid(record_json))
);
CREATE INDEX artwork_game_kind ON artwork(game_id,kind,id);
CREATE INDEX artwork_content_hash ON artwork(content_hash);

CREATE TABLE collections (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('manual','smart')),
    query_json TEXT CHECK(query_json IS NULL OR json_valid(query_json)),
    CHECK((kind='manual' AND query_json IS NULL) OR (kind='smart' AND query_json IS NOT NULL))
);
CREATE TABLE collection_members (
    collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position>=0),
    PRIMARY KEY(collection_id,game_id)
);
CREATE INDEX collection_members_game ON collection_members(game_id);
CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL CHECK(json_valid(value_json))
);

-- External-content index maintained exclusively by triggers, including direct
-- SQL updates and FK cascades during repository transactions.
CREATE VIRTUAL TABLE game_search USING fts5(
    title,sort_title,aliases_json,tags_json,
    content='games',content_rowid='rowid',tokenize='trigram remove_diacritics 1'
);
CREATE TRIGGER games_search_insert AFTER INSERT ON games BEGIN
    INSERT INTO game_search(rowid,title,sort_title,aliases_json,tags_json)
    VALUES(new.rowid,new.title,new.sort_title,new.aliases_json,new.tags_json);
END;
CREATE TRIGGER games_search_delete AFTER DELETE ON games BEGIN
    INSERT INTO game_search(game_search,rowid,title,sort_title,aliases_json,tags_json)
    VALUES('delete',old.rowid,old.title,old.sort_title,old.aliases_json,old.tags_json);
END;
CREATE TRIGGER games_search_update AFTER UPDATE ON games BEGIN
    INSERT INTO game_search(game_search,rowid,title,sort_title,aliases_json,tags_json)
    VALUES('delete',old.rowid,old.title,old.sort_title,old.aliases_json,old.tags_json);
    INSERT INTO game_search(rowid,title,sort_title,aliases_json,tags_json)
    VALUES(new.rowid,new.title,new.sort_title,new.aliases_json,new.tags_json);
END;
