# Playtime stats

A UI-only plugin that asks for no permissions.

- **Playtime stats view** in the sidebar: total hours across your most played games, and a bar chart of the top ten. Bars use the theme's accent, and each shows its game and hours on hover. A **Table** toggle shows the same data as a table.
- **When a session ends** (`game.exited`), a notice says "Played <title> for <n>m". The title comes from `ctx.library.game`.

It uses `useGames({ sort: "playtime", limit: 10 })` from `@caskade/sdk`. Its CSS is styled only with the theme's `--cs-*` tokens.
