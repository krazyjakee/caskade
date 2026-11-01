-- Local accounts (#202). `pin_hash` is an argon2id PHC string; the wrong-PIN
-- throttle lives here too, so a restart doesn't reset it.
CREATE TABLE accounts (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    avatar TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('admin','standard')),
    pin_hash TEXT,
    permissions_json TEXT NOT NULL CHECK(json_valid(permissions_json)),
    max_age INTEGER CHECK(max_age IS NULL OR max_age BETWEEN 0 AND 99),
    pin_failures INTEGER NOT NULL DEFAULT 0,
    pin_lockouts INTEGER NOT NULL DEFAULT 0,
    pin_locked_until_ms INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
-- Every library starts with one admin without a PIN, so it starts as before.
-- An admin may do everything: its permissions aren't read.
INSERT INTO accounts(id,name,avatar,role,permissions_json,created_at_ms,updated_at_ms)
SELECT id,'Player','avatar:default','admin','{}',now,now FROM (SELECT
    lower(hex(randomblob(4))||'-'||hex(randomblob(2))||'-'||hex(randomblob(2))||'-'||hex(randomblob(2))||'-'||hex(randomblob(6))) AS id,
    CAST((julianday('now')-2440587.5)*86400000 AS INTEGER) AS now);
-- Collections belong to the account that made them (#206). SQLite can't add
-- a NOT NULL column with a reference; the store always writes one.
ALTER TABLE collections ADD COLUMN account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE;
UPDATE collections SET account_id=(SELECT id FROM accounts);
CREATE INDEX collections_account ON collections(account_id);
