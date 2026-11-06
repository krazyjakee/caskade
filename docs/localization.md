# Localization

Caskade is translated with gettext (the decisions are in the Localization milestone's tracker, #221). The English text is the message id, with a context (`msgctxt`) where the same English means different things. Each domain's template is generated from the code and committed as `locales/<domain>.pot`, and each language is `locales/<lang>/<domain>.po`, named by its BCP 47 tag (`sv`, `pt-BR`). Translators work through pull requests. Missing and fuzzy entries show English.

## Rust crates: the `core` domain

The `core` domain covers the text the Rust side makes for people: `caskade-core`, the Tauri adapter (`apps/desktop/src-tauri`) and the tray. `caskade-installer` and `caskade-discovery` stay out of it: their messages are for developers, or the core maps them by code (#214). The installer's questions, warnings and progress reach Activity, so `install/job_text.rs` says them again from the core's catalog each time a job is read, by the installer's English; a test fails when the installer stops saying a text it knows.

### Runtime

`crates/caskade-i18n` is a small pure-Rust gettext runtime: a PO reader and writer, the `Plural-Forms` expression evaluator, catalog lookups with context and plurals, locale negotiation and the extractor. It has no libintl, so the deb, AppImage, NSIS and macOS bundles and the Windows and macOS cross lints need no C gettext. Its only runtime dependencies are `sys-locale`, for the OS's languages, and `tracing`.

The crates considered (#212):

| Crate | Why not |
|---|---|
| `gettext-rs` 0.8 | Binds the C libintl. |
| `tr` 0.1.11 | Its default backend is `gettext-rs`. Its pure-Rust PO backend (through `rspolib`) uses fuzzy entries, keeps one translator per crate, and its extractor `xtr` is a separate AGPL binary installed with `cargo install`. |
| `gettext` 0.4 | Pure Rust, but reads only compiled MO files, so the build would need `msgfmt` or a PO-to-MO compiler; no negotiation or switching. |
| `rust-i18n` 4 | Keys in YAML, JSON or TOML, not gettext PO. |
| `polib` 0.3 | Reads and writes PO, but doesn't evaluate plural rules or look messages up. |

What Caskade needs from gettext is small (PO in, lookups with context and plurals, fuzzy left out), so a crate of its own costs less than adapting any of these.

`caskade_core::i18n` holds the core domain:

| Macro | gettext | Example |
|---|---|---|
| `tr!` | `gettext` | `tr!("Show Caskade")` |
| `trn!` | `ngettext` | `trn!("One game", "{n} games", count)` |
| `trp!` | `pgettext` | `trp!("folder browser place", "Home")` |
| `trnp!` | `npgettext` | `trnp!("tray", "One game running", "{n} games running", count)` |

- Arguments are named placeholders, filled in after the lookup, so a translation can reorder them: `tr!("Couldn't start {game}", game = title)`. The plural macros fill `{n}` with the count. `{{` and `}}` are literal braces.
- The macros return a `String` in the active language. Inside the core write `crate::tr!`; elsewhere `caskade_core::tr!`. Extraction reads only string literals, so the message must be one.
- Catalogs are compiled into the binary, parsed the first time their language is chosen. `caskade-i18n`'s build script finds every `locales/<lang>/core.po` and lists it, through `include_str!`, in `caskade_i18n::bundled::CORE` (the core's `CATALOGS`), so adding the file ships the language. A release needs no locale files on disk.
- The core owns the language. `i18n::set_locale` takes a BCP 47 tag or `system` (the OS's languages, through `sys-locale`; POSIX names such as `pt_BR.UTF-8` are normalized). It falls back through the tag's less specific forms (`pt-BR`, then `pt`) and the next preferred language, then English, and skips a catalog that can't be read. The app sets the OS language at start, then the Language setting's (`core.general.language`, `system` by default) before its window loads: `i18n::apply_setting`, then `i18n::follow_setting` switches on each change. `i18n::resolve` tells what a value would switch to without switching (System's language in Settings).
- `i18n::subscribe` follows the language. The tray rebuilds its menu labels on a change, without a restart (`tray.rs`). Lookups and switches are thread-safe.
- Text kept for later is stored as a message, not as rendered text (#214): `msg!`, `msgn!`, `msgp!` and `msgnp!` take the `tr!` macros' arguments and make an `i18n::Message` (an argument can be a message too), rendered in the language in use each time it is read. Notifications keep their title and body as messages beside their English (`title_message`, `body_message`, migration 0018); a failed update check keeps its message in its `error` column (`i18n::Text::store` and `load`), carried there by an `i18n::TextError` inside the error. Rows from before keep their English. The core says a failed install's summary again from its kind (`install::failure_message`) each time a job is read, and translates the core settings schemas' titles and descriptions as it serves them (`settings/schema_text.rs`, context `settings`). Errors from dependencies stay as they come inside a translated sentence, and logs stay English.

### Extraction

`just i18n` runs `caskade-i18n extract` (`cargo run -p caskade-i18n --features extract --bin caskade-i18n -- extract`). It reads every `.rs` file under `crates/caskade-core/src` and `apps/desktop/src-tauri/src` as Rust tokens, so calls inside other macros' arguments count and comments don't. It writes `locales/core.pot`, then merges it into each `locales/<lang>/core.po`, as `msgmerge` would without fuzzy matching: translations stay, new messages come in empty, removed ones go, and a message whose English plural changed is marked fuzzy. No gettext tools are needed.

The output is deterministic. References name files without line numbers and the template has no creation date, so extracting twice changes nothing and moving code doesn't make the template stale. `--check` writes nothing and fails when a file is out of date.

Tests:

- `cargo test -p caskade-core --test it i18n::` fails when `locales/core.pot` is stale and checks that extraction and merging are stable.
- `caskade_core::i18n`'s unit tests read every bundled catalog, and lint it (placeholders a translation adds, plural forms the language lacks); `caskade-i18n`'s fail when a `locales/<lang>/core.po` isn't bundled.
- `cargo test -p caskade-i18n` covers the PO format, plural rules, lookups, context, fallback, negotiation and switching.
- The adapter's `tray::tests` switch the language on Tauri's mock runtime and check the menu's labels.

### Translating

The translator's guide is [Translating Caskade](translating.md): starting a language (`pnpm i18n:extract --add <lang>`, then `just i18n`), editing, trying it and the checks. Swedish (`sv`) is the first language.

### Not yet covered

The native dialogs the Rust side opens itself, OS notifications and the macOS app menu, and the bundles' translated `.desktop` entries, `CFBundleLocalizations` and NSIS and WiX installer languages.

## Webview: the `ui` and `app` domains

The webview uses [Lingui](https://lingui.dev) 6 with gettext PO catalogs (`@lingui/format-po-gettext`, so plurals are native `msgid_plural` / `msgstr[n]` entries that Poedit and Weblate understand). There are two domains: `ui` is `@caskade/ui` (`packages/ui/src`), so plugins and Storybook get its translations too, and `app` is the rest of the webview (`apps/desktop/src`). Tests, stories and `src/test/` are not extracted.

`packages/i18n` (`@caskade/i18n`) holds the pipeline:

| Module | What it is |
|---|---|
| `@caskade/i18n` | The runtime: the global `i18n` (the handle for code outside React), `locales`, `loadCatalog`, `activateLocale`, `requestedLocale` (`?lang=`), `translated` (how much of each language is translated, every domain together, `core` included, counted at build time from `locales/`; the Language setting shows it). |
| `@caskade/i18n/react` | What the macros compile `useLingui()` and `<Trans>` to: Lingui's own, falling back to the global `i18n` outside an `I18nProvider` (a unit test rendering one component). |
| `@caskade/i18n/vite` | The Vite plugin, in the app's Vite and Vitest configs, `@caskade/ui`'s Vitest config and Storybook's. The Lingui configuration, PO reading and catalog compilation for the scripts. |

### Marking strings

```tsx
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";

// Strings defined outside components are lazy descriptors, translated where they're shown.
const label = msg({ message: "Home", context: "navigation", comment: "The sidebar link to the home view" });

function Status({ waiting, version }: { waiting: number; version: string }) {
  const { t } = useLingui();
  return (
    <>
      <span>{t(label)}</span>
      <span>{plural(waiting, { one: "# task needs attention", other: "# tasks need attention" })}</span>
      <Trans>Caskade v{version}</Trans>
    </>
  );
}
```

- Inside components use `useLingui()`'s `t`, `plural` and `select` (they re-render when the language changes) or `<Trans>`. `t` from `@lingui/core/macro` reads the global `i18n` and is for code outside React.
- `comment` becomes a translator comment (`#.`), `context` the `msgctxt`.
- `select` has no gettext equivalent and stays ICU text in the PO file; prefer two messages.
- Placeholders get the name of what fills them: `` t`Open ${{ title: game.title }}` `` gives `Open {title}`, where a bare `game.title` would give `{0}`. A plural's count is part of its id, so the same English plural must use the same name everywhere (`plural({ total: count }, …)`), or msgmerge rejects the template as a duplicate.
- The macros are transformed natively by `@lingui/native-tools` (no Babel or SWC plugin). They keep the English text in the code (`descriptorFields: "message"`), so English needs no catalog and anything a catalog lacks shows English.

### Extraction and catalogs

`pnpm i18n:extract` (part of `just i18n`) writes `locales/ui.pot` and `locales/app.pot`, then merges them into each language's `locales/<lang>/{ui,app}.po` with GNU gettext's `msgmerge`: translations stay, a changed message comes back fuzzy with its old translation (and the old English as `#|`), and a removed one stays as an obsolete `#~` entry. It needs `msgmerge` and `msginit` (`gettext` on Linux and Homebrew, MSYS2 on Windows). `pnpm i18n:extract --add <lang>` starts a language. Running it twice changes nothing: references name files without line numbers, and the template's header is kept.

- `pnpm i18n:check` (`scripts/check.ts`, the webview's half of `just i18n-check`) fails on a stale template, a PO file `msgfmt --check` rejects or a translation that drops or changes its English's placeholders or tags (`src/check.ts`), naming the file, line and entry, then prints each language's completion per domain. The `@caskade/i18n` test `the committed templates are current` catches a stale template without gettext.
- `pnpm i18n:compile` compiles every catalog the way the build does, prints how much of each language is translated, and fails on a translation that doesn't compile.

Catalogs compile at build time: `virtual:caskade-i18n/catalogs` lists a lazy loader per language and domain, and each compiles from its PO file into a chunk of its own, so a release bundle carries none until a language is chosen and only the active one is loaded. A fuzzy, obsolete or empty entry, or a translation that doesn't compile, is left out and shows English. A `.po` file imported directly (`import { messages } from "./shell.sv.po"`) compiles on its own, without the template.

The pseudo-locale `en-XA` is made from the templates: accented, bracketed and about 35 % longer, to find unmarked strings and labels that overflow. Dev servers, tests, debug app builds (`TAURI_ENV_DEBUG`) and Storybook have it; release builds don't. Try a language in `just web` with `?lang=en-XA` or `?lang=sv`.

### Where it's set up

- `main.tsx` asks the core for its language (`locale_current`, through `api/locale.ts`), activates it before the first render and wraps the app, first-run setup included, in an `I18nProvider` and a `LocaleProvider` (`src/locale/`). The core resolves the Language setting and is the only source of truth; `LocaleProvider` follows its `LocaleChanged` event, loads the language's catalogs, activates them and sets `<html lang>`, so both shells switch live. Settings → General → Language and first-run setup's first step (`onboarding/LanguageStep.tsx`) only save `core.general.language`. The browser preview has no core: it resolves the setting itself, with `navigator.languages` as the OS's, and `?lang=` in place of those.
- `src/test/harness.tsx`: `renderApp(path, { locale, messages })` renders the app in a language, with messages from a test PO file or `await loadCatalog(locale)`. `src/test/locale.test.tsx` is the example. Its Language setting switches the language as the core would (`systemLanguages` stands in for the OS's); `src/locale/language.test.tsx` switches both shells. `src/test/pseudo.test.tsx` renders both shells, the title bar's, account and game menus in the pseudo-locale and fails on English text, aria-labels, titles or placeholders.
- Storybook's Language toolbar loads `@caskade/ui`'s catalog for the chosen language (`apps/storybook/src/Locale.tsx`).

### Formatting, sorting and search (#218)

- Numbers, sizes, durations, dates and times go through `apps/desktop/src/api/presentation.ts`, never `toLocaleString()` or `Intl.*(undefined, …)`. It formats with `Intl` in the active language (`formatLocale()`: the core's, in the OS's region when the OS speaks that language, so English in the UK keeps en-GB's dates). Sizes use `Intl`'s units (`1,5 GB`, `1,5 Go`); binary sizes (`formatBytes`, KiB, GiB) translate their symbols as messages (context `byte unit`). Relative days come from `Intl.RelativeTimeFormat`. The formatters read the language when they run, so a component that calls `useLingui()` re-renders in a new one.
- The console clock (`shell/console/HintBar.tsx`) uses the language's 12 or 24 hours unless Settings → General → Time format (`core.general.timeFormat`: `system`, `12h`, `24h`) picks one.
- Titles sort in the language's alphabet: the core registers the `caskade_title` collation (ICU4X `icu_collator`) on the library's connections and `ListGames` sorts by it, so in Swedish Å, Ä and Ö come after Z. `title_letters` files each title under the letter the language counts it as (É under E in English, Ä under A in German, Å under Å in Swedish), which the alphabet bar and console letter paging step through. Nothing indexes the collation, so a language switch reorders the next query. The webview's fixtures and `compareTitles` use `Intl.Collator`.
- Search folds case and accents: the core's trigram index already removes diacritics, and words under three letters match through a GLOB whose letters include their accented forms (`db/collation.rs`); the webview's `foldSearch` / `findFolded` do the same for the search box's highlight and the fixtures.
- The on-screen keyboard (`input/OnScreenKeyboard.tsx`, `keyboardLayouts`) has a layout per language: AZERTY in French with é è à ç on the letters, QWERTZ in German, å ä ö in Swedish; the language's other letters are a row of the symbols. Other languages get English's QWERTY.

### Translating the webview

See [Translating Caskade](translating.md): a new language needs no code change.

### Not yet covered

The rest of the markup (#215–#217). `just i18n-check` runs only `msgfmt --check` on plugins' and themes' catalogs; their placeholders are the core's (`{name}`, with gettext plurals), so the core's placeholder check fits them.

## Plugins and themes (#219)

A plugin or a theme ships its own catalogs, `locales/<lang>.po` in its folder, in a domain named after its id, with the English text as the message id. The host reads them with `caskade-i18n` (`caskade_plugin_host::locales`, `caskade_theme::Locales`) in the core's language: a plugin's name, description, titles and settings schema, its UI half's `t` and `<Trans>` (`@caskade/sdk`, not Lingui: the catalog comes from the host), and `host.locale` for its backend; a theme's name and description. A catalog that can't be read is a warning on that plugin or theme and its English shows. See [plugins](plugins/README.md#translations-219) and [theming](theming/README.md#translations-219).
