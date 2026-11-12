# Playtime stats

Where your playtime goes (plugin API 0.2, #109). Its backend draws the view
with Caskade's components and asks only to read the library.

- **Playtime stats view** in the sidebar, in desktop and console mode: total hours across your most played games, then a bar chart of the top ten, each bar with its game and hours as text. **Show as** switches to a table; in console mode LB and RB do too. The choice is the view's state, which `act` returns and `render` reads.
- **Last session**: when a game exits (`game.exited`), the backend keeps its title and length, and the view shows them.

It reads the library with `library-query` sorted by playtime (`library: read`).
