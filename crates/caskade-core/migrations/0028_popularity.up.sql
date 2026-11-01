-- Daily PopScore cache; independent of game metadata and excluded from backups.
CREATE TABLE igdb_popularity (
    igdb_id INTEGER PRIMARY KEY CHECK (igdb_id > 0),
    popularity_type INTEGER,
    value REAL CHECK (value >= 0),
    calculated_at INTEGER,
    fetched_at_ms INTEGER NOT NULL
);
