-- Installed plugins (TECH-PLAN §3 PluginState, #123): whether each is
-- enabled, what the user granted, and its private key-value store.
CREATE TABLE plugins (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    grants_json TEXT NOT NULL CHECK(json_valid(grants_json)),
    updated_at_ms INTEGER NOT NULL
);

-- Kept when a plugin is disabled; removed with the plugin.
CREATE TABLE plugin_kv (
    plugin_id TEXT NOT NULL,
    key TEXT NOT NULL CHECK(length(key) BETWEEN 1 AND 256),
    value BLOB NOT NULL CHECK(length(value) <= 65536),
    PRIMARY KEY(plugin_id, key)
);
