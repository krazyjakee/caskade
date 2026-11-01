-- Permanent age-control exceptions belong to one account and one game.
CREATE TABLE account_game_approvals (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, game_id)
);
