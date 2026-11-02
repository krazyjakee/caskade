CREATE TABLE notification_members_next (
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
INSERT INTO notification_members_next
    SELECT notification_id,member,kind,game_id,entry_id,source,label,at_ms
    FROM notification_members WHERE kind IN ('game','gameUpdate','emulatorUpdate');
DROP TABLE notification_members;
ALTER TABLE notification_members_next RENAME TO notification_members;
DROP TABLE managed_tools;
