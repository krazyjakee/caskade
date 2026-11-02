//! Versions of managed programs: newer or not, and how people read them.

use crate::updates::newer;

/// A version for people: a rolling build (`latest@2026-10-25T09:30:00Z`)
/// as its tag and day.
pub fn version_label(version: &str) -> String {
    match version.split_once('@') {
        Some((tag, time)) => format!("{tag} ({})", time.get(..10).unwrap_or(time)),
        None => version.to_owned(),
    }
}

/// Whether `latest` is newer than `installed` and wasn't `skipped`.
pub fn update_available(
    installed: Option<&str>,
    latest: Option<&str>,
    skipped: Option<&str>,
) -> bool {
    match (latest, installed) {
        (Some(latest), Some(installed)) => {
            // Rolling builds compare by publish time (ISO 8601 sorts as text).
            let newer = match (latest.split_once('@'), installed.split_once('@')) {
                (Some((_, new)), Some((_, old))) => new > old,
                _ => newer(latest, installed) || hotfix(latest, installed),
            };
            newer && skipped != Some(latest)
        }
        _ => false,
    }
}

/// A hotfix of the same release: Dolphin's `2609a` after `2609`, `2609b`
/// after `2609a`.
fn hotfix(latest: &str, installed: &str) -> bool {
    fn split(version: &str) -> (&str, &str) {
        let base = version.trim_end_matches(|c: char| c.is_ascii_lowercase());
        version.split_at(base.len())
    }
    let (new_base, new_letter) = split(latest);
    let (old_base, old_letter) = split(installed);
    new_base == old_base
        && new_base.ends_with(|c: char| c.is_ascii_digit())
        && new_letter.len() <= 1
        && new_letter > old_letter
}
