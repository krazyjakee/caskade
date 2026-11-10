# Catalogue test data

- `test-signing.key` / `.pub`: a minisign key pair made with `pnpm tauri signer
  generate --ci -p ""` **for tests only**. No build trusts it: release builds
  check the catalogue against Caskade's release key (`tauri.conf.json`,
  `plugins.updater.pubkey`). Local catalogue servers (`scripts/addons/`) sign
  with it, and the app trusts it only with `CASKADE_CATALOGUE_PUBKEY` set.
- `catalogue.json`: a plugin, a theme and records that must be left out (bad
  id, duplicate id, a metadata provider, unusable releases).
  `catalogue.json.sig` is its signature with the test key:
  `pnpm tauri signer sign -f test-signing.key -p "" catalogue.json`.
