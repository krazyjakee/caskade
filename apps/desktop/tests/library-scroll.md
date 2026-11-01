# Library scroll measurement

Use an isolated Chromium profile with CDP on port 9223. The script creates and
closes its own tabs; never attach it to a normal user profile.

For a production frontend measurement:

```sh
flock /tmp/caskade-heavy.lock pnpm build
pnpm --filter @caskade/desktop exec vite preview --port 1424 --strictPort
# In a second terminal:
flock /tmp/caskade-heavy.lock python3 apps/desktop/tests/library-scroll.py \
  --url http://localhost:1424 --port 9223 --build release \
  --ipc-fixtures apps/desktop/tests/library-scroll-ipc.js \
  --output target/app-core/library-scroll-release.json
```

The IPC preload supplies 1,203 original synthetic native DTOs, empty job/root
inventories and event subscriptions. It is a benchmark harness only: it is never
imported by the app or bundled into release assets. No local library data is
read. This measures rendering through the native adapter, rather than native
IPC/database latency. Development fixture mode remains separately available as
documented in the script.

Release mode rejects a page containing Vite's development client. Both layouts
use the same 1440×1000 viewport, fonts, pagination warmup and 240-frame traversal.
The report retains browser/build, refresh baseline, missed refresh intervals,
maximum mounted cards and the final game reached. The command fails on any
missed interval or unbounded/empty card inventory. A fixed request deadline also
prevents an event stream from keeping an unresponsive CDP request alive forever.

27 October 2026, Chromium 155, source `559d676`: release grid and list both
reported **0/240** missed intervals, maximum 16.8 ms; grid/list mounted maxima
were **36/10**, and both reached `Fixture Game 1203`. Development measurements
after the storage controls had reported three grid misses; that result is not
presented as a release measurement. See issue #107 for the full JSON evidence.
