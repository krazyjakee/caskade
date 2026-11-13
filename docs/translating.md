# Translating Caskade

Caskade is translated with gettext PO files, kept in the repository. English is the source language: its text is in the code, and anything a language leaves untranslated shows in English. Translations come in through pull requests. A hosted translation service can come later: Weblate works on these same PO files, so adding one changes nothing for translators who use a PO editor.

How the pipeline works inside the code is in the [tech spec](TECH-SPEC.md#localization).

## The files

Each domain is a group of messages with a template of its own, `locales/<domain>.pot`, generated from the code. Never edit a template. Each language is a folder named by its BCP 47 tag (`sv`, `pt-BR`), with one PO file per domain.

| Domain | File | What it covers |
|---|---|---|
| `ui` | `locales/<lang>/ui.po` | The shared components (`apps/native/src/ui`): buttons, badges, dialogs. |
| `app` | `locales/<lang>/app.po` | The rest of the interface: the library, game pages, Settings, first-run setup. |
| `core` | `locales/<lang>/core.po` | Text from the Rust side: error messages, notifications, the tray menu. |

Plugins and themes have a domain each, named after the plugin or theme, in their own folder's `locales/<lang>.po` (#219). Translate them like the app's.

## Starting a language

From the repository root:

```sh
just i18n --add de   # starts locales/de/core.po, app.po and ui.po
```

The new files' header has `Language: de` and English's `Plural-Forms`, which you then correct for your language:

```
"Plural-Forms: nplurals=2; plural=(n != 1);\n"
```

`Plural-Forms` says how many plural forms the language has and which one a count takes. Copy it from [the gettext manual's table](https://www.gnu.org/software/gettext/manual/html_node/Plural-forms.html) or from Poedit's translation properties, then run `just i18n` again: a plural entry gets one `msgstr[n]` per form. Swedish and German have two forms, Polish three, Japanese one. (A language that has one of its files already gets the others with that file's `Plural-Forms`.)

That's all it takes to ship a language: the build finds every folder in `locales/`, and the Language setting lists it with how much of it is translated. No code changes.

## Translating

Open the PO files in [Poedit](https://poedit.net), Lokalize, Gtranslator or any text editor.

- The English is the `msgid`. A `msgctxt` tells apart the same English used for different things ("Home" the sidebar link, "Home" the folder). Comments starting with `#.` are notes from the developers, and `#:` names the file the text comes from.
- Keep every placeholder the English has, in whatever order your language needs: `{name}` and `#` (the count, in plurals) are filled in when the text is shown. Don't translate what's inside the braces.
- Keep the tags around words, such as `<0>` and `</0>`: they become links or emphasis. Move them with the words they wrap.
- In a plural entry, `msgstr[0]`, `msgstr[1]`, … follow the order of your `Plural-Forms`. A form that only ever means one may leave the count out ("En fil"); the others keep it.
- An empty or fuzzy entry shows English. An entry is marked fuzzy when the English changed since it was translated: check the translation, then clear the flag.
- `{{` and `}}` in the core's messages are literal braces.
- Entries starting with `#~` are obsolete: their English is gone from the code. They keep their translation, which comes back if the message does; leave them be.

When the code changes, the templates change with it. Run `just i18n` after pulling, so your files get the new messages; the old translations stay.

Swedish (`sv`) is the first language. Its first draft is machine-translated and needs a native speaker's review.

## Trying a language

- In the app: `just dev`, then Settings → General → Language (first-run setup asks too). The language switches at once, the tray menu included.
- From a terminal: `CASKADE_LANG=sv cargo run -p caskade-native` (a debug build). `CASKADE_LANG=en-XA` is the pseudo-locale: every translatable text accented and lengthened, which finds text that isn't marked or doesn't fit.

The app compiles the catalogs in, so rebuild it after changing a PO file.

## Checking

```sh
just i18n-check
```

It runs what CI runs (the `rust` job of `.github/workflows/ci.yml`) and fails, naming the file and the entry, on:

- a template or PO file that doesn't match the code (run `just i18n`);
- a PO file `msgfmt --check` rejects (a broken header, a missing plural form, a line ending that differs from the English), plugins' and themes' included;
- a translation that drops, renames or adds a placeholder (`{name}`, `#`) or a tag (`<0>`, `</0>`).

Then it prints how much of each language is translated, per domain. It needs GNU gettext's `msgfmt` (`gettext` on Linux and Homebrew, MSYS2 on Windows).

## Sending a translation

Open a pull request with your `locales/<lang>/` files. Run `just i18n-check` first if you can; CI runs it either way. Partial translations are welcome: a language ships as soon as it has a file, showing how complete it is.

## For developers

Translators see only the English and what you add to it.

- Mark the interface's text (`apps/native`) with `caskade_ui_i18n`'s `t!("Quick menu")`, `t!("Stop {title}", title = name)`, `tp!("size", "Unknown")`, `tn!("# game", "# games", count)` and `tnp!`, in ICU syntax: `#` is the count. Rich text goes through `trans!("<0>{guide}</0> opens the quick menu", guide = glyph; 0 => |inner| rsx! { kbd { {inner} } })`. A test fails on a text literal with words in `rsx!`. Errors, the tray and dialogs keep the core's `tr!`, `trn!`, `trp!` and `trnp!`, where `{n}` is the count.
- English kept in one place and translated where it's shown (`caskade_ui_data::labels`) is marked with `t_noop!("…")`, which `just i18n` extracts like `t!`.
- Add a translator comment when the English alone can leave someone guessing: what a placeholder holds, where the text appears, how much room it has, or whether a word is a verb or a noun. `note = "…"` first in a `t!` call writes it as a `#.` line. Name placeholders by what fills them (`{title}`, not `{0}`).
- Add a `msgctxt` only when the same English means different things in different places, so each needs its own translation: `tp!("navigation", "Home")`, `trp!("folder browser place", "Home")`. Two places that mean the same thing share one message. A context isn't a comment: changing it makes every translation of the message start over (the old one stays as an obsolete entry).
- The core's extractor doesn't read Rust comments, and `tr!` takes no note. Where the English alone is unclear there, word the context (`trp!`) for the translator.
- Mark whole sentences, with placeholders, not pieces joined in code: word order differs between languages. Use plurals (`tn!`, `trn!`) for counts, never `if count == 1 { … } else { … }`.
- A message used in `apps/native/src/ui` is the `ui` domain's, any other the `app` domain's; the UI looks both up, and `just i18n` moves a message's translation along when it changes domain.
- Run `just i18n` after changing marked text and commit the templates and PO files with the change; `just i18n-check` and CI fail on a stale one.
