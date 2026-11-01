-- Games the user separated from a game that IGDB matching merged
-- (crate::metadata): they are never merged with the games that share their
-- IGDB id again.
CREATE TABLE kept_apart (
    game_id TEXT PRIMARY KEY NOT NULL REFERENCES games(id) ON DELETE CASCADE
);
