-- The device a play session was played on (#74): its endpoint id, recorded
-- when the session starts here and synced with it, so Home can say a game
-- was last played on another device. NULL for sessions from before this
-- migration and for any recorded before the device had an identity.
ALTER TABLE play_sessions ADD COLUMN device_id TEXT;
