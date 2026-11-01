-- Games' videos: the IGDB cache fetches the answers it kept from before
-- videos again (crate::igdb, `store.rs` SHAPE 3), but the game page reads
-- only the cache. Matched games are applied once more, which fetches them.
UPDATE igdb_matches SET pending=1 WHERE igdb_id IS NOT NULL;
