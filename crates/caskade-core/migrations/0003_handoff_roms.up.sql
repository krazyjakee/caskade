-- ROM hand-offs stored before emulation existed (#51) waited in a broken
-- "needs emulator setup" state with the installer's platform ids. They are
-- now emulated games (#120): installed, with Caskade's platform ids.
UPDATE game_entries SET
    state_kind = 'installed',
    record_json = json_set(record_json,
        '$.state', json_object('type', 'installed'),
        '$.launch.platformId', CASE json_extract(record_json, '$.launch.platformId')
            WHEN 'nintendo_game_boy' THEN 'nintendo_gb'
            WHEN 'nintendo_game_boy_color' THEN 'nintendo_gbc'
            WHEN 'nintendo_game_boy_advance' THEN 'nintendo_gba'
            WHEN 'nintendo_64' THEN 'nintendo_n64'
            WHEN 'sega_megadrive' THEN 'sega_genesis'
            ELSE json_extract(record_json, '$.launch.platformId') END)
WHERE source_id = 'caskade-handoff'
    AND launch_kind = 'emulator'
    AND state_kind = 'broken'
    AND json_extract(record_json, '$.state.value.reason') = 'needs emulator setup';
