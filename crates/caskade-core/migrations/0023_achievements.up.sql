-- Achievements (#185): one set per store account and service product, so
-- two accounts' (or two stores') progress for one game never merges.
-- `sync_json` is `caskade_core::achievements::SetSync`: whether it was
-- fetched, why not, when, and when to ask again.
CREATE TABLE achievement_sets (
    id TEXT PRIMARY KEY NOT NULL,
    store_account_id TEXT NOT NULL REFERENCES store_accounts(id) ON DELETE CASCADE,
    product_id TEXT NOT NULL CHECK(length(product_id) BETWEEN 1 AND 256),
    sync_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(sync_json)),
    created_at_ms INTEGER NOT NULL,
    UNIQUE(store_account_id, product_id)
);

-- A set's achievements: the service's definition and the account's state.
-- `achievement_id` is the service's own id. An unlock, once seen, stays.
CREATE TABLE achievement_items (
    set_id TEXT NOT NULL REFERENCES achievement_sets(id) ON DELETE CASCADE,
    achievement_id TEXT NOT NULL CHECK(length(achievement_id) BETWEEN 1 AND 512),
    position INTEGER NOT NULL CHECK(position >= 0),
    definition_json TEXT NOT NULL CHECK(json_valid(definition_json)),
    state_json TEXT NOT NULL CHECK(json_valid(state_json)),
    unlocked_at_ms INTEGER,
    PRIMARY KEY(set_id, achievement_id)
);

-- A release's identity at an achievement service that its store ids don't
-- give: a RetroAchievements game verified by ROM hash, or set by hand.
-- `record_json` is `caskade_core::achievements::AchievementLink`.
CREATE TABLE achievement_links (
    entry_id TEXT NOT NULL REFERENCES game_entries(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK(length(provider) BETWEEN 1 AND 128),
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    PRIMARY KEY(entry_id, provider)
);
