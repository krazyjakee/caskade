# Catalogue sources

`starter.json` is the source of the starter add-on catalogue: the example plugins and the Ember theme, with what their manifests don't say. Build and check it with `just starter-catalogue [<dir>]` (default `target/catalogue`).

To try it in the app, serve the folder and point a run at it with a test key (never the release key):

```sh
python3 -m http.server -d target/catalogue 8765
apps/native/node_modules/.bin/tauri signer sign -f crates/caskade-addons/src/testdata/test-signing.key -p "" target/catalogue/catalogue.json
CASKADE_CATALOGUE_URL=http://127.0.0.1:8765/catalogue.json \
CASKADE_CATALOGUE_PUBKEY="$(cat crates/caskade-addons/src/testdata/test-signing.key.pub)" just dev
```

See `docs/plugins/publishing.md`.
