# Local patch for hudsucker 0.25.0

The unmodified published crates.io package is committed in `11d1b1a2`. One change:

- `Cargo.toml`: `tokio-rustls` gets `default-features = false` (keeping `logging` and
  `tls12`). Upstream leaves tokio-rustls's defaults on, which enable its `aws_lc_rs`
  feature and so compile aws-lc (C) into rustls. hudsucker's library code takes the
  crypto provider from its caller, so nothing else depends on that default. Caskade passes
  `rustls::crypto::ring::default_provider()` (`apps/desktop/src-tauri/src/web_player/proxy.rs`).

Effect: the web player proxy's upstream TLS uses ring, without the post-quantum
`X25519MLKEM768` key exchange aws-lc's provider offered. Every other TLS client in Caskade
already used ring.

Upgrading: re-apply the change to the new version, or drop this patch if upstream stops
enabling tokio-rustls's defaults. The test `web_player::proxy::tests::the_lockfile_has_no_aws_lc`
fails while aws-lc is back in `Cargo.lock`.

## Draft upstream issue — not filed

Title: Let users build without aws-lc: don't enable tokio-rustls default features.
hudsucker already takes a `CryptoProvider` in `with_rustls_connector` and `RcgenAuthority::new`,
but its `tokio-rustls` dependency keeps default features, forcing `aws-lc-rs`/`aws-lc-sys`
into every dependent's build even when it passes ring. Declaring `tokio-rustls` with
`default-features = false` (and an opt-in `aws-lc-rs` feature for the doc examples and
tests) would fix it.
