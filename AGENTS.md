# AGENTS.md

Guidance for coding agents working in this repository. `CLAUDE.md` holds the full project guide (layout, commands, build, lint and test); read it too.

## Git workflow

- Commit all work to the default branch, `master`, in the main checkout. Don't create branches or git worktrees. Several agents work in this checkout at once, and work kept off `master` drifts from theirs and has to be merged back later.
- Commit only the files you changed: `git add <paths>`, never `git add -A`, `git add .` or `git commit -a`. Other sessions' uncommitted changes share the working tree; leave them for their owners.
- The repository is GitHub: `origin` is https://github.com/krazyjakee/caskade, and issues, milestones and pull requests live there (`gh issue …`). Push with `git push origin master`; never force-push. Gitea (`gitea` remote, 192.168.1.144) is archived and read-only since 3 Nov 2026: its full commit history and closed issues (Gitea numbers, which old commit messages cite) stay readable there, and GitHub's history before then is one summary commit per publish.
- `CLAUDE.md` repeats these rules. Keep the two in step.

## Translations

- Translations (`docs/translating.md`, `docs/localization.md`): `just i18n` extracts marked text into `locales/<domain>.pot` (`ui`, `app`, `core`) and merges it into each `locales/<lang>/<domain>.po` (needs GNU gettext); commit both with the change. `just i18n-check` (also in CI) fails on a stale template, a PO file `msgfmt --check` rejects or a translation that drops or changes a placeholder or tag, and prints each language's completion. `pnpm i18n:extract --add <lang>` starts a language; adding `locales/<lang>/*.po` is all it takes to ship one.

## Sync

- Sync (devices, library sync, saves, copies, shares and playing elsewhere; `docs/sync.md`): `crates/caskade-sync` holds the iroh endpoint, pairing and the copies' blob transfer; `caskade_core::{devices, sync, transfer, remote_play, saves}` build on it. Never `cargo update` iroh-base, iroh-relay, iroh-dns or noq (`Cargo.lock` pins them to iroh 1.0.3's). The device key stays in the OS keychain: never in the database, settings, logs, events, backups or the webview; grants never reach the webview. A new table or setting needs its `sync::policy` classification: the guard test `every_synced_or_optional_rule_has_an_entity` and the `sync_policy` fixtures fail otherwise. Game files move only over endpoints with no relay transport (`DirectDialer`, `SwarmDialer`, `FileServer`), never a relay. Tests run two or more cores in one process (`crates/caskade-core/tests/cores.rs`, on loopback or `caskade_sync::testing::local_relay`) and never reach the internet: `flock /tmp/caskade-heavy.lock cargo test -p caskade-sync` and `cargo test -p caskade-core --test it -- sync_ game_copy devices::`.
- `CLAUDE.md` has this entry too. Keep the two in step.
