-- What the Collections view shows of a collection besides its games.
ALTER TABLE collections ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE collections ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE collections ADD COLUMN updated_at_ms INTEGER NOT NULL DEFAULT 0;
