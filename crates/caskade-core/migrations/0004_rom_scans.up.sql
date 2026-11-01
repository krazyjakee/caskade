-- What each ROM scanner found last time, and the choices a manual review
-- made (#119), so the next automatic scan agrees with them. Both go with
-- their scanner.
CREATE TABLE rom_scan_state (
    scanner_id TEXT PRIMARY KEY NOT NULL REFERENCES rom_scanners(id) ON DELETE CASCADE,
    last_scan_ms INTEGER NOT NULL,
    games INTEGER NOT NULL CHECK(games >= 0)
);

-- Keyed by a ROM's path relative to the scanner folder (`scanner:<id>/…`).
CREATE TABLE rom_overrides (
    scanner_id TEXT NOT NULL REFERENCES rom_scanners(id) ON DELETE CASCADE,
    path TEXT NOT NULL CHECK(length(path) > 0),
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    PRIMARY KEY(scanner_id, path)
);
