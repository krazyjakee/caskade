# Translating Caskade

Caskade is translated with gettext PO files, kept in the repository. English is the source language: its text is in the code, and anything a language leaves untranslated shows in English. Translations come in through pull requests. A hosted translation service can come later: Weblate works on these same PO files, so adding one changes nothing for translators who use a PO editor.

How the pipeline works inside the code is in [Localization](localization.md).

## The files

Each domain is a group of messages with a template of its own, `locales/<domain>.pot`, generated from the code. Never edit a template. Each language is a folder named by its BCP 47 tag (`sv`, `pt-BR`), with one PO file per domain.

| Domain | File | What it covers |
|---|---|---|
| `ui` | `locales/<lang>/ui.po` | The shared components (`@caskade/ui`): buttons, badges, dialogs. Plugins and themes use them too. |
| `app` | `locales/<lang>/app.po` | The rest of the interface: the library, game pages, Settings, first-run setup. |
| `core` | `locales/<lang>/core.po` | Text from the Rust side: error messages, notifications, the tray menu. |

Plugins and themes have a domain each, named after the plugin or theme, in their own folder's `locales/<lang>.po` (#219). Translate them like the app's.

## Starting a language

With GNU gettext installed (`gettext` on Linux and Homebrew, MSYS2 on Windows), from the repository root:

```sh
pnpm i18n:extract --add de   # starts locales/de/ui.po, app.po and core.po
just i18n                    # brings core.po into the core's layout
```

`msginit` fills in the language's header. Without gettext, copy each template to `locales/<lang>/<domain>.po` and fill in the header yourself:

```
"Language: de\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"
```

`Plural-Forms` says how many plural forms the language has and which one a count takes. Copy it from [the gettext manual's table](https://www.gnu.org/software/gettext/manual/html_node/Plural-forms.html) or from Poedit's translation properties; a plural entry then needs one `msgstr[n]` per form. Swedish and German have two forms, Polish three, Japanese one.

That's all it takes to ship a language: the build finds every folder in `locales/`, and the Language setting lists it with how much of it is translated. No code changes.

## Translating

Open the PO files in [Poedit](https://poedit.net), Lokalize, Gtranslator or any text editor.

- The English is the `msgid`. A `msgctxt` tells apart the same English used for different things ("Home" the sidebar link, "Home" the folder). Comments starting with `#.` are notes from the developers, and `#:` names the file the text comes from.
- Keep every placeholder the English has, in whatever order your language needs: `{name}`, `{0}` and `#` (the count, in plurals) are filled in when the text is shown. Don't translate what's inside the braces.
- Keep the tags around words, such as `<0>` and `</0>`: they become links or emphasis. Move them with the words they wrap.
- In a plural entry, `msgstr[0]`, `msgstr[1]`, … follow the order of your `Plural-Forms`. A form that only ever means one may leave the count out ("En fil"); the others keep it.
- An empty or fuzzy entry shows English. Poedit marks entries fuzzy when the English changed since they were translated: check the translation, then clear the flag.
- `{{` and `}}` in the core's messages are literal braces.

When the code changes, the templates change with it. Run `pnpm i18n:extract` (and `just i18n` for `core.po`) after pulling, so your files get the new messages; the old translations stay.

Swedish (`sv`) is the first language. Its first draft is machine-translated and needs a native speaker's review.

## Trying a language

- In a browser, without building the app: `just web`, then open http://localhost:1420/?lang=sv (use your language's tag). The pseudo-locale `?lang=en-XA` shows every translatable text accented and lengthened, which finds text that isn't marked or doesn't fit.
- In the app: `just dev`, then Settings → General → Language (first-run setup asks too). The language switches at once, the tray menu included.

The interface picks up changes to `ui.po` and `app.po` when the page reloads; the core compiles `core.po` in, so restart `just dev` after changing it.

## Checking

```sh
just i18n-check
```

It runs what CI runs (the `frontend` and `rust` jobs of `.github/workflows/ci.yml`) and fails, naming the file, the line and the entry, on:

- a template that doesn't match the code (run `just i18n`);
- a PO file `msgfmt --check` rejects (a broken header, a missing plural form, a line ending that differs from the English);
- a translation that drops, renames or adds a placeholder (`{name}`, `{0}`, `#`) or a tag (`<0>`, `</0>`).

Then it prints how much of each language is translated, per domain. `pnpm i18n:check` runs the webview's half (and every PO file) without Rust.

## Sending a translation

Open a pull request with your `locales/<lang>/` files. Run `just i18n-check` first if you can; CI runs it either way. Partial translations are welcome: a language ships as soon as it has a file, showing how complete it is.

## For developers

Translators see only the English and what you add to it.

- Add a translator comment when the English alone can leave someone guessing: what a placeholder holds, where the text appears, how much room it has, or whether a word is a verb or a noun. In the webview, `msg({ message: "Open", comment: "A button that opens the game's folder" })` or `t({ message, comment })` writes it as a `#.` line. Name placeholders by what fills them (`` t`Open ${{ title }}` `` gives `{title}`, not `{0}`).
- Add a `msgctxt` only when the same English means different things in different places, so each needs its own translation: `context: "navigation"` in the webview, `trp!("folder browser place", "Home")` in the core. Two places that mean the same thing share one message. A context isn't a comment: changing it makes every translation of the message start over.
- The core's extractor doesn't read Rust comments. Where the English alone is unclear there, word the context (`trp!`) for the translator.
- Mark whole sentences, with placeholders, not pieces joined in code: word order differs between languages. Use plurals (`plural`, `trn!`) for counts, never `count === 1 ? … : …`.
- Run `just i18n` after changing marked text and commit the templates and PO files with the change; `just i18n-check` and CI fail on a stale template.
