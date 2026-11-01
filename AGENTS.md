# AGENTS.md

Guidance for coding agents working in this repository. `CLAUDE.md` holds the full project guide (layout, commands, build, lint and test); read it too.

## Git workflow

- Commit all work to the default branch, `master`, in the main checkout. Don't create branches or git worktrees. Several agents work in this checkout at once, and work kept off `master` drifts from theirs and has to be merged back later.
- Commit only the files you changed: `git add <paths>`, never `git add -A`, `git add .` or `git commit -a`. Other sessions' uncommitted changes share the working tree; leave them for their owners.
- `CLAUDE.md` repeats these rules. Keep the two in step.

## Translations

- Translations (`docs/translating.md`, `docs/localization.md`): `just i18n` extracts marked text into `locales/<domain>.pot` (`ui`, `app`, `core`) and merges it into each `locales/<lang>/<domain>.po` (needs GNU gettext); commit both with the change. `just i18n-check` (also in CI) fails on a stale template, a PO file `msgfmt --check` rejects or a translation that drops or changes a placeholder or tag, and prints each language's completion. `pnpm i18n:extract --add <lang>` starts a language; adding `locales/<lang>/*.po` is all it takes to ship one.
