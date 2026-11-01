# macOS app bundles

`AppHandler` proposes static extraction for a directory identified as
`Format::AppBundle`, or a listed ZIP containing a top-level `.app` Info.plist.
`AppExtractor` accepts an explicit source map and uses the executor's staging
capability. `ArchiveExtractor` and `HybridExtractor` route `macos-app` to it.
The builtin probe registry identifies `.app` directories; minimal probe engines
retain their generic directory behavior.

Folder copies preserve regular-file and directory modes (without setuid/setgid),
relative framework links, and Unix xattrs. macOS link xattrs use an `O_SYMLINK`
file descriptor with final-component following disabled. Quarantine attributes
are retained. No `xattr -d`, shell script or bundle executable runs during
installation. The native launch test runs the genuine fixture only when
explicitly selected on macOS.

ZIP extraction preserves its Unix modes and symlinks. Bounded AppleDouble v2
metadata restores FinderInfo, resource forks and ATTR-table xattrs. Unsupported
filesystem attributes fail explicitly; they are not silently removed. Metadata
never follows a symlink to modify its target. Linux privilege-bearing capability
and ACL attributes are rejected. The extracted AppleDouble records remain
available alongside the copied data.

`parse_info` accepts XML and binary plists, bounding bytes, object-table offsets,
reference expansion and nesting before the plist value is constructed. Metadata
includes the display name, bundle ID, executable and ICNS path. `discover` uses
the shared discovery implementation to normalize the icon to PNG.

Linux structure, golden trees, metadata, modes, symlinks, xattrs, quarantine,
AppleDouble bounds and hostile plist tests:

```sh
flock /tmp/caskade-heavy.lock cargo test -p caskade-installer --all-features --test it macos_app::
```

Native macOS acceptance (issue #22 runner):

```sh
flock /tmp/caskade-heavy.lock cargo test -p caskade-installer --all-features --test it macos_app::native_macos_installed_zip_launches_and_framework_links_survive -- --ignored --exact
```
