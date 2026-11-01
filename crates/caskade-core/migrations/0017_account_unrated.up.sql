-- Whether a game without an age rating needs an admin PIN for a standard
-- account with an age limit (#208); off, so unrated games play as before.
ALTER TABLE accounts ADD COLUMN unrated_needs_pin INTEGER NOT NULL DEFAULT 0
    CHECK(unrated_needs_pin IN (0,1));
