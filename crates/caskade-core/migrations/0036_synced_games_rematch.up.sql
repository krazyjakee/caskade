-- Games that came from another device have no entries here, so matching
-- found nothing to ask the metadata server about and recorded them as not
-- on IGDB, not to be asked about again for a week (#75). It now asks about
-- the store ids their devices sent: look at them again on the next run.
UPDATE igdb_matches SET checked_at_ms=0
WHERE source='none'
  AND game_id IN (SELECT game_id FROM sync_game_ids WHERE method='created');
