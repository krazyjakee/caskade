Ubisoft Connect discovery reads `SOFTWARE\Ubisoft\Launcher\Installs\<id>`
in both registry views. `Uplay Install <id>` uninstall entries provide titles
and recorded uninstall commands. Missing titles fall back to the launcher's
`configurations` cache in LocalAppData or the launcher installation directory.
Launch targets use `uplay://launch/<id>/0`; no launcher or shell is executed.

The configurations reader checks protobuf lengths/varints/field counts before
allocating records, reads only the product identity and YAML title fields,
resolves default localization keys and rejects YAML anchors/aliases/custom tags.
File/field/string/node/depth budgets bound work. Cache corruption warns without
hiding installed registry games. Cached library products and DLC never become
installed games by themselves. Missing install folders remain Broken/Maybe.

Tests create synthetic two-game registry/file trees and protobuf/YAML cache
bytes at runtime. They exercise IDs, URIs, both registry views, uninstall aliases,
missing/corrupt data, cancellation, and fixture symlink confinement. Native
Windows parity and current real launcher cache behavior are deferred on #71.

Original implementation, no code ported. Format facts were checked against:

- Playnite UplayLibrary, MIT: https://github.com/JosefNemec/PlayniteExtensions/tree/master/source/Libraries/UplayLibrary
- Cache wire field numbers: https://github.com/JosefNemec/PlayniteExtensions/blob/master/source/Libraries/UplayLibrary/Models/LocalCache.cs
- Product YAML schema: https://github.com/JosefNemec/PlayniteExtensions/blob/master/source/Libraries/UplayLibrary/Models/ProductSchema.cs
- BoilR, Apache-2.0: https://github.com/PhilipK/BoilR/blob/main/src/platforms/uplay/platform.rs
