# JS/TS backend spike: `jco componentize` against `caskade:plugin@0.1.0` (#132)

**Question** (TECH-PLAN §4.5, §9): can a plugin's backend half be written in TypeScript and built with jco, or should TypeScript authors be told to write a UI half plus a Rust backend for 0.1?

**Answer: not blessed for 0.1.** A TypeScript backend built with jco's default StarlingMonkey backend loads in the host and passes every check the Rust test guest passes. But it can't do outbound HTTP from our synchronous exports, is about 70× larger, takes seconds to compile on first use, and costs several times more per call and in memory. Point TypeScript authors to **UI half + Rust backend** for 0.1. Revisit when the world moves to WASI 0.3 async exports and StarlingMonkey can build them (details below). No scaffolder template follows from this spike.

## What was built

- `docs/research/js-backend-spike/plugin.ts`: a TypeScript backend for the `plugin` world, mirroring the Rust test guest (`crates/caskade-plugin-host/tests/guest`). It answers `handle-request` (`echo`, `kv`, `emit`, `settings`, `fetch` for outbound HTTP, and `spin`/`grow`/`trap` failure modes), handles `on-event` (stores the payload in KV) and `intercept` (rewrites or cancels).
- `docs/research/js-backend-spike/build.sh`: builds it with jco 1.39.0 (componentize-js 0.23, StarlingMonkey by default; `quickjs` as the second argument). jco only ever sees a copy of `wit/`, because it can rewrite the WIT folder it is given (see Blockers).
- `crates/caskade-plugin-host/tests/js_guest.rs`: an opt-in host test (`#[ignore]`, needs `CASKADE_JS_GUEST`). It runs the same functional checks as `tests/host.rs` against the JS component, then the failure-containment and measurement passes for both guests:

```sh
docs/research/js-backend-spike/build.sh /tmp/plugin.wasm
CASKADE_JS_GUEST=/tmp/plugin.wasm cargo test --release -p caskade-plugin-host \
  --test it js_guest:: -- --ignored --nocapture
```

## Results

Measured on 27 Oct 2026, Linux x86-64, `cargo test --release` (wasmtime 50, Cranelift), host defaults (64 MiB memory, 2 s per call). The Rust guest is `tests/guest`, built with `--release` for wasm32-wasip2. Latencies are host-side, around `PluginHost::request`/`intercept`; 1,000 echo calls and 200 intercepts per guest on a running instance.

| | Rust guest | TS, StarlingMonkey | TS, StarlingMonkey `--aot` | TS, QuickJS |
|---|---|---|---|---|
| Component size | 0.17 MiB | 11.5 MiB | 14.6 MiB | 6.2 MiB |
| jco build time | — | 4.1 s | 11.1 s | 1.8 s |
| Loads in the host | yes | yes | yes | **no**: `task.cancel` needs the component-model-async feature |
| Same checks as the Rust guest (echo, KV, `on-event`, `emit` → bus, intercept continue/cancel, plugin error) | pass | **pass** | **pass** | — |
| First use: compile + instantiate | 20 ms | 1.04 s | 1.05 s | — |
| Restart (instantiate a compiled component), median | 0.11 ms | 0.14 ms | 0.16 ms | — |
| `handle-request` echo, median / p95 | 0.56 µs / 0.58 µs | 15.6 µs / 28.7 µs | 15.5 µs / 22.1 µs | — |
| `intercept`, median | 0.97 µs | 34.5 µs | 21.1 µs | — |
| Smallest memory limit that starts | 2 MiB | 12 MiB | 12 MiB | — |
| Endless loop (300 ms call limit) | epoch stop, next call works | epoch stop, next call works | same | — |
| 256 MiB allocation | memory limit, next call works | memory limit, next call works | same | — |
| Thrown error / panic | trap, next call works | trap, next call works | same | — |
| `fetch()` from `handle-request` | n/a | **never settles** (returns `"pending"`) | same | — |

