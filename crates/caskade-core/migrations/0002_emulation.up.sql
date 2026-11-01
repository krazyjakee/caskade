-- Emulation (EMULATION-PLAN §4). Profiles have their own rows so platform
-- defaults and scanners can reference them; JSON keeps the typed payload.
CREATE TABLE emulators (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(trim(name)) > 0),
    definition_id TEXT,
    install_kind TEXT NOT NULL CHECK(install_kind IN ('folder','path','flatpak','app_image','app_bundle')),
    record_json TEXT NOT NULL CHECK(json_valid(record_json))
);
CREATE INDEX emulators_name ON emulators(name COLLATE NOCASE,id);

CREATE TABLE emulator_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    emulator_id TEXT NOT NULL REFERENCES emulators(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    kind TEXT NOT NULL CHECK(kind IN ('built_in','custom')),
    definition_profile TEXT,
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    UNIQUE(emulator_id,id),
    CHECK((kind = 'built_in') = (definition_profile IS NOT NULL))
);
CREATE INDEX emulator_profiles_order ON emulator_profiles(emulator_id,position);

-- Deleting a default's profile leaves the platform without a default.
CREATE TABLE platform_settings (
    platform_id TEXT PRIMARY KEY NOT NULL,
    emulator_id TEXT,
    profile_id TEXT,
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    CHECK((emulator_id IS NULL) = (profile_id IS NULL)),
    FOREIGN KEY(emulator_id,profile_id) REFERENCES emulator_profiles(emulator_id,id) ON DELETE SET NULL
);

-- A scanner's profile decides what it matches, so it cannot be deleted
-- while a scanner uses it.
CREATE TABLE rom_scanners (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(trim(name)) > 0),
    folder TEXT NOT NULL CHECK(length(folder) > 0),
    platform_id TEXT,
    emulator_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    in_library_update INTEGER NOT NULL CHECK(in_library_update IN (0,1)),
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    FOREIGN KEY(emulator_id,profile_id) REFERENCES emulator_profiles(emulator_id,id) ON DELETE RESTRICT
);
CREATE INDEX rom_scanners_profile ON rom_scanners(emulator_id,profile_id);

-- Emulator launches move to {platformId, target, roms, args}. No emulators
-- existed before, so a pinned profile falls back to the platform default.
-- ROM paths were relative to an external location; they become absolute.
UPDATE game_entries SET record_json = json_set(record_json, '$.launch', json_object(
    'type', 'emulator',
    'platformId', json_extract(record_json, '$.launch.platformId'),
    'target', json_object('type',
        CASE json_extract(record_json, '$.launch.profile.type')
            WHEN 'ask' THEN 'ask_emulator' ELSE 'platform_default' END),
    'roms', json((
        SELECT json_group_array(json_object(
            'location', json_object('type', 'absolute', 'path',
                CASE
                    WHEN substr(rom.value, 1, 1) IN ('/', '\') OR substr(rom.value, 2, 1) = ':'
                        OR json_extract(game_entries.record_json, '$.location.type') IS NOT 'external'
                    THEN rom.value
                    ELSE rtrim(json_extract(game_entries.record_json, '$.location.path'), '/\')
                        || CASE WHEN instr(json_extract(game_entries.record_json, '$.location.path'), '\') > 0
                            THEN '\' ELSE '/' END
                        || rom.value
                END),
            'name', NULL,
            'size', NULL))
        FROM json_each(json_extract(game_entries.record_json, '$.launch.roms')) AS rom)),
    'args', CASE
        WHEN json_array_length(json_extract(record_json, '$.launch.args')) > 0
        THEN json_object('type', 'append', 'args', json(json_extract(record_json, '$.launch.args')))
        ELSE json_object('type', 'default') END
))
WHERE launch_kind = 'emulator';
