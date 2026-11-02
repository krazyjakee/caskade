-- Helper programs Caskade installed on this device (the streaming host and
-- client, STREAMING-PLAN §3), beside managed_emulators.
CREATE TABLE managed_tools (
    tool_id TEXT PRIMARY KEY NOT NULL,
    source TEXT NOT NULL,
    dir TEXT,
    install_json TEXT,
    installed TEXT,
    file TEXT,
    latest TEXT,
    latest_url TEXT,
    checked_at_ms INTEGER NOT NULL DEFAULT 0,
    skipped TEXT,
    error TEXT
);

-- Their updates join "Updates available" as members of their own kind.
CREATE TABLE notification_members_next (
    notification_id TEXT NOT NULL REFERENCES notifications(id) ON DELETE CASCADE,
    member TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('game','gameUpdate','emulatorUpdate','toolUpdate')),
    game_id TEXT,
    entry_id TEXT,
    source TEXT,
    label TEXT NOT NULL,
    at_ms INTEGER NOT NULL,
    PRIMARY KEY(notification_id, member)
);
INSERT INTO notification_members_next
    SELECT notification_id,member,kind,game_id,entry_id,source,label,at_ms
    FROM notification_members WHERE kind IN ('game','gameUpdate','emulatorUpdate','toolUpdate');
DROP TABLE notification_members;
ALTER TABLE notification_members_next RENAME TO notification_members;
