-- Store accounts (#164): a launcher or store account Caskade reads owned
-- games from. `owner_id` is the local account that connected it (NULL:
-- shared by every account). Credentials never live here (#165).
CREATE TABLE store_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL CHECK(length(provider) BETWEEN 1 AND 128),
    external_id TEXT NOT NULL CHECK(length(external_id) BETWEEN 1 AND 256),
    display_name TEXT NOT NULL,
    owner_id TEXT REFERENCES accounts(id) ON DELETE SET NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
    sync_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(sync_json)),
    created_at_ms INTEGER NOT NULL,
    UNIQUE(provider, external_id)
);

-- What each store account owns. The entry is the game's place in the
-- library, a release that isn't installed (source `store:<account id>`).
-- It follows the game through merges, and is NULL once the user removed
-- the game, so a refresh doesn't bring it back. `lost_at_ms` is set when a
-- complete library snapshot no longer lists the product.
CREATE TABLE account_entitlements (
    store_account_id TEXT NOT NULL REFERENCES store_accounts(id) ON DELETE CASCADE,
    product_id TEXT NOT NULL CHECK(length(product_id) BETWEEN 1 AND 256),
    entry_id TEXT REFERENCES game_entries(id) ON DELETE SET NULL,
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    first_seen_ms INTEGER NOT NULL,
    last_seen_ms INTEGER NOT NULL,
    lost_at_ms INTEGER,
    PRIMARY KEY(store_account_id, product_id)
);
CREATE INDEX account_entitlements_entry ON account_entitlements(entry_id);
