This source inspects `/Applications`, `~/Applications`, and their `Games`
subfolders. Directory traversal, Info.plist bytes/values/depth and classifier
work have bounds. It reuses the shared XML/binary plist parser and classifier;
no Apple command, app or launcher is invoked.

A game LSApplicationCategoryType supplies Certain evidence. Other bundles need
classifier evidence. Known launcher bundle IDs are excluded before category
classification (Heroic itself declares the games category). Executables and
ICNS paths must remain inside their bundle. Missing executables are Broken/Maybe;
malformed bundles warn while healthy apps remain visible. Distinct installed
copies remain separate, even with the same CFBundleIdentifier.

`Contents/_MASReceipt` marks Mac App Store-family provenance, including possible
Apple Arcade apps. Its presence cannot prove a store product ID or Arcade
subscription entitlement; discovery does not read personal receipt contents.

Original implementation; format references:

- Apple Info.plist categories: https://developer.apple.com/documentation/bundleresources/information-property-list/lsapplicationcategorytype
- Apple bundle keys: https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/CoreFoundationKeys.html
- Heroic bundle ID/category (configuration facts only, no GPL code ported): https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/blob/main/electron-builder.yml
- itch/kitch bundle ID formula: https://github.com/itchio/itch/blob/master/release/common.js
- Steam bundle identity observed in Valve's Mac support diagnostics: https://steamcommunity.com/discussions/forum/2/4751949102179723309/
- Epic/GOG/Battle.net bundle preference and saved-state identities: https://github.com/Homebrew/homebrew-cask/blob/master/Casks/e/epic-games.rb , https://github.com/Homebrew/homebrew-cask/blob/master/Casks/g/gog-galaxy.rb , https://github.com/Homebrew/homebrew-cask/blob/master/Casks/b/battle-net.rb
- Riot bundle metadata: https://doesitarm.com/app/install-league-of-legends-eune/

Linux tests author both XML and binary Info.plists and synthetic receipt markers;
native macOS category, signed receipt, filesystem and launch parity remain on #71.
