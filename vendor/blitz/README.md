# Blitz, anyrender, Stylo and Taffy with Caskade's patches

`crates/` holds the Blitz, anyrender, Stylo and Taffy crates Caskade's native UI
builds: the upstream revisions in [`PINS`](PINS) with the patch series in
[`patches/`](patches/) applied, each crate rewritten as a standalone package.
Both builds take them through `[patch]` entries for the four git sources:

- the root workspace's `caskade-native` (root `Cargo.toml`);
- the prototype, `apps/native-proto` (its own `Cargo.toml`).

`apps/native/Cargo.toml` and the prototype keep their upstream git
dependencies at the same revisions; the patches redirect them here, so
`Cargo.lock` only records these crates without a source.

Don't edit `crates/` by hand: it is generated. The root workspace excludes
`vendor/`, so these crates are not workspace members and Caskade's clippy,
formatting and i18n checks skip them.

## The series

| Repo | Patches | Upstream |
| --- | --- | --- |
| blitz | 0001–0005: gradient centring and tiling, inline SVG snapshots, damage-limited repaint of animation frames, `prefers-reduced-motion`; 0006–0013: screen readers (ARIA states and properties, bounds, actions routed to DOM events, controls named by their labels, no stray `<main>` around a Dioxus app, the AT-SPI backend built, text as labels, the window named); 0014–0020: text fields (composition events, IME placement and purpose, preedit underline, read-only and password fields, clipboard fixes and commands, selection past the edge and by long press, `::selection`, setting the value); 0021–0022: `text-overflow` (ellipsis and string markers, from blitz#893) and placeholders in the `::placeholder` style | not yet submitted ([0021–0022 drafted](../../docs/spikes/blitz-1.md#upstream-not-filed-waiting-for-the-users-go)) |
| anyrender | 0001–0002: damage rendering in vello_hybrid; 0003–0004 measurement only (`ANYRENDER_PRESENT_MODE`, `ANYRENDER_FRAME_LIMIT_HZ`) | not yet submitted (0003–0004 never) |
| stylo | 0001–0002: when a stepped animation next changes, the `prefers-reduced-motion` media feature | not yet submitted |
| taffy | 0001: an auto-height column flex container sizes from its items' hypothetical main sizes, not their content (`flex: 1 1 0` with a min-height kept a scroll area the view's height) | not yet submitted ([draft issue](../../docs/spikes/blitz-2.md#upstream-not-filed-waiting-for-the-users-go)) |

What each does and why: [`docs/spikes/blitz-3.md`](../../docs/spikes/blitz-3.md)
(#82, with measurements), [`docs/spikes/blitz-4.md`](../../docs/spikes/blitz-4.md) (#83), [`docs/spikes/blitz-5.md`](../../docs/spikes/blitz-5.md) (#84) and [`docs/spikes/blitz-2.md`](../../docs/spikes/blitz-2.md) (#81). When a patch lands upstream, drop it from the series at the next pin
bump.

## Regenerating

```sh
vendor/blitz/update.sh [work dir]   # default ~/.cache/caskade-blitz-vendor, outside the repo
```

It clones each repo at its pin into the work dir, applies the series on a
`caskade` branch, resolves every crate with `cargo metadata` in a scratch
workspace, and writes `crates/` with [`normalize.py`](normalize.py):
standalone manifests (no `workspace = true`), path dependencies between the
vendored crates, no dev-dependencies, tests or examples (and only what a
manifest's `include` lists, when it has one: Taffy's crate is its
repository's root). Optional
dependencies outside the set (Blitz's devtools server) are left out with the
features that enable them. Run it twice and nothing changes.

## Changing a patch

1. `vendor/blitz/update.sh <work dir>`, then edit the checkout in
   `<work dir>/<repo>` and commit there (one commit per change, with a
   message that can go upstream).
2. Export the series again: delete `patches/<repo>/*.patch`, then, from
   the repo's root,
   `git -C <work dir>/<repo> format-patch --zero-commit -N --no-signature -o "$PWD/vendor/blitz/patches/<repo>" <pin>..caskade`
   (`-o` must be absolute: with `-C`, a relative path lands in the
   checkout). Export before running `update.sh` again: it resets the
   `caskade` branch to the exported series, and unexported commits are
   left only in the checkout's reflog.
3. `vendor/blitz/update.sh <work dir>` and build both:
   `cargo build -p caskade-native` and, in `apps/native-proto`,
   `cargo build --profile proto`.
4. Commit `patches/`, `crates/` and any lockfile change together.

## Bumping a pin

Blitz, anyrender, Stylo and Dioxus move together (#113, condition 6).
Taffy's revision is the one Blitz's workspace names for its `taffy`
dependency: bump it with Blitz's.

1. In `PINS`, set the new revisions (and the crate lists, if the upstream
   crates changed).
2. Bump the git `rev`s to match: `apps/native/Cargo.toml` and
   `apps/native-proto/Cargo.toml` (Blitz's `dioxus-native`, `blitz-*`), and
   Dioxus's crates.io versions there if Blitz's workspace moved them.
3. `vendor/blitz/update.sh <work dir>`. A patch that no longer applies stops
   it: rebase it in the checkout (`git am --abort`, fix, commit), then export
   the series again as above. Drop patches that landed upstream.
4. If the set of crates the root lockfile takes from these repos changed,
   update the root `Cargo.toml`'s `[patch]` lists (one entry per crate the
   lockfile names; the prototype's lists follow `PINS`).
5. `cargo update -p dioxus-native` (root) and `cargo update` in
   `apps/native-proto` for the new revisions; check that the root lockfile
   moved only these crates and their new dependencies: iroh's crates keep
   their versions (never `cargo update` them), and no `aws-lc-*` package
   appears (`grep aws-lc Cargo.lock`; nothing checks this automatically).
6. Rerun BLITZ-0's and BLITZ-3's measurements (`apps/native-proto/scripts`,
   `docs/spikes/blitz-0.md` and `blitz-3.md`) and note what moved.
