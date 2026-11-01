//! Group identity across scans: two groups are the same game when they
//! share an entry (same source and key).

use crate::types::GameGroup;
use std::collections::BTreeSet;

fn entry_keys(group: &GameGroup) -> BTreeSet<(&str, &str)> {
    group
        .entries
        .iter()
        .map(|entry| (entry.source.as_str(), entry.key.as_str()))
        .collect()
}

/// `(added, updated, removed)` going from `old` to `new`. Each old group
/// matches at most one new group (the first that shares an entry); a new
/// group without a match is added, an old one without a match removed, and
/// a matched pair that differs is updated (with the new contents).
pub fn diff(
    old: &[GameGroup],
    new: &[GameGroup],
) -> (Vec<GameGroup>, Vec<GameGroup>, Vec<GameGroup>) {
    let old_keys: Vec<_> = old.iter().map(entry_keys).collect();
    let mut matched = vec![false; old.len()];
    let (mut added, mut updated) = (Vec::new(), Vec::new());
    for group in new {
        let keys = entry_keys(group);
        let partner = old_keys
            .iter()
            .enumerate()
            .find(|(index, old)| !matched[*index] && !old.is_disjoint(&keys))
            .map(|(index, _)| index);
        match partner {
            Some(index) => {
                matched[index] = true;
                if old[index] != *group {
                    updated.push(group.clone());
                }
            }
            None => added.push(group.clone()),
        }
    }
    let removed = old
        .iter()
        .zip(matched)
        .filter(|(_, matched)| !matched)
        .map(|(group, _)| group.clone())
        .collect();
    (added, updated, removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DetectedGame, SourceId};

    fn group(entries: &[(&'static str, &str, &str)]) -> GameGroup {
        GameGroup {
            primary: 0,
            entries: entries
                .iter()
                .map(|(source, key, title)| DetectedGame::new(SourceId::new(source), *key, *title))
                .collect(),
        }
    }

    #[test]
    fn added_updated_removed_and_unchanged() {
        let old = vec![
            group(&[("steam", "1", "One")]),
            group(&[("steam", "2", "Two")]),
            group(&[("heroic", "a", "A")]),
        ];
        let new = vec![
            group(&[("steam", "1", "One")]),
            group(&[("steam", "2", "Two (renamed)")]),
            group(&[("heroic", "a", "A"), ("legendary", "a", "A")]),
            group(&[("steam", "3", "Three")]),
        ];
        let (added, updated, removed) = diff(&old, &new);
        assert_eq!(added, [new[3].clone()]);
        assert_eq!(updated, [new[1].clone(), new[2].clone()]);
        assert!(removed.is_empty());
        let (added, updated, removed) = diff(&new, &old);
        assert!(added.is_empty());
        assert_eq!(updated, [old[1].clone(), old[2].clone()]);
        assert_eq!(removed, [new[3].clone()]);
        assert_eq!(diff(&old, &old), (vec![], vec![], vec![]));
    }

    #[test]
    fn split_and_joined_groups() {
        let joined = vec![group(&[("heroic", "a", "A"), ("legendary", "a", "A")])];
        let split = vec![
            group(&[("heroic", "a", "A")]),
            group(&[("legendary", "a", "A")]),
        ];
        let (added, updated, removed) = diff(&joined, &split);
        assert_eq!((added.len(), updated.len(), removed.len()), (1, 1, 0));
        let (added, updated, removed) = diff(&split, &joined);
        assert_eq!((added.len(), updated.len(), removed.len()), (0, 1, 1));
    }
}
