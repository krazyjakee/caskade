# Webview recovery and diagnostics

The main window reloads after a renderer exits or becomes unresponsive on
Windows, and after WebKitGTK's `web-process-terminated` on Linux. A Windows
browser-process exit needs a new environment and controller: Caskade saves its
window position and current route (including search parameters), then restarts.
WebView2's SourceChanged event tracks routes, including history API navigation.
Native callbacks only send notifications; recovery runs after the callback has
returned. GPU, utility and child-frame failures are logged and left to the
runtime's own recovery. See [Microsoft's process recovery guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events).

A renderer that remains unresponsive gets 20 seconds to finish its current
work. Three renderer reloads in two minutes escalate to an app restart. At most
two automatic app restarts run within ten minutes, counted across processes in
`<cache>/webview-restarts`. After that, a single native dialog offers Restart
Caskade and Quit, and shows the log directory. Failure to persist the budget
also stops automatic restarts. The current route is kept separately in
`<cache>/webview-route` and consumed on the next start.

Game windows reload renderers with the same policy and close after a browser
loss or exhausted reload budget. Video windows close after a renderer failure
or browser loss. They do not restart the parent app. A capture uses its normal
close/save rules.

Every process writes to stderr and daily `caskade.YYYY-MM-DD.log` files in
`AppPaths::logs`, keeping seven files. This includes Windows release builds and
the game/video subprocesses. On Windows the directory is
`%LOCALAPPDATA%\Caskade\Caskade\data\logs`. Process failures include the native
kind, exit code, reason and process description when available; source URLs are
excluded because they can contain credentials.

`crates/caskade-webview2` contains the small Windows COM boundary, isolated from
the workspace's `unsafe_code = "forbid"`. It denies unsafe code except at
individual COM calls in its Windows module, where getter out parameters, string ownership and event-handler
lifetimes are documented at each unsafe block. The adapter remains safe Rust.

## Native verification

Use an installed Windows build, with a library route such as
`/library?filter=installed&sort=recent&layout=grid`, and keep a copy of the logs.

1. End Caskade's renderer `msedgewebview2.exe` (its command line includes
   `--type=renderer`) in Task Manager. Check that the page returns on the same
   route, the app PID is unchanged, and the failure is logged.
2. End its browser process (the WebView2 process directly below Caskade with no
   `--type` argument). Check that Caskade restarts, restores the route and window
   position, and leaves no blank player window behind. Other installed apps'
   WebView2 processes are unrelated.
3. Cause three browser failures within ten minutes. Check that automatic
   recovery stops after two restarts, that only one native dialog appears, and
   that Restart Caskade and Quit both work.
4. Repeat with a web game and a trailer. The game renderer should reload; a
   failed video renderer or a dead browser should close its player. Verify that playback stops and Caskade still works.
5. On Linux, run under Xvfb, terminate the WebKit content process belonging to
   the test window, and check reload and the retry limit. Never send synthetic
   input to the real desktop.

The automated tests cover the persistent retry budget and route parsing. The
Windows COM crate can be checked independently with
`cargo check -p caskade-webview2 --target x86_64-pc-windows-gnu`. Real process kills
on Windows require the native checks above.
