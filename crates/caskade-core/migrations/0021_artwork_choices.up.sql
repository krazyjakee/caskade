-- Artwork the user chose (#175): each kind of a game's artwork the user
-- controls. Under a choice the game shows only its `user`-source artwork of
-- that kind (an image the user imported, or their copy of an automatic one;
-- the cache bytes are shared), and none when there is none: the user
-- removed it. Without a row, automatic precedence applies and no `user`
-- artwork of that kind exists.
CREATE TABLE artwork_choices (
    game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('cover','hero','logo','icon')),
    PRIMARY KEY(game_id,kind)
);
