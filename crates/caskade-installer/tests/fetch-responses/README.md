# Resolver response fixtures

`github-7zip-26.05.json` is a projected **real public GitHub API response**;
its source URL, timestamp and raw-response SHA-256 are recorded separately.
Only release flags and asset metadata are retained. No account credentials,
author profile or other irrelevant fields are captured.

Replay tests substitute local asset URLs and the committed mini-game's size and
SHA-256. Asset names, host selection and release response shape remain recorded.
This allows an actual CLI download/install/uninstall proof without network access
or executing upstream programs. The raw upstream digests are exercised separately
in the six-host metadata snapshot.

`drive-confirmation.html` is a small **synthetic contract fixture**, not a live
Google account response. It models the anonymous `download-form` with hidden
confirmation fields. Additional tests cover warning cookies, old download links,
quota/login HTML, unsafe actions, wrong IDs and duplicate/oversized fields.

Use the explicitly ignored `fetch_resolvers_live` test through
`just test-resolvers-live` to refresh public GitHub metadata. Default tests and
native profiles do not contact live services or refresh recordings. Review
recording/snapshot changes before committing them.