Notes:
- First use is dominated by compiling about 11 MiB of SpiderMonkey with Cranelift. The host keeps compiled components only in memory, so every app start pays it again on the plugin's first call. In a debug build of the app (`pnpm tauri dev`), the same compile took 15.4 s. The JS times for `spin` above (about 1.3–1.4 s) are that first compile plus the 300 ms limit.
- The wizer snapshot makes restarts cheap: after a crash or `deactivate`, a JS backend is back in about 0.15 ms.
- AOT (weval) costs 3 MiB and builds 2.7× slower. It helps `intercept` (21 vs 35 µs) but not echo, which is host-boundary bound.
- 12 MiB is the per-instance floor before any plugin data: a host running ten JS backends needs at least about 120 MiB, against about 20 MiB for Rust.

## Blockers

1. **No outbound HTTP from the 0.1 exports.** `fetch()` is asynchronous, and every `caskade:plugin@0.1.0` export is a synchronous function. The spike's `fetch` request starts a request, but the export returns before the promise settles, so it answers `"pending"`, and nothing in a synchronous call can wait for it. HTTP is the main reason a backend exists in JS (TECH-PLAN §4.5), so this alone rules out blessing it for 0.1.
2. **WASI 0.3 async exports aren't buildable yet with the default backend.** With `handle-request` declared as `async func` in a private copy of the WIT, StarlingMonkey (componentize-js 0.23) crashes in its splicer (`RuntimeError: unreachable`). QuickJS builds it (1.6 MB), but see 3. The host would also need wasmtime's `component-model-async` support and an async `plugin` world (a 0.2 API change).
3. **The QuickJS backend doesn't load in our host.** Its components use component-model-async intrinsics (`task.cancel`) even for synchronous worlds, so wasmtime refuses them unless the host enables that feature. They also export `init(shim, script, …)`, which must be called with the source at startup, and bundled TypeScript pulls in `jco:node/{http,timers,console}` imports and callback exports that only jco's own Node/browser runtime provides. That route needs async support in the host and shims for jco's private interfaces.
4. **Every StarlingMonkey component imports `wasi:http`**, whatever its permissions. That's harmless here: the host always links `wasi:http` and enforces the network allowlist per request (#124). But any host that links HTTP only for plugins with a `network` grant would have to change, or build with `--disable http`.
5. **Tooling side effect.** jco can rewrite the world file in the `--wit` folder it is given (it added `jco:node/*` imports and created `wit/deps/jco-node-0.1.0/` during this spike) and auto-adds dependencies. A scaffolder template would have to copy the WIT first, as `build.sh` does.

## Recommendation

- **0.1: don't bless TypeScript backends.** Docs and the scaffolder (#130) point TypeScript authors to a **UI half in TypeScript plus a Rust backend** (`caskade-plugin-sdk`, #129), or a UI-only plugin when no backend is needed. The functional parity is real, so this is not a "never". But without HTTP, the main use of a JS backend (calling web APIs such as HowLongToBeat, #133) is impossible, and the costs below need a reason to accept them.
- **What would change the answer** (track as follow-ups, not 0.1 work):
  1. An async `plugin` world (`handle-request`, `on-event`, `intercept` as `async func`, WASI 0.3), with wasmtime's `component-model-async` enabled in the host. That's a host API minor bump and a wasmtime feature decision.
  2. componentize-js building async exports (StarlingMonkey currently crashes in its splicer), or a QuickJS output that doesn't need jco's host interfaces.
  3. An on-disk compiled-component cache in the host (wasmtime `Component::serialize`/`deserialize` keyed by component hash and engine). That removes the 1 s (15 s in debug) first-call compile on every start, and benefits Rust plugins too.
- **If blessed later**, the template would use StarlingMonkey without `--aot` (smaller, faster to build, the same echo latency), copy `wit/` before building (blocker 5), and document the 12 MiB per-instance floor. The host's 64 MiB default limit leaves room.
- Rust stays the reference backend: 70× smaller, about 30× faster per call, 50× faster to compile, 6× less memory.
