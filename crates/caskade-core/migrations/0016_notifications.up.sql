-- Notifications (#198): the feed, its read state, and the journal of games
-- genuinely new to the library.

-- Every game insert writes its row in the same transaction, through the
-- trigger below; an upsert that updates an existing game doesn't fire it.
-- No foreign key: a game merged away or removed keeps its row, so the same
-- id is never counted twice. `processed` is 1 once the notifications service
-- has put it in a batch, or skipped it.
CREATE TABLE game_additions (
    game_id TEXT PRIMARY KEY NOT NULL,
    added_at_ms INTEGER NOT NULL,
    processed INTEGER NOT NULL DEFAULT 0 CHECK(processed IN (0,1))
);
CREATE INDEX game_additions_pending ON game_additions(added_at_ms) WHERE processed=0;
CREATE TRIGGER game_additions_insert AFTER INSERT ON games BEGIN
    INSERT OR IGNORE INTO game_additions(game_id,added_at_ms)
    VALUES (NEW.id, CAST((julianday('now')-2440587.5)*86400000 AS INTEGER));
END;

-- The feed's revision: every change worth reading takes the next value as
-- its notification's `version`. Never goes back, even when items are pruned.
CREATE TABLE notification_revision (
    id INTEGER PRIMARY KEY CHECK(id=1),
    value INTEGER NOT NULL
);
INSERT INTO notification_revision(id,value) VALUES (1,0);

-- One item of the feed. `group_key` is what later events update instead of
-- adding another item (`games.added:<batch start>:<id>`, `job.failed:<job>`, …).
-- It is unread while `read_version < version`: marking it read records the
-- version that was shown, so a change after that leaves it unread.
CREATE TABLE notifications (
    id TEXT PRIMARY KEY NOT NULL,
    category TEXT NOT NULL,
    severity TEXT NOT NULL CHECK(severity IN ('info','warning','error')),
    group_key TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    first_at_ms INTEGER NOT NULL,
    last_at_ms INTEGER NOT NULL,
    count INTEGER NOT NULL DEFAULT 0 CHECK(count>=0),
    sources_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(sources_json)),
    target_json TEXT NOT NULL CHECK(json_valid(target_json)),
    version INTEGER NOT NULL DEFAULT 0,
    read_version INTEGER NOT NULL DEFAULT 0,
    resolved_at_ms INTEGER
);
CREATE INDEX notifications_feed ON notifications(updated_at_ms DESC, id DESC);
CREATE INDEX notifications_batches ON notifications(category, first_at_ms);

-- What a notification is about: the games of a batch, the updates on offer.
-- Members keep the entry they came with, so a game merged since can be
-- followed to the game it is now part of.
CREATE TABLE notification_members (
    notification_id TEXT NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
    member TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('game','gameUpdate','emulatorUpdate')),
    game_id TEXT,
    entry_id TEXT,
    source TEXT,
    label TEXT NOT NULL,
    at_ms INTEGER NOT NULL,
    PRIMARY KEY(notification_id, member)
);
