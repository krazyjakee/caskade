-- Library sync (#228–#230). All local: never synced, never backed up (a
-- restored library re-syncs in full on its next connection).

-- Every change to synced data, this device's and those received: who made it
-- (the device's endpoint id) and when (a hybrid logical clock). `key` is in
-- the origin device's ids; `target` is the key here once applied. A NULL
-- value is a tombstone. `applied` is 0 while it waits for a category to be
-- turned on or for a game it refers to.
CREATE TABLE sync_changes (
    origin TEXT NOT NULL,
    wall INTEGER NOT NULL,
    counter INTEGER NOT NULL,
    entity TEXT NOT NULL,
    key TEXT NOT NULL,
    field TEXT NOT NULL,
    value_json TEXT CHECK(value_json IS NULL OR json_valid(value_json)),
    target TEXT,
    applied INTEGER NOT NULL DEFAULT 0 CHECK(applied IN (0,1)),
    PRIMARY KEY(origin, wall, counter)
);
CREATE INDEX sync_changes_pending ON sync_changes(applied, origin, wall, counter);
CREATE INDEX sync_changes_target ON sync_changes(entity, target, field);

-- The value each synced field has here, and the clock of the change that set
-- it: last writer wins against this.
CREATE TABLE sync_fields (
    entity TEXT NOT NULL,
    key TEXT NOT NULL,
    field TEXT NOT NULL,
    value_json TEXT CHECK(value_json IS NULL OR json_valid(value_json)),
    origin TEXT NOT NULL,
    wall INTEGER NOT NULL,
    counter INTEGER NOT NULL,
    PRIMARY KEY(entity, key, field)
);

-- The highest clock this device has from each device: its version vector.
CREATE TABLE sync_seen (
    origin TEXT PRIMARY KEY NOT NULL,
    wall INTEGER NOT NULL,
    counter INTEGER NOT NULL
);

-- What each paired device last said it has, and categories to send it again
-- in full after they were turned back on here.
CREATE TABLE sync_peers (
    device TEXT PRIMARY KEY NOT NULL,
    seen_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(seen_json)),
    replay_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(replay_json))
);

-- Which game here another device's game is (#229), and how they were matched.
CREATE TABLE sync_game_ids (
    origin TEXT NOT NULL,
    origin_id TEXT NOT NULL,
    game_id TEXT NOT NULL,
    method TEXT NOT NULL,
    PRIMARY KEY(origin, origin_id)
);
CREATE INDEX sync_game_ids_game ON sync_game_ids(game_id);
