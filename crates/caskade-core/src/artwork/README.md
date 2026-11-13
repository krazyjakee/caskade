# Artwork cache and scheme (#106)

`ArtworkCache::new(paths.cache.join("artwork"))` opens the cache directory.
`import_bytes(bytes)` and (with `net`) `download(url)` share one bounded
pipeline. Hashing, file I/O, decoding, resizing and WebP encoding run on
the shared FIFO resource queue, with one heavy stage active across services. Input is
limited to 16 MiB, PNG/JPEG/WebP only, 8192 pixels per axis and 16 megapixels;
decoder allocation is capped too. SVG and executable formats are rejected.

The SHA-256 of input bytes names immutable content. An OS lock serializes
writers, including independent app instances. Atomic capability-relative file
publication writes original WebP plus 96/list, 320/grid and 960/details WebP
thumbnails, then publishes a completion manifest. A completed entry is reused
across restarts; an interrupted entry is regenerated. Small icons are not
upscaled, and alpha is preserved. URL-to-hash mappings persist without recording
URLs or credentials, so repeated downloads use cached content. Requests have
byte/time/redirect limits; errors remove request URLs. The network fetch runs
outside the queue, so downloads of different URLs overlap; one URL downloads
once at a time per process. `download_metadata(url)` is for image URLs a
metadata server sent (IGDB, and RomM URLs off the RomM server): the URL, every
redirect and every DNS answer must be a public address, never loopback,
private, link-local or unspecified ones.

`CachedArtwork::record(game_id, kind, source)` creates an Artwork entity with a
new opaque UUID and relative cache paths. Multiple game/kind/source associations
may share the same content hash. Persist associations using LibraryStore's
artwork repository; deleting an association does not delete shared cache bytes.

UI grids select `thumb-320.webp`, lists select `thumb-96.webp`, and details select
`thumb-960.webp`. Missing thumbnail metadata means placeholder artwork, never an
original-image fallback. Pass the relative thumbnail path to Tauri's
`convertFileSrc(path, "caskade-asset")`. Linux/macOS use
`caskade-asset://localhost/<hash>/<variant>.webp`; Windows uses
`http://caskade-asset.localhost/...`. Tauri percent-encodes separators: the core
resolver decodes once, then checks the exact hash/variant grammar. Traversal,
double encoding, foreign hosts, arbitrary extensions and cache manifest/lock
files are rejected. Capability-relative opens prevent symlink escapes even
when files change between checks. No ambient path reaches the webview.

`artwork::serve(&cache, uri)` runs reads off-thread and returns a framework-free
HTTP result. The native app (`apps/native/src/library/art.rs`) reads covers
through it. Errors have empty bodies and expose no filesystem paths. Core
protocol tests cover encoded routes and symlinks.
