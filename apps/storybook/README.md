# @caskade/storybook

The playground for theme and plugin authors (#144): every `@caskade/ui`
component in every state the anatomy catalogue documents, the catalogue entry
beside each story, the shell's game pieces and settings rows, and the token
reference.

```sh
pnpm storybook        # http://localhost:6006
pnpm build-storybook  # static build in apps/storybook/storybook-static
pnpm --filter @caskade/storybook test
```

## Toolbar

- **Theme**: every folder under the repository's `themes/` (`themes/default`,
  `themes/paper`, …), or none (the contract's default tokens). The playground
  compiles a theme the way `caskade-theme` does (`src/themes/compile.ts`): its
  `extends` chain's `tokens.json` as `--cs-*` on `:root`, then each
  `theme.css`, base first, inside `@layer cs.theme`, with `url(assets/…)`
  pointing at the theme's files. A theme that doesn't compile shows its
  problems above the story.
- **Mode**, **Scheme** and **Effects** set `data-mode`, `data-color-scheme`
  and `data-effects="reduced"` on `<html>`, as the app does. "Theme default"
  is the theme's first entry in `modes`.
- **Anatomy** shows or hides the catalogue entry beside the story.

### Trying a theme folder from disk

Theme folders are read at build time with `import.meta.glob` over
`themes/*/`. Copy or symlink a theme folder into `themes/` and it appears in
the toolbar; under `pnpm storybook` edits to its `theme.json`, `tokens.json`,
`theme.css` and `assets/` restyle the open story live. Lint it with
`cargo run -p caskade-theme --features cli -- lint themes/<folder>`.

## Stories

- `Components/*` are generated from the anatomy catalogue
  (`packages/ui/src/anatomy.ts`) and the renders in `src/catalogue/fixtures.tsx`:
  `Parts` shows every part, `States` every domain state value, `Interactions`
  each interaction attribute a prop can set.
- `Shell/*` puts them together: the game card grid, the hero, Home's shelves,
  settings rows. `Shell/About` renders the app's own About page and Legal view
  (`apps/desktop/src/views/settings/about/`), in desktop and console modes.
- `Tokens/Reference` is generated from `@caskade/tokens/manifest.json`, one
  page per tier; previews read the live tokens, so they follow the toolbar.

The Vitest suite renders every story through portable stories and fails on
errors or console warnings, and compares the catalogue's `@caskade/ui`
entries, states and settable interactions with what the stories render. App
entries of the catalogue (the shell) are checked by `apps/desktop`.
