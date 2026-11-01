-- Named launch actions (#177): a release's play and utility actions besides
-- its own launch (`game_entries.record_json.launch`, the built-in play
-- action). `record_json` is `caskade_core::model::GameAction`.
CREATE TABLE game_actions (
    id TEXT PRIMARY KEY NOT NULL,
    entry_id TEXT NOT NULL REFERENCES game_entries(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    record_json TEXT NOT NULL CHECK(json_valid(record_json))
);
CREATE INDEX game_actions_entry ON game_actions(entry_id,position,id);

-- A release's action choice (default action or ask at launch) and its
-- changes to the built-in action: `caskade_core::model::EntryActionSettings`.
CREATE TABLE entry_action_settings (
    entry_id TEXT PRIMARY KEY NOT NULL REFERENCES game_entries(id) ON DELETE CASCADE,
    record_json TEXT NOT NULL CHECK(json_valid(record_json))
);

-- Lifecycle scripts (#179): global (no scope ids), per emulator profile or
-- per game. `record_json` is `caskade_core::scripts::LifecycleScript`.
CREATE TABLE lifecycle_scripts (
    id TEXT PRIMARY KEY NOT NULL,
    scope TEXT NOT NULL CHECK(scope IN ('global','profile','game')),
    profile_id TEXT REFERENCES emulator_profiles(id) ON DELETE CASCADE,
    game_id TEXT REFERENCES games(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    CHECK (
        (scope='global' AND profile_id IS NULL AND game_id IS NULL)
        OR (scope='profile' AND profile_id IS NOT NULL AND game_id IS NULL)
        OR (scope='game' AND game_id IS NOT NULL AND profile_id IS NULL)
    )
);
CREATE INDEX lifecycle_scripts_scope ON lifecycle_scripts(scope,profile_id,game_id,position,id);
