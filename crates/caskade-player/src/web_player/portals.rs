//! Stand-ins for portals' ad SDKs: an offline copy
//! plays without the portal's page, and a game that waits for an ad its
//! page would have shown waits for good.

/// In every frame of an offline copy. Poki's SDK, in the "hoist" build its
/// game frames load, finishes `init()` and every ad break only once
/// poki.com's page answers; alone, the game stops its clock and its input
/// for an ad that never comes (Upgrade the CARS). Its own no-ads switch
/// finishes them at once and grants rewarded breaks.
pub const SCRIPT: &str = "window.__poki_no_ads = true;";
