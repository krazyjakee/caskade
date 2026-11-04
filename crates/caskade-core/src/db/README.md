# Library store

`LibraryStore::open_default(&AppPaths)` opens the per-user library database and
runs the embedded migrations. Opening (the first connection, which creates the
file and switches it to WAL, and the migrations) has its own bound, `Opening`:
the library, a staged restore's and the caches (`cache_db`, the Ludusavi
index) wait up to five minutes for a slow disk (`Opening::PATIENT`);
`open(path)` waits 30 s. Queries keep sqlx's default
wait for a connection. `open(path)` supports an explicit database;
`in_memory()` isolates tests. Handles are cloneable and use an asynchronous
SQLx pool, foreign keys, WAL for files and a five-second busy timeout.

Games retain every source entry. The `(source_id, external_id)` pair identifies
an entry across refreshes; its opaque ID stays stable. Moving an entry to another
game requires `merge_games`, which transfers sessions, artwork and collection
membership in one transaction. Merging two games with open sessions is refused.
Deleting a game cascades its records. Explicit entry deletion also deletes that
entry's sessions; discovery refreshes should update install state instead.

Managed entries persist the installer's root ID and a portable relative game
directory. Launch paths, working directories, prefix paths and tracking paths
are relative to that game. Changing a root path never changes the entry. Runtime
runner paths are resolved by the launch service. External entries retain their
original absolute location and structured discovery launch target.

`list_games`, `collection_games` and `library_stats` omit hidden games;
`get_game` can still retrieve one by ID. Lists are bounded to 1,000 rows and use
consistent count/page snapshots. Search treats words literally: tokens of at
least three characters use the trigger-maintained FTS5 trigram index, while
shorter tokens use an escaped substring query. Query input is bound as data.

Session UTC timestamps describe events; duration is supplied by the monotonic
tracker rather than calculated from wall time. A partial unique index reserves
one open session per game. Activation sets the observed game start, heartbeats
never decrease duration, and finishing is idempotent. Tracking JSON belongs to
the sessions service. Settings offer atomic compare-and-set so section services
can resolve concurrent edits and emit events after a successful commit.

The reversible initial migration and repository tests cover all entities,
rollback, source-preserving merges, concurrent reservations/settings updates,
managed path rejection, search trigger maintenance and a generated 1,203-game
search benchmark. Run `cargo test -p caskade-core db::tests -- --nocapture` to
see measured latency (the test requires p95 below 50 ms).
