# Web game capture spike

Status: Linux measured (WebKitGTK 2.52.6, 2026-10-29); Windows and macOS still to measure · Plan: [WEB-GAMES-PLAN §6](../WEB-GAMES-PLAN.md)

Two questions before offline bundles (W4): can Caskade keep everything a game loads in its game window, and can a saved bundle run games that need `SharedArrayBuffer` (Godot 4's threaded exports)? The spike is a tao + wry window, the versions the app builds, in [`web-capture-spike/`](web-capture-spike/).

## Results on Linux

### Capture

The fixture page (`site/`) loads files every way a game does: a stylesheet with a `url()`, a script, `fetch`, XHR, a Worker that fetches and calls `importScripts`, `WebAssembly.instantiateStreaming`, dynamic `import()`, `new Image()`, `<audio>`, a cross-origin iframe (`other/`, on a second port) that fetches, and a `fetch` 3 s after load.

| Approach | Requests seen | Bodies |
|---|---|---|
| WebKitGTK's `resource-load-started` + `WebKitWebResource::get_data` | All 16 URLs, the Worker's, the iframe's and the late one included | Right for the document's parser-loaded resources (HTML, CSS, scripts, images, `import()`); **1 byte** for `fetch`, XHR, wasm, audio, the Worker's loads and the iframe's |
| A MITM proxy on loopback (`hudsucker`, a CA made at start), set as the window's proxy, with WebKit told to ignore TLS errors in that window's data manager | 15 of the 16: all but the iframe's own `fetch` (see below); HTTPS decrypted (`https://example.com/`) | **Complete** (8-byte wasm, 844-byte WAV, every JSON) |

With COEP added to every response, the cross-origin iframe's own `fetch` was blocked; the proxy should also send `Cross-Origin-Resource-Policy: cross-origin`.

WebKit's API can list what a game loaded but not keep it. The proxy keeps it, and can replay it.

### `SharedArrayBuffer` and cross-origin isolation

`src/isolation.html` reports `isSecureContext`, `crossOriginIsolated` and whether `new SharedArrayBuffer(8)` works.

| Served from | Headers | Secure | Isolated | `SharedArrayBuffer` | With `JSC_useSharedArrayBuffer=1` |
|---|---|---|---|---|---|
| A wry custom scheme (`spike://`) | COOP + COEP | yes | **no** | undefined | works, but not isolated |
| A wry custom scheme | none | yes | no | undefined | works, not isolated |
| `http://127.0.0.1` | COOP + COEP | yes | **yes** | undefined | **works, isolated** |
| `http://127.0.0.1` | none | yes | no | undefined | works, not isolated |

WebKitGTK keeps `SharedArrayBuffer` off unless JavaScriptCore's `useSharedArrayBuffer` option is on, and a custom scheme never counts as cross-origin isolated. Godot 4's threaded exports check both.

## What W4 takes from it

- **One mechanism for capture and playback**: the game window's traffic goes through a proxy in its own process. Capturing, it fetches upstream (verifying TLS itself) and keeps every response; playing a bundle, it answers from the bundle at the original URLs, so the page keeps its origin (its saves, absolute URLs, workers and iframes all work) and gets COOP, COEP and CORP. Ignoring TLS errors is safe only because every request goes through the proxy: no bypass list, loopback included.
- The game window starts with `JSC_useSharedArrayBuffer=1`.
- Custom schemes are out for bundles.

## Still to measure

- **Windows (WebView2)**: wry's proxy config sets `--proxy-server`; the window's own WebView2 environment can be told to accept the proxy's certificates (`--ignore-certificate-errors`, or `ServerCertificateErrorDetected` for only the session CA).
- **macOS (WKWebView)**: wry's proxy needs macOS 14 and its `mac-proxy` feature, and wry doesn't expose the TLS challenge; until a custom navigation delegate handles it, offline bundles may be Linux and Windows only.
- Real games: an itch.io Godot 4 game, a Unity WebGL game, a Poki game, a CrazyGames game and a GitHub Pages game.

## Reproducing

```
cd docs/research/web-capture-spike
python3 make-site.py
(cd site && python3 -m http.server 8741 --bind 127.0.0.1 &) ; (cd other && python3 -m http.server 8742 --bind 127.0.0.1 &)
flock /tmp/caskade-heavy.lock cargo build
Xvfb :518 -screen 0 1280x800x24 &
export DISPLAY=:518 NO_AT_BRIDGE=1 GTK_A11Y=none
webspike capture http://127.0.0.1:8741/index.html   # WebKit's resource API
webspike proxy http://127.0.0.1:8741/index.html     # through the MITM proxy
webspike proxy https://example.com/
for mode in scheme scheme-noheaders http http-noheaders; do webspike $mode; JSC_useSharedArrayBuffer=1 webspike $mode; done
```

`webspike` is in the cargo target directory. On NVIDIA with X11, also set `WEBKIT_FORCE_DMABUF_RENDERER=1 WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`.
