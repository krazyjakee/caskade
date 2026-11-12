# HowLongToBeat

How long a game takes to beat, on its page (plugin API 0.2, #109).

- **"How long to beat" tab** on the game page, drawn by the backend: main story, main + extras and completionist hours. The host gives `render` the game's title, so the plugin needs no library access. When the site can't be reached the tab says so.
- **Backend** (`backend/`): looks the title up on howlongtobeat.com's search API through `wasi:http`, and caches each answer in the plugin's key-value store.

Permissions: `network: ["howlongtobeat.com"]` only. The host refuses every other host, including redirects to one.

Setting **Search endpoint** defaults to `https://howlongtobeat.com/api/search`. HowLongToBeat's API is unofficial and changes. If it moves, point this setting at the new endpoint, which must still be on an allowed host. The tests use a local mock server.
