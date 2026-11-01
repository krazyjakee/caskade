-- Launchers' own totals become seed sessions again; store totals go.
INSERT INTO play_sessions(id,game_id,entry_id,started_at_ms,ended_at_ms,duration_ms,heartbeat_at_ms,tracking_json)
SELECT lower(hex(randomblob(16))), e.game_id, r.entry_id, coalesce(r.last_played_ms,0), coalesce(r.last_played_ms,0),
       coalesce(r.max_total_ms,0), r.observed_at_ms,
       json_object('discoverySeed', json_object('source', r.provider, 'key', e.external_id, 'aggregate', json('true')))
FROM remote_playtime r JOIN game_entries e ON e.id=r.entry_id WHERE r.account='';
DROP TABLE remote_playtime;
