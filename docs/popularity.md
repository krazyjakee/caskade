# Popularity sorting

The Library's **Popular** sort and smart collections use IGDB's **Visits**
PopScore primitive: the share of visits to a game's IGDB page. It covers
games across platforms. It is not a player count, rating or sales figure.
The Store offers the same sort alongside its default relevance/most-rated
order and Newest. Recent and Upcoming keep their release-date order.

## Metadata server contract

IGDB credentials remain on the metadata server. The server discovers Visits
through `popularity_types` and `external_popularity_source.name`, and keeps
a daily snapshot of its `popularity_primitives`. It never mixes different
primitive types. See [IGDB's PopScore documentation](https://api-docs.igdb.com/#popscore).

`POST /popularity` accepts an array of up to 500 numeric IGDB ids. It answers:

```json
{
  "scores": [
    { "gameId": 1942, "popularityType": 1, "value": 0.0066, "calculatedAt": 1793404800 }
  ],
  "notFound": [1020]
}
```

`popularityType` is discovered, not a client constant; `calculatedAt` is
Unix seconds. Every requested id must appear exactly once in either list.
An incomplete answer, duplicate, unsolicited id, invalid score or failed
request keeps the previous batch and follows the IGDB client's shared
pause and Retry-After rules. Earlier successful batches are kept.

`POST /catalog/search` additionally accepts `sort`: `relevance` (default),
`mostRated`, `popular` or `newest`. The client omits the default for older
servers. Explicit sorts with a title query match the title as a substring:
IGDB's relevance search cannot be combined with an explicit sort.
All platform, genre and first-release-date filters still apply before
paging. The server intersects ranked primitive ids with the matching games,
then appends unscored games by title. `nextOffset` is the next filtered page,
or null; no total is claimed. Install the server changes before using the
new app sorts; an older server has no popularity endpoint.

## A server without popularity

An older server answers `/popularity` with 404 and ignores `sort` on
`/catalog/search`, answering in its usual order (so a page can't tell
whether it was sorted). The app degrades without errors:

- A popularity request answered 400, 404, 405, 422 or 501 pauses
  popularity alone for an hour (`popularity::UNSUPPORTED_PAUSE`); game
  lookups, matching and the Store are not paused, and matching's watch
  doesn't count it as a failed run. The Library's Popular sort keeps the
  last scores, and games without one (all of them, on a library that never
  had any) follow by title.
- A Store search with a sort other than relevance first learns whether the
  server has popularity (`Igdb::popularity_offered`: any popularity answer
  says so, else one request for a single id, remembered, and asked again
  after the hour). Without it, the search asks for the usual order and the
  page says so (`IgdbCatalogPage::sort_unavailable`); the Store shows a
  quiet note instead of an error. A server that has popularity but refuses
  a sort with one of those statuses gets the same answer.
- Other failures (unreachable, 429, 5xx) keep the client's shared pause and
  Retry-After rules.

Once the server has popularity, the sorts work without an app update. Check
it with the ignored live test:
`cargo test -p caskade-core --lib igdb::popularity_tests::live -- --ignored`.

## Local cache and ordering

`igdb::popularity` refreshes due matched ids from `Metadata::watch`, after
matching, under one refresh lock. Successful answers, including explicit
missing scores, are fresh for 24 hours. Sorting only reads SQLite and never
makes a request. Failed refreshes retain scores across restarts and retry
through the background metadata watch. The independent `igdb_popularity`
table records the IGDB id, type, value, calculation time and fetch time;
`backup::policy` excludes it because it can be fetched again.

The query uses each game's current `igdb_matches` row. A merge uses the kept
game's match, and a rematch immediately uses the new id's cached score.
Scored games come first, descending, including a score of zero. Equal scores
and all games without a score use the normal localized title order.
Hidden games and account age limits use the existing visibility rules.
The Store keeps the sort in its URL and includes it in page cache keys.

The permanent `IgdbGame` cache and its refresh policy are unchanged.
