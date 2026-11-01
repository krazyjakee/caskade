UPDATE game_entries SET record_json = json_set(record_json, '$.launch', json_object(
    'type', 'emulator',
    'platformId', json_extract(record_json, '$.launch.platformId'),
    'profile', CASE json_extract(record_json, '$.launch.target.type')
        WHEN 'profile' THEN json_object('type', 'pinned', 'profileId',
            json_extract(record_json, '$.launch.target.profileId'))
        WHEN 'platform_default' THEN json_object('type', 'default')
        ELSE json_object('type', 'ask') END,
    'roms', json((
        SELECT json_group_array(json_extract(rom.value, '$.location.path'))
        FROM json_each(json_extract(game_entries.record_json, '$.launch.roms')) AS rom)),
    'args', json(coalesce(json_extract(record_json, '$.launch.args.args'), json_array()))
))
WHERE launch_kind = 'emulator';
DROP TABLE rom_scanners;
DROP TABLE platform_settings;
DROP TABLE emulator_profiles;
DROP TABLE emulators;
