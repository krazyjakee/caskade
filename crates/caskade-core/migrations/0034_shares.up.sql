-- Games and smaller things shared once with another person through a link
-- (#246, #248). A share's grant is a bearer credential: the table is local
-- (never synced) and left out of backups.
CREATE TABLE shares (
    id TEXT PRIMARY KEY NOT NULL,
    -- `game`, `save`, `web`, `rom` or `collection`.
    kind TEXT NOT NULL,
    game_id TEXT,
    entry_id TEXT,
    title TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    -- The collection's root hash: the game's folder, or the item.
    root TEXT,
    -- The extras' collection: setup, cover, the save.
    extras_root TEXT,
    -- 64 hex characters: what the link presents.
    grant TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    -- When the link stops working; NULL until it's stopped.
    expires_at_ms INTEGER,
    stopped_at_ms INTEGER,
    -- What comes with it, and its description (JSON).
    details_json TEXT NOT NULL DEFAULT '{}'
);

-- Who opened a share's link, and how far each got.
CREATE TABLE share_recipients (
    share_id TEXT NOT NULL REFERENCES shares(id) ON DELETE CASCADE,
    endpoint_id TEXT NOT NULL,
    name TEXT NOT NULL,
    first_seen_ms INTEGER NOT NULL,
    done_bytes INTEGER NOT NULL DEFAULT 0,
    finished_at_ms INTEGER,
    PRIMARY KEY (share_id, endpoint_id)
);
