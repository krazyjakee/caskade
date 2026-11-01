-- Age ratings (#204): each game's ratings as IGDB lists them
-- ([{board, rating}], crate::igdb::IgdbAgeRating) and the minimum age they
-- come to (crate::igdb::age): the highest of its ratings, and for a merged
-- game the highest over its releases. NULL is unrated, never 0.
ALTER TABLE games ADD COLUMN age_ratings_json TEXT NOT NULL DEFAULT '[]'
    CHECK(json_valid(age_ratings_json) AND json_type(age_ratings_json)='array');
ALTER TABLE games ADD COLUMN min_age INTEGER CHECK(min_age IS NULL OR min_age BETWEEN 0 AND 99);
-- Matched games are applied once more, so they get their ratings without a
-- rematch: the IGDB cache fetches the answers it kept from before ratings
-- again (crate::igdb, `store.rs`).
UPDATE igdb_matches SET pending=1 WHERE igdb_id IS NOT NULL;
