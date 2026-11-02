-- Per-release manifest matches and machine-specific save location overrides.
CREATE TABLE release_save_preferences (
    entry_id TEXT PRIMARY KEY NOT NULL REFERENCES game_entries(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK(revision >= 1),
    record_json TEXT NOT NULL CHECK(json_valid(record_json))
);
