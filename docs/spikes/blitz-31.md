# BLITZ-31: packaging, platforms and the GPU fallback (2026-11-10)

Issue #110. The native app (`caskade-native`) ships beside the Tauri app
until the cutover (BLITZ-33). This records how it's bundled, what each
platform still needs, how it draws without a GPU, and what it costs.

## Bundles

Every release bundle carries both apps. The release's build legs
(`.github/workflows/build-bundles.yml`, called by `release.yml`) build
`caskade-native` for the target, then the Tauri bundles with it inside as a
second executable (Tauri's `externalBin`, beside the 7-Zip sidecar). They
install, update and uninstall together; the native app's own updater
(`caskade_app::self_update`) replaces the same bundles, from the same
`latest.json` and signatures.

| Bundle | Where the native app is | Starts with |
| --- | --- | --- |
| deb | `/usr/bin/caskade-native` | `caskade-native`, or `caskade --native` |
| AppImage | inside the image, beside `caskade` | `Caskade.AppImage --native` (AppRun only starts `caskade`, which hands over) |
| NSIS | `caskade-native.exe` in the install folder | that file, or `Caskade.exe --native` |
| macOS app (dmg, updater tarball) | `Caskade.app/Contents/MacOS/caskade-native` | that file, or `Caskade --native` |

`caskade --native` starts the sibling binary with the remaining arguments:
in its place on Unix (`exec`), as a process of its own on Windows. From
inside an AppImage it keeps the image's environment (`APPIMAGE`, its
libraries), which the self-updater needs.

The `bundles` workflow (Actions → bundles) runs the same build legs on any
commit and leaves the bundles as artifacts: no tag, draft, signatures or
secrets. Its first run (42846824721, commit 2d94c01c) built all seven
bundles.

### Webviews each platform still needs

The native app draws its main window itself, but web games and the video
window stay in webviews (`caskade-player`, processes of their own):

- **Linux:** WebKitGTK 4.1. The deb depends on `libwebkit2gtk-4.1-0`, and
  the AppImage bundles it, as both already did for the Tauri app.
- **Windows:** WebView2. The NSIS installer's bootstrapper installs it
  where it's missing, as for the Tauri app. The native app links the MSVC
  runtime in (`+crt-static`), as the Tauri CLI does for `caskade.exe`: the
  first artifact run's `caskade-native.exe` didn't start on a fresh
  Windows 10 (no `VCRUNTIME140.dll`).
- **macOS:** WKWebView, part of the system.

Nothing new is bundled for them.

## Drawing without a GPU

The renderer is chosen as the window opens (series `blitz/0026`,
`anyrender/0005`–`0007`):

1. **vello_hybrid** on a GPU adapter wgpu finds (Vulkan, DX12, Metal or
   GL).
2. **vello_cpu over softbuffer** when vello_hybrid can't start: no
   adapter, device or surface, or only a software adapter (wgpu's
   `DeviceType::Cpu`: llvmpipe, lavapipe, WARP). A software adapter
   rasterises through a GPU API and presents the whole window every frame.
   vello_cpu repaints only an animation's damage, and softbuffer presents
   only that rectangle when the surface's buffer still holds the last
   frame.

`CASKADE_RENDERER=cpu` forces the CPU path. On it the root gets
`data-paint="cpu"`, as a webview painting on the CPU does, so the tokens'
`paint-cpu` set pauses ambient loops unless Motion is Full.

Measured with release builds on a private 1440×900 Xvfb, with Vulkan
restricted to Mesa's lavapipe (`VK_ICD_FILENAMES`). CPU is the share of
one core over 30 s, after a 20 s settle:

| Running | Before (vello_hybrid on lavapipe) | After (vello_cpu, damage) |
| --- | ---: | ---: |
| Nothing | 0.13% | 0.16% |
| The status-bar spinner | 22.7% | 0.56% |
| Spinner and ambient backdrop | 361% | 0.90% (backdrop paused) |
| Memory (RSS) | 355 MB | 302 MB |

381 of the spinner's 384 frames were damage-only, about 0.1 ms each:
88 µs to render the region, 12 µs to present it. With no Vulkan driver at
all, the app used to stop ("Caskade couldn't draw its window"); now it
draws on the CPU. A real GPU (an RTX 3090 here, which Vulkan exposes
under Xvfb too) keeps vello_hybrid.

On the Ubuntu 24.04 VM (no GPU, plain VGA), the deb's
`/usr/bin/caskade --native` opened its window on the CPU path (the log
gives the reason: no adapter) and idled at 0.3% of a core. On the
Windows 10 VM (virtio-gpu, no Direct3D driver), the NSIS install's
`Caskade.exe --native` found only WARP ("Microsoft Basic Render Driver",
DX12), refused it as a software adapter and drew on the CPU, at 89 MB.
Its first start raises Windows Firewall's prompt for `caskade-native`
(publisher unknown: it isn't code-signed).

## Size and startup against the Tauri app

| Bundle | 0.1.4 (Tauri app only) | With the native app | Growth |
| --- | ---: | ---: | ---: |
| NSIS installer | 75.4 MB | 116.8 MB | +41.3 MB |
| deb | 133.1 MB | 214.1 MB | +81.0 MB |
| AppImage | 195.9 MB | 266.9 MB | +71.0 MB |
| dmg (Apple silicon) | 112.1 MB | 176.5 MB | +64.4 MB |
| dmg (Intel) | 117.9 MB | 185.9 MB | +68.1 MB |

The binaries themselves are about the same size. Linux x86_64, from the
deb:

| Binary | Size | Stripped | Stripped, xz |
| --- | ---: | ---: | ---: |
| `caskade` (Tauri) | 237 MB | 167 MB | 42 MB |
| `caskade-native` | 223 MB | 159 MB | 40 MB |

Release binaries aren't stripped today: `strip = true` in the release
profile would take about 64 MB off each, and the bundles' growth with
them. Every Tauri bundle also carries the desktop crate's other binaries,
`export-bindings` (155 MB) and `caskade-catalogue` (64 MB). Both are
separate decisions, not part of this issue.

Startup to a visible window, Linux, private 1440×900 Xvfb, fresh profile,
median of five: Tauri 378 ms, native 324 ms, and native through
`caskade --native` 350 ms. The Tauri window shows once its page has
loaded, while the native one may map before its first frame, so this
counts a visible window, not first paint. BLITZ-0's first-frame numbers
on the real library are in [`blitz-0.md`](blitz-0.md).

## Left for the user (#120)

- The release workflow on a test tag: the real `prepare` → `build` →
  `sign` → `verify` run with the native app inside, and an update from the
  previous release.
- Code signing. The bundles aren't code-signed or notarised today
  (`docs/RELEASING.md`), so neither app is. macOS signing and notarisation
  need an Apple Developer ID certificate and an App Store Connect key,
  which the workflow doesn't have. Windows Authenticode needs a
  certificate too.
- Machines without a GPU on real hardware: WARP on Windows, llvmpipe on a
  Linux desktop, a macOS VM.
