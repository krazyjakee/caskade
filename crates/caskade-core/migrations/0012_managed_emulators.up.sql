-- Emulators Caskade installed and keeps up to date (EMULATOR-SETUP-PLAN §3).
CREATE TABLE managed_emulators (
    emulator_id TEXT PRIMARY KEY NOT NULL REFERENCES emulators(id) ON DELETE CASCADE,
    definition_id TEXT NOT NULL,
    -- The definition's install source: 'github', 'dolphin', 'url' or 'flatpak'.
    source TEXT NOT NULL CHECK(source IN ('github','dolphin','url','flatpak')),
    -- Where it was unpacked; NULL for Flatpaks.
    dir TEXT,
    -- The version installed; NULL when the source doesn't tell.
    installed TEXT,
    -- The file it was installed from.
    file TEXT,
    latest TEXT,
    latest_url TEXT,
    checked_at_ms INTEGER NOT NULL DEFAULT 0,
    -- A version the user chose not to be offered.
    skipped TEXT,
    -- Why the last check failed.
    error TEXT
);
