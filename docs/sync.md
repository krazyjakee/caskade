# Device connections and pairing

How a household's Caskade devices reach each other: the `caskade-sync` crate
(`crates/caskade-sync`), its identity, the opening every connection starts
with, and how to check it. Milestone: Devices, sync and sharing (#252).

## The crate

`caskade-sync` has no Tauri dependency and no database. It owns:

- `Identity`: this device's ed25519 key, in the OS keychain.
- `SyncEndpoint`: the iroh endpoint and router, held for the process lifetime.
  It speaks Caskade's ALPN `caskade/sync/1` and, when configured,
  `iroh_blobs::ALPN` on the same endpoint, key and socket.
- The opening (`opening`): what a connection is about and whether it's welcome.
- Framing (`frame`): one JSON object per line, each read with a bound.

The core decides who is admitted by implementing `Admission`, and takes
accepted connections through `Sessions`. Later work (sync, copies, shares)
builds on a `Session`: the authenticated peer, the purpose, and the opening's
stream left open for the purpose's own messages.

### Versions

iroh 1.0.3, iroh-blobs 0.103.1 and iroh-tickets 1.0.0: the set wobu proved
together (`wobu/docs/10-sync-spike.md`). n0 yanked iroh-blobs 0.103.0 on
2026-10-06 and released 0.103.1, which fixes `EventMask` ignoring
per-request modes; 0.103.1 is the same generation. `Cargo.lock` also holds
iroh-base, iroh-relay and iroh-dns at 1.0.3 and noq at 1.1.1, the versions
iroh 1.0.3 was built and run with. `cargo update` would otherwise move them to
the 1.4 line under iroh 1.0.3. A mismatched iroh/iroh-blobs pair fails at
resolve time with an `ed25519-dalek` error that never names iroh.

TLS goes through ring (iroh's default `tls-ring`), the provider the rest of
Caskade uses.

## Identity

The device key is a 32-byte ed25519 secret in the OS keychain, service
`dev.caskade.sync`, entry `device-key` (beside `dev.caskade.store-accounts`).
Its public half is the device's endpoint id: its name to every paired device
and its TLS certificate.

The secret is never written to the database, settings, logs, events, backups
or the webview. `Identity` has no `Serialize`, its `Debug` prints only the
public id, and the key leaves it only into iroh's endpoint builder.

`Identity::load(store, known)` reads it:

| Keychain | `known` (the id this device had) | Result |
| --- | --- | --- |
| A key | any | `Ready`, the stored identity |
| No entry | none: first use | `Ready { created: true }`, a new key, stored |
| No entry | some | `NeedsConfirmation { Missing }` |
| Not a key | any | `NeedsConfirmation { Unreadable }` |
| No answer within 20 s, or refused | any | `Unavailable`: sync waits, nothing is created |

A new identity is a new device to everything paired with this one: they
refuse it until it pairs again. So after first use it's made only by
`Identity::create`, once the user has agreed. `create` fails rather than
returning an identity it couldn't store, which would be a new device on every
start.

Each keychain call runs on a thread of its own with a deadline. On Linux a
locked keychain can block a read forever, waiting for an unlock prompt nothing
draws. A call that misses the deadline is abandoned, not joined, and the app
loads the identity off the main thread, so the window comes up regardless and
only sync waits. It isn't `spawn_blocking`, because the runtime waits for
those at shutdown.

## The opening

The dialler opens one bidirectional stream and writes one line:

```json
{"purpose":"sync","grant":"<64 hex characters>"}
```

`purpose` is `sync`, `copy`, `share`, `pair` or `forget`. `grant` is what the
dialler holds: the grant the other device gave it when they paired, or the
single-use grant from a pairing link. A grant is 32 random bytes. It's a
capability, not a key: nothing derives, signs or encrypts with it, it never
appears in `Debug` output, and it's compared in constant time.

The accepting device asks its `Admission` once with the authenticated peer id
(`Connection::remote_id`, proved by TLS), the purpose and the grant, and gets
back only yes or no. It answers with one line:

- `{"answer":"accepted"}`: the stream stays open for the purpose's messages.
- `{"answer":"refused"}` (`REFUSAL`), then a QUIC close with code 1 and reason
  `refused`.

Every refusal is those bytes and that close, whatever the reason: an unknown
device, a missing, wrong or used grant, a purpose the grant doesn't open, a
forgotten device, an unknown purpose, a line that isn't JSON or is too long,
or a dialler too slow to finish its opening. A peer learns that it was
refused and nothing about what this device holds. The dialler reports it as
`Error::Refused`, which carries no fields.

There's no key exchange, signature or challenge in application code. TLS 1.3
on the endpoint keys already encrypts the connection and authenticates both
sides.

### Framing

Every message is one JSON object on one line. The reader takes a bound per
message (256 bytes for the opening) and fails as `Error::Malformed` once a
line grows past it, before buffering more. No length a peer wrote is trusted.
Errors never quote what a peer sent.

### Timeouts

Each step of the opening (accepting the stream, reading the offer, writing the
answer, the dial itself) has `Config::open_timeout` (10 s): a peer that
connects and says nothing can't hold a task open, and one that stalls can't
hang the dialler.

## Lifetime

`SyncEndpoint::bind` runs on the caller's tokio runtime; the crate creates
none. iroh's `Router` aborts its accept loop when dropped, so the app holds the
endpoint for the process lifetime and calls `SyncEndpoint::shutdown` at exit.

## Checks

- Tests: `flock /tmp/caskade-heavy.lock cargo test -p caskade-sync`. The
  integration tests (`tests/loopback.rs`) run two endpoints in one process on
  `Reach::Loopback`: one socket on 127.0.0.1, no relay, no address lookup, so
  nothing leaves the machine. They cover the opening, admission, a refusal
  that is byte-identical for every reason (including against a device that
  holds nothing), a silent dialler, the identity across a restart, and
  iroh-blobs on the same endpoint.
- Lint: `flock /tmp/caskade-heavy.lock cargo clippy -p caskade-sync
  --all-targets -- -D warnings`, and the same with
  `--target x86_64-pc-windows-gnu`.
- macOS cross lint, for this crate alone. ring's C build needs Apple SDK
  headers, so the gate compiles it with clang against check-only stubs in
  `crates/caskade-sync/xcheck/apple/` (clippy never links):

  ```sh
  CC_x86_64_apple_darwin=clang \
  CFLAGS_x86_64_apple_darwin="-nostdlibinc -I$PWD/crates/caskade-sync/xcheck/apple" \
  flock /tmp/caskade-heavy.lock cargo clippy -p caskade-sync --all-targets \
    --target x86_64-apple-darwin -- -D warnings
  ```
