# Caskade — Web Games Plan

Status: W1, W2 and W4 done, offline bundles on Linux only; W3, W5, ad SDK stand-ins and offline bundles on Windows and macOS planned · Last updated: 2026-10-29

How Caskade adds, shows and plays games that run in a web page: hosted games (itch.io, Newgrounds, Poki, CrazyGames, a game's own site), downloaded HTML5 bundles and Flash. It follows the conventions in `TECH-PLAN.md`.

---

## 1. Goals and non-goals

**Goals**

- The Library has a **Web** tab. There, **Add web game** takes a page URL and nothing else.
- Web games show in the Web tab only: the Library's other tabs, their counts, the platform list and smart collections leave them out (`db/query.rs` `NOT_WEB_ONLY`), and so do Home's shelves and the search. A game with a release of another kind as well shows in both; a manual collection keeps a web game put in it, and Settings lists hidden ones.
- A web game plays in Caskade's own game window, not the system browser, so play time is tracked, console mode is fullscreen and the guide button brings Caskade back, as with any other game.
- **Play offline** (optional) saves the game into a storage root as a neat local bundle: the game and its files, without the site around it, its ads or its trackers.
- Where a site has a simple route to the game's files (a direct file, itch.io), Caskade takes it. Elsewhere it guides the user through a short play-through and keeps what the game loads.
- The Web tab shows the games as a mosaic of rounded square tiles, a few of them large.

**Non-goals (for now)**

- Logins, purchases and cloud saves on the sites themselves.
- Patching games' code (HTTPS hosts baked into `.wasm`, obfuscated scripts). The offline check reports what is missing instead.

---

## 2. Model

- `LaunchSpec::Web { url }`: the game's page, `http` or `https`. `launch_kind` is `web`; `GameFilter::Web` lists games with such an entry, and `LibraryStats.web` counts them.
- Web games are on the `browser` platform (`platforms.toml`: "Browser (Flash/HTML5)", IGDB 82).
- The entry's source is `web` and its `external_id` the normalised URL (no fragment, no credentials), so adding the same page twice finds the first.
- An offline bundle (W4) is a managed install: the same entry gets a `Managed` location, and the folder holds `caskade-web.json` (the start URL, the game's size when its page gave one, and, for every URL kept, its file, status and the headers that matter: `Content-Type`, `Content-Encoding`) and the files. Without a location the game plays online.

## 3. The game window

`LaunchSpec::Web` resolves to Caskade's own executable in player mode:

```
caskade --web-game <url> --data-dir <data>/web/<entry id> [--fullscreen]
```

- A separate process, so the launch pipeline needs nothing new: the session is the process, recovery, Stop and the game focus logic work as for any game, and a crashing page can't take Caskade down. The window's process alone is the game (`TrackingMode::OriginalProcess`), so the session ends the moment it closes, without the exit grace; WebKit's helper processes don't count. Closing the window, or Stop's SIGTERM, ends its web process too: left alone, a web process busy with the game runs on for up to 10 s.
- `main` dispatches on `--web-game` before Tauri starts: the window is plain tao + wry (the versions Tauri already builds), with **no IPC handler at all**, so the page can't reach the core.
- Each game has its own webview data folder, so its saves (localStorage, IndexedDB) stay its own and survive restarts.
- New windows and pop-ups (`target=_blank`, `window.open`) are refused, online and offline: a game's links ("More games", the developer's site) open nothing, in the game window or the system browser. Top-level navigation is left alone: WebKitGTK and WKWebView ask the same handler for iframes, and blocking those would break most games.
- In desktop mode it opens over Caskade's window (`--geometry x,y,w,h`, the main window's place while it's on screen, title bar included), and Caskade stays where it is under it: `core.general.onLaunch` doesn't minimize or hide it for a web game. The guided capture opens there too. A game whose size is known opens at that size instead (CSS pixels at the screen's scale, shrunk to 90 % of the screen if bigger), centred over Caskade's window: the size the page gives its frame or movie (`width`/`height` or inline `style`), passed to the capture as `--size WxH` and kept in the bundle, else a Flash movie's stage from its header (`caskade_core::web::swf`; not LZMA `ZWS` movies). A game played online, as its whole page, keeps the window above. A window the user resized opens at their size next time, centred the same way (`web_player/size.rs`): closing or stopping it keeps the size in logical pixels as `caskade-window.json` in the game's data folder (`--data-dir`, deleted with the game), unless it is maximized or fullscreen or still the size it opened at. A capture always sizes itself to the game.
- Fullscreen in console mode (`core.console.active`). A page's own fullscreen button works in either mode.
- It starts with `JSC_useSharedArrayBuffer=1` (W4): WebKitGTK keeps `SharedArrayBuffer` off otherwise, and Godot 4's threaded exports need it.

## 4. Adding a web game

**Add web game** on the Web tab opens a small dialog: **Game page** (a URL) and **Play offline** (W4). For an online game:

1. Caskade fetches the page (15 s, 2 MB) and reads its title (`og:title`, then `<title>`, cut at ` | `, ` - `, ` – `, ` — `), its description, and the pictures it offers, the most dedicated first: its own preview (`og:image` with its declared size, `twitter:image`, `itemprop="image"`, `<link rel="image_src">`, JSON-LD `image`, `thumbnailUrl` and `screenshot`, leaving out publishers' logos); then, when it has none of those, its oEmbed `thumbnail_url` and its web app manifest's pictures (square icons largest first, then the rest and its screenshots); then its icons, largest first, and last the site's `/favicon.ico`, linked or not. Icons may be ICO files (the artwork cache reads PNG, JPEG, WebP and ICO, and stores WebP).
2. The tile picture (cover) is the first that downloads and that no other game already has as its cover: one studio's games often share a site icon. When every picture is taken, the first is used anyway. The game page's hero is the first wide picture (half as wide again as tall), or the tile picture when that is wide; the icon is the tile picture when it's square, else the first square one. All are stored with source `web`.
3. A page that can't be fetched is still added, titled after its host, without a picture. A store's page that says it has no game to play is refused, as is Play offline for it: an itch.io game page (`body[data-host="itch.io"][data-page_name="view_game"]`) without an HTML5 embed has to be bought first (Buy Now) or is a download (`page::Unplayable`).
4. Adding a page already in the library looks for its pictures again and replaces the ones an earlier look stored (source `web`), so a game added before a page got its preview, or before Caskade read manifests, can be fixed by adding it again.

A web game's page links to its page on the web as **Official site** (the web entry's `LaunchSpec::Web` URL), so the live version stays a click away, also when the game plays from an offline copy.

Scans never find web games, so a web game's menu has **Remove…** where other games have Hide from library. After a confirmation, `WebGames::remove` deletes the offline copy, if any, and the game window's data folder (cookies, local storage, the game's saves), then the web entry with its play sessions; a game with no other entry leaves the library. It refuses while the game runs.

## 5. The Web tab's mosaic

A dense CSS grid (`grid-auto-flow: dense`) of rounded square tiles. A tile is large (2×2) when any of these holds, and the first game on the tab is extra large (3×3) when it does too:

- the game is a favourite;
- it was played in the last 7 days;
- its play time is in the top quarter of the tab's games (and over 0).

A per-game size set by hand (game menu → Tile size) comes later and overrides these.

## 6. Offline bundles (W3, W4)

Routes, tried in order:

1. **Direct file**: a URL ending in `.zip`, `.html` or `.swf` downloads through the installer's fetch engine. A `.swf` plays through Ruffle (an emulator definition for `adobe_flash` and `browser`).
2. **itch.io**: only the game's iframe (`html-classic.itch.zone/…`), captured as below; it has no ads. Caskade doesn't ask for itch API keys.
3. **Newgrounds**: the `.swf` (Ruffle), or the HTML5 game's iframe.
4. **Portals** (Poki, CrazyGames, …): the game's iframe only, ads blocked, and stand-ins for the portals' ad SDKs that report every ad as finished at once, so games don't hang waiting for one.
5. **Anything else**: the largest iframe or canvas on the page, captured.

**Guided capture.** The game opens in the game window with one line along the top:

> Play for a minute so Caskade can save the game. · 142 files · 38 MB · **Done**

Everything the game loads is kept: the game window's traffic goes through a proxy in its own process (a MITM proxy on loopback with a CA made for the session and never trusted outside that window), which fetches upstream, verifying TLS itself, and keeps every response. Requests to ad and tracking hosts are refused and not kept: a short built-in host list first, Brave's `adblock` engine with lists downloaded at run time later (EasyList's licence keeps it out of the app). After **Done**:

1. "Checking it works offline…": the bundle is played again through the same proxy with upstream fetches turned off; every request the bundle can't answer is a missing part.
2. "Ready to play offline." or "A few parts are still missing." with **Play a bit more** and **Save anyway**.

The bundle is written into the chosen storage root as an install job (Activity). It plays through the same proxy, which answers from the bundle at the original URLs: the page keeps its origin, so its saves, absolute URLs, workers and iframes work, and every response gets COOP, COEP and CORP for cross-origin isolation. Afterwards, a bundle that asks for a file it doesn't have fetches and keeps it while online, so it completes itself.

**Built (Linux).** Play offline is in Add web game and in a web game's menu, which then has Remove offline copy (the game plays online again). Both ask where to keep the game with the install root picker; `WebGames::save_offline` finds the game's frame (itch.io's `data-iframe`, else the page's Flash movie (an `<embed>`'s or `<object>`'s `.swf`), else the page's largest iframe, lazy ones' `data-src` and `style` size included and video players (YouTube, Vimeo, …) left out, else the page), opens the capture into a free folder named after the game, and makes the saved bundle the web entry's managed location; a capture closed before it was saved leaves nothing. A bundle plays through the proxy and keeps what it lacks while online, so it completes itself. When the page builds its game with scripts (Poki, and portals like it), the capture plays the whole page and finds the game while it runs (`web_player/find.rs`): a script in every frame tells the page its address and size, and every second the window asks the page for its biggest visible frame, Flash movie or canvas, taking a frame's deepest inner frame that fills it (a portal's loader wraps the game). Ads, blocked hosts and video players don't count, and a canvas first means the page draws the game itself. On Done the copy starts at the game found (a Flash movie, or a frame from another site), at the size the page showed it, and drops what only the page loaded: the proxy remembers each kept URL's `Referer`, and the game keeps what its origin asked for, then what those origins asked for, until nothing new joins (URLs without a referer stay). A frame from the page's own site keeps the page as the start: all of it is the game's origin anyway, and some games refuse to run outside it (totaljerkface.com's Happy Wheels checks its parent page and reads the level from its address). POSTs are kept too, by URL and a hash of what they posted (`<url>#post=<hash>` in the manifest, whose `posts` names the last kept for each URL): games load levels that way (Happy Wheels' `get_level.hw`). Offline, a POST gets the response kept for the same body, else the last one kept for its URL; playing online asks the server first and falls back to it. Checked live with Happy Wheels, played with no network. The offline check then plays the game alone; Play a bit more goes back to the page, since some games load only there (Poki's sitelock and Cloudflare refuse its frame on its own). Checked live with Poki's Robo Cleaner Simulator: 44 files kept of 270, played with no network. An offline copy plays with Poki's SDK told there are no ads (`window.__poki_no_ads`, in every frame, `web_player/portals.rs`): the "hoist" SDK in Poki's game frames finishes `init()` and ad breaks only once poki.com's page answers, so alone a game stopped its clock and input for an ad that never came; checked live with Poki's Upgrade the CARS, played with no network. A `.swf` start plays on the game window's own page, loaded at the movie's address so it keeps its origin, in Ruffle's web build from jsDelivr, which the bundle keeps (`web_player/flash.rs`); checked live with Armor Games' GemCraft, played with no network. Root scans skip a folder holding `caskade-web.json`; Move and Uninstall don't apply to bundles. Checked live with a fixture loading every way a game does and with an itch.io game (Celeste Classic), played with no network. Not yet: the capture as an Activity job, the other portals' ad SDK stand-ins (CrazyGames, …), Brave's `adblock` engine, the rest of the simple routes (W3: direct files, Newgrounds' movies, Flash when playing online), and Windows and macOS.

**Spike** ([research/web-capture-spike.md](research/web-capture-spike.md)). On WebKitGTK 2.52, WebKit's resource API lists every request but returns 1 byte for most bodies (fetch, XHR, wasm, media, workers); the proxy keeps them all, HTTPS included. A custom scheme is a secure context but never cross-origin isolated; HTTP(S) with COOP and COEP is, and `SharedArrayBuffer` needs `JSC_useSharedArrayBuffer=1` either way. Windows (WebView2: `--proxy-server` and accepting the session CA) and macOS (WKWebView: proxy from macOS 14, TLS challenge not exposed by wry) are still to measure; offline bundles may start as Linux and Windows only. Still to try with real games: an itch.io Godot 4 game, a Unity WebGL game, a Poki game, a CrazyGames game and a GitHub Pages game.

---

## 7. Phases

| Phase | Delivers |
|---|---|
| **W1** | `LaunchSpec::Web`, the `web` filter and Web tab, Add web game for online games, the game window and play-time tracking |
| **W2** | The Web tab's mosaic |
| **W3** | Simple routes: direct files, itch.io and Newgrounds game iframes, Ruffle for Flash |
| **W4** | Guided capture: the capture bar, ad blocking, the offline check, bundles in storage, self-completing bundles (done, Linux); ad SDK stand-ins |
| **W5** | Offline bundles on Windows and macOS, RomM `browser` games, pad-to-key mapping and a Gamepad API shim fed by gilrs, per-game tile size |
