-- Devices paired with this one (#226). Grants are bearer credentials: the
-- table is local (never synced) and left out of backups.
CREATE TABLE paired_devices (
    endpoint_id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    os TEXT NOT NULL,
    -- Where it was last reached, as transport addresses.
    addresses_json TEXT NOT NULL DEFAULT '[]',
    -- Wake-on-LAN MAC addresses (#234).
    wol_macs_json TEXT NOT NULL DEFAULT '[]',
    last_sync_ms INTEGER,
    -- 64 hex characters: what the device presents when it dials this one.
    grant_in TEXT NOT NULL,
    -- What this device presents when it dials that one.
    grant_out TEXT NOT NULL,
    paired_at_ms INTEGER NOT NULL,
    -- Forgotten here, and the device not told yet; refused either way.
    forgotten_at_ms INTEGER
);
