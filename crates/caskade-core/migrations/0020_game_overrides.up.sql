-- Manual game edits (#171): the fields the user set by hand, each with the
-- value automatic sources (discovery, IGDB, RomM, scanners) would give it.
-- `record_json` and its columns hold what the library shows, the user's
-- values included; `{field: automatic value}` (camelCase `Game` keys) is the
-- layer beneath, which automatic refreshes write and Reset restores.
ALTER TABLE games ADD COLUMN overrides_json TEXT NOT NULL DEFAULT '{}'
    CHECK(json_valid(overrides_json) AND json_type(overrides_json)='object');
