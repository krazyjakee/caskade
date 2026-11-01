-- Matched games apply again once, to fetch IGDB's icon (artwork type 13)
-- that earlier applies didn't take.
UPDATE igdb_matches SET pending=1 WHERE igdb_id IS NOT NULL;
