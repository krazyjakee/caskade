# HowLongToBeat

A plugin with a UI half and a backend half.

- **"How long to beat" tab** on the game page. It shows main story, main + extras and completionist hours for the game's title.
- **Backend** (`backend/`): looks the title up on howlongtobeat.com's search API through `wasi:http`, and caches each answer in the plugin's key-value store.

Permissions: `network: ["howlongtobeat.com"]` only. The host refuses every other host, including redirects to one.

Setting **Search endpoint** defaults to `https://howlongtobeat.com/api/search`. HowLongToBeat's API is unofficial and changes. If it moves, point this setting at the new endpoint, which must still be on an allowed host. The tests use a local mock server.
