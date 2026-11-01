-- Play time a store or a launcher reports for a release (#169), apart from
-- the sessions Caskade tracks. One row per release, provider and account
-- (account '' for a launcher's own total on this computer). `total_ms` is
-- the latest total and `max_total_ms` the largest seen (a reset never
-- lowers the count); both NULL when the source doesn't say, which is not
-- zero. `last_played_ms` is NULL when unknown. `family` is the store
-- identity the total belongs to; `counted_ms` what the game's total adds
-- for the row after the overlap rules (crate::db::playtime).
CREATE TABLE remote_playtime (
    entry_id TEXT NOT NULL REFERENCES game_entries(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    account TEXT NOT NULL DEFAULT '',
    family TEXT,
    total_ms INTEGER CHECK(total_ms IS NULL OR total_ms>=0),
    max_total_ms INTEGER CHECK(max_total_ms IS NULL OR max_total_ms>=0),
    last_played_ms INTEGER,
    observed_at_ms INTEGER NOT NULL,
    counted_ms INTEGER NOT NULL DEFAULT 0 CHECK(counted_ms>=0),
    PRIMARY KEY(entry_id, provider, account)
);
CREATE INDEX remote_playtime_entry ON remote_playtime(entry_id);

-- Launcher totals discovery seeded as sessions (#100) move here once and
-- count as they did; the made-up sessions go.
INSERT OR IGNORE INTO remote_playtime(entry_id,provider,account,total_ms,max_total_ms,last_played_ms,observed_at_ms,counted_ms)
SELECT entry_id, coalesce(json_extract(tracking_json,'$.discoverySeed.source'),'launcher'), '',
       duration_ms, duration_ms, CASE WHEN started_at_ms>0 THEN started_at_ms END,
       heartbeat_at_ms, duration_ms
FROM play_sessions
WHERE json_extract(tracking_json,'$.discoverySeed.aggregate')=1 AND ended_at_ms IS NOT NULL
ORDER BY duration_ms DESC;
DELETE FROM play_sessions
WHERE json_extract(tracking_json,'$.discoverySeed.aggregate')=1 AND ended_at_ms IS NOT NULL;
