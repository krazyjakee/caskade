# Native read-only DMG installation

Probing is data-only on every host: read the bounded UDIF `koly` trailer and XML
resource plist, check all spans, reject segmented/invalid/oversized metadata,
and classify the payload as macOS. No filesystem decoder or new C dependency
is used. Off macOS the handler proposes no executable candidate, so normal
orchestration reports Unsupported.

An explicitly injected macOS host/runtime mounts with `hdiutil attach -readonly
-nobrowse -noautoopen -mountpoint <private-temp>/volume -verify <image>`. Bounded
stdin supplies `qy\n` for an SLA pager/acceptance question. Top-level real `.app`
directories are copied through #41's capability writer, keeping ordinary modes,
framework symlinks and xattrs. Decorative metadata and the usual `/Applications`
symlink are skipped. Input-image quarantine is propagated when the app lacks its
own quarantine; quarantine is never removed. Payload code is never executed.

The mount guard exists before attach completes. Cancellation kills and joins an
incomplete attach before attempting detach; cleanup ignores cancellation. Normal
detach is attempted first, then `-force`. Successful attach's process scope is
kept alive until after detach so its helper cannot be killed during copy. Detach
runs after successful copy, error, cancellation and panic unwinding. An explicit
cleanup failure reports the mount path and retains its private directory rather
than recursively deleting a mounted filesystem. Mount points live outside game
staging, so rollback cannot traverse them.

The hermetic `dmg` tests use FakeRuntime, never real/fake filesystem images, to
assert command argv/stdin, attach errors/timeouts, cancellation, panic cleanup,
force retry and retained-directory behavior. Copy checks use the actual committed
app ZIP. Native UDZO/ULFO/SLA acceptance belongs to #44/#22 on the macOS runner.

References: [hdiutil manual](https://keith.github.io/xcode-man-pages/hdiutil.1.html),
[UDIF trailer layout](https://github.com/jetbrains/go-dmg-writer/blob/master/internal/udif/koly.go),
and [Homebrew SLA handling](https://github.com/Homebrew/brew/blob/main/Library/Homebrew/unpack_strategy/dmg.rb).
Public robustness entry points are `parse_trailer(bytes, file_size)` and
`parse_resource_plist(bytes)`; the latter never decodes filesystem block maps.
