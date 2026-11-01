-- RomM servers' ROMs in the library (#1): which library entry each remote
-- ROM became, what the last sync saw, and its local download cache. One
-- row per (server, ROM); the record holds the rest.
CREATE TABLE romm_roms (
    server_id TEXT NOT NULL CHECK(length(server_id) BETWEEN 1 AND 64),
    rom_id INTEGER NOT NULL CHECK(rom_id > 0),
    entry_id TEXT NOT NULL CHECK(length(entry_id) > 0),
    cached INTEGER NOT NULL CHECK(cached IN (0,1)),
    -- Least recently used first when the cache is over its limit.
    last_used_ms INTEGER,
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    PRIMARY KEY(server_id, rom_id)
);
CREATE INDEX romm_roms_entry ON romm_roms(entry_id);
CREATE INDEX romm_roms_cached ON romm_roms(cached, last_used_ms);
