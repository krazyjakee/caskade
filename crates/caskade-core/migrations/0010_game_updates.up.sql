-- Where each managed install's updates come from (crate::updates): the
-- project and release it was installed from, and the newest release the
-- last check found. One row per managed entry that tracking has looked at;
-- provider 'none' when its source has no update provider.
CREATE TABLE game_updates (
    entry_id TEXT PRIMARY KEY NOT NULL REFERENCES game_entries(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK(length(provider) BETWEEN 1 AND 64),
    -- The provider's name for it: 'owner/repo' on GitHub.
    project TEXT NOT NULL,
    -- The release installed; NULL until known.
    installed TEXT,
    -- The file it was installed from, to pick the same one next time.
    asset TEXT,
    prereleases INTEGER NOT NULL DEFAULT 0 CHECK(prereleases IN (0,1)),
    latest TEXT,
    latest_url TEXT,
    checked_at_ms INTEGER NOT NULL DEFAULT 0,
    -- A release the user chose not to be offered.
    skipped TEXT,
    -- Why the last check failed.
    error TEXT
);
