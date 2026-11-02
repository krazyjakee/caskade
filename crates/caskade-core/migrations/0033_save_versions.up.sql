-- Save versions (#239): a game's saves after each session, content-addressed.
-- The id is the same on every device; game_id is this device's game. No
-- foreign key on the game: versions outlive a game's removal, and merges
-- move them.
CREATE TABLE save_versions (
    id TEXT PRIMARY KEY NOT NULL,
    game_id TEXT NOT NULL,
    -- The account that played; '' when none was signed in.
    account_id TEXT NOT NULL DEFAULT '',
    -- The device that made it (its sync endpoint id, or 'local').
    device_id TEXT NOT NULL,
    session_id TEXT,
    created_at_ms INTEGER NOT NULL,
    -- 'sha256:…' of the blob listing its files, each by place and hash:
    -- equal saves have equal hashes.
    content_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0),
    file_count INTEGER NOT NULL CHECK(file_count >= 0),
    parents_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(parents_json)),
    reason TEXT NOT NULL,
    -- Made here (1), or received from another device (0).
    local INTEGER NOT NULL
);
CREATE INDEX save_versions_game ON save_versions(game_id, account_id, created_at_ms DESC);

-- The version each game's save files here match, per account, and the
-- files' hash as they were then (restores leave files a version doesn't list).
CREATE TABLE save_heads (
    game_id TEXT NOT NULL,
    account_id TEXT NOT NULL DEFAULT '',
    version_id TEXT NOT NULL,
    disk_hash TEXT NOT NULL,
    PRIMARY KEY (game_id, account_id)
);
