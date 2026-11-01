# Third-party code in caskade-discovery

## Licensing rule

Code from permissively licensed projects (MIT, Apache-2.0 and similar) may be
ported. GPL projects (Steam ROM Manager, Lutris, Heroic, legendary, Cartridges
and any other copyleft project) are read only to learn file locations and
formats: no code, comments or tests are copied from them. Caskade is
MIT-licensed (`LICENSE`), and this rule keeps ported code compatible with it.

Crates used as ordinary dependencies (for example `steamlocate`) carry their
own notices and are not listed here.

## Header comment for ported files

Every file that contains ported code starts with:

```rust
// Ported from <project> (<repository URL>), <path in that project> at <commit>.
// Copyright (c) <year> <holders>. Licensed under <license>; see THIRD-PARTY.md.
// Changes: <one line on what was adapted>.
```

and the project gets an entry below with its full copyright and license text
(or, for Apache-2.0, the license name plus any `NOTICE` file contents).

## Notices

None yet.
