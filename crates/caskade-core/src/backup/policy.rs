//! What the database snapshot leaves out. Every table and setting is backed
//! up unless it is listed here: a new table needs no backup code, only an
//! entry here when it holds credentials or tokens, a cache Caskade fills
//! again, or the state of work under way.

/// Tables whose rows are deleted from the snapshot, and why.
pub const EXCLUDED_TABLES: &[(&str, &str)] = &[
    (
        "igdb_popularity",
        "daily IGDB Visits scores, fetched again after restoring",
    ),
    (
        "paired_devices",
        "pairings hold the grants each device presents (credentials); devices pair again after a restore",
    ),
    (
        "shares",
        "a share holds the grant its link presents (a credential); shares are made again after a restore",
    ),
    (
        "share_recipients",
        "progress of shares that a restore drops with them",
    ),
    (
        "sync_peers",
        "what other devices said they have: a restored library re-syncs in full on its next connection",
    ),
    (
        "icon_checks",
        "which executables were read for icons: a restore without the icons must read them again",
    ),
];

/// Settings keys (exact, or a prefix ending in `.`) deleted from the
/// snapshot, and why.
pub const EXCLUDED_SETTINGS: &[(&str, &str)] = &[
    (
        "plugins.component-cache-key",
        "signs the compiled-plugin cache, which isn't backed up",
    ),
    (
        "sync.device",
        "this device's endpoint id and name, which a restore on another machine mustn't take over",
    ),
    (
        "storage.move.",
        "moves under way, which belong to this machine's folders",
    ),
];

/// The `WHERE` clause matching every excluded settings key.
pub(super) fn settings_filter() -> (String, Vec<&'static str>) {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    for (key, _) in EXCLUDED_SETTINGS {
        if key.ends_with('.') {
            // `substr` rather than LIKE: keys may hold `_` and `%`.
            clauses.push(format!("substr(key,1,{})=?", key.len()));
        } else {
            clauses.push("key=?".to_owned());
        }
        binds.push(*key);
    }
    (clauses.join(" OR "), binds)
}
