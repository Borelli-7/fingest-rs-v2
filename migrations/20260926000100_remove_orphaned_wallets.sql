-- Account deletion used to cascade only the account_wallet link, leaving the wallet row and
-- every expense in it unreachable. Remove wallets that no account owns any more.
DELETE FROM wallet w
WHERE NOT EXISTS (
    SELECT 1 FROM account_wallet aw WHERE aw.wallet_id = w.id
);
