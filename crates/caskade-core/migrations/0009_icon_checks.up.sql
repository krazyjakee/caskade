-- Executable icons tried for games without an icon (`crate::icons`): what was
-- looked at, so an unchanged game's executables aren't read again.
CREATE TABLE icon_checks (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    signature TEXT NOT NULL,
    checked_at_ms INTEGER NOT NULL
);
