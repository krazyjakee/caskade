# Local patch for librqbit-sha1-wrapper 9.0.1

The unmodified published crates.io package is committed in `11d1b1a2`. Two changes:

- `Cargo.toml`: the `sha1-ring` feature enables an optional `ring = "0.17"` dependency
  instead of `aws-lc-rs`.
- `src/lib.rs`: the `sha1-ring` implementation imports
  `ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY as SHA1, SHA256}` instead of the same
  names from `aws_lc_rs::digest`. The two APIs are the same, so no other code changes.

Upstream's `sha1-ring` feature has used aws-lc-rs despite its name, which put aws-lc (C,
~36 s of build script) into every cold build for torrent hashing (`caskade-installer`'s
`torrent` feature). The alternative feature, `sha1-crypto-hash`, would link system
OpenSSL on Linux.

Upgrading: re-apply the change to the new version, or drop this patch if upstream's
`sha1-ring` uses ring again. The test `web_player::proxy::tests::the_lockfile_has_no_aws_lc`
in `caskade-desktop` fails while aws-lc is back in `Cargo.lock`.
