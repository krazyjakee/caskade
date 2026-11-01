-- Epic games were asked about by ids IGDB doesn't keep; matching now asks
-- by their store offers, so those found without a match are due again.
UPDATE igdb_matches SET checked_at_ms = 0
WHERE source = 'none' AND game_id IN (
    SELECT e.game_id FROM game_entries e, json_each(e.record_json, '$.ids') id
    WHERE json_extract(id.value, '$.type') = 'epic'
);
