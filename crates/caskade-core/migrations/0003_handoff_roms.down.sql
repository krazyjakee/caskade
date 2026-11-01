-- Platform ids keep Caskade's names; only the placeholder state returns.
UPDATE game_entries SET
    state_kind = 'broken',
    record_json = json_set(record_json, '$.state',
        json_object('type', 'broken', 'value', json_object('reason', 'needs emulator setup')))
WHERE source_id = 'caskade-handoff' AND launch_kind = 'emulator';
