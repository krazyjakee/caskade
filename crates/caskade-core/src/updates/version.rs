//! Release names as versions: which of two is newer, which are
//! prereleases, and a file name with one version swapped for another.
//! Release tags are free text, so these read the numbers out of them and
//! fall back to "different is newer" when there are none.

/// Whether `latest` is a newer release than `installed`: a different one
/// whose numbers aren't lower, or the release of the prerelease installed.
pub(crate) fn newer(latest: &str, installed: &str) -> bool {
    if latest == installed {
        return false;
    }
    let (Some(new), Some(old)) = (numbers(latest), numbers(installed)) else {
        return true;
    };
    match new.cmp(&old) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        // `v1.2` after `1.2`, or `1.2.0` after `1.2.0-rc1`.
        std::cmp::Ordering::Equal => prerelease(installed) && !prerelease(latest),
    }
}

/// The first run of numbers in `version` (`v1.2.10-beta` → 1, 2, 10),
/// without trailing zeros so `1.2` and `1.2.0` are the same.
fn numbers(version: &str) -> Option<Vec<u64>> {
    let start = version.find(|c: char| c.is_ascii_digit())?;
    let mut numbers = Vec::new();
    let mut rest = &version[start..];
    loop {
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        // A run too long for a number (a hash) isn't a version.
        numbers.push(rest[..end].parse().ok()?);
        rest = &rest[end..];
        match rest.strip_prefix(['.', '_']) {
            Some(next) if next.starts_with(|c: char| c.is_ascii_digit()) => rest = next,
            _ => break,
        }
    }
    while numbers.len() > 1 && numbers.last() == Some(&0) {
        numbers.pop();
    }
    Some(numbers)
}

/// Whether a release name marks a prerelease: a word like `beta`, `rc2`
/// or `nightly`.
pub(crate) fn prerelease(version: &str) -> bool {
    const MARKS: [&str; 9] = [
        "alpha", "beta", "rc", "pre", "preview", "dev", "nightly", "canary", "snapshot",
    ];
    version
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| {
            MARKS.iter().any(|mark| {
                word.strip_prefix(mark)
                    .is_some_and(|rest| rest.bytes().all(|b| b.is_ascii_digit()))
            })
        })
}

/// `name` with release `old` swapped for `new` (`Game-1.2-linux.zip` →
/// `Game-1.3-linux.zip`), as the tag or without its `v`. Only whole parts
/// of the name are swapped, so `1` doesn't hit `x64`. `None` when the name
/// doesn't carry the version.
pub(crate) fn swap(name: &str, old: &str, new: &str) -> Option<String> {
    [(old, new), (bare(old), bare(new))]
        .into_iter()
        .filter(|(from, _)| !from.is_empty())
        .find_map(|(from, to)| replace_whole(name, from, to))
}

fn bare(tag: &str) -> &str {
    tag.strip_prefix(['v', 'V']).unwrap_or(tag)
}

fn replace_whole(name: &str, from: &str, to: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    let mut last = 0;
    for (at, _) in name.match_indices(from) {
        let end = at + from.len();
        if alone(&name[..at], &name[end..]) {
            out.push_str(&name[last..at]);
            out.push_str(to);
            last = end;
        }
    }
    (last > 0).then(|| out + &name[last..])
}

/// Whether text between `before` and `after` is a part of its own: no
/// letter or digit next to it, nor a `.` that continues a number
/// (`1.2` in `1.2.1`).
fn alone(before: &str, after: &str) -> bool {
    let mut back = before.chars().rev();
    let mut ahead = after.chars();
    let part = |next: Option<char>, beyond: Option<char>| match next {
        None => true,
        Some('.') => !beyond.is_some_and(|c| c.is_ascii_digit()),
        Some(c) => !c.is_ascii_alphanumeric(),
    };
    part(back.next(), back.next()) && part(ahead.next(), ahead.next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_reads_the_numbers_and_prerelease_marks() {
        assert!(newer("v1.3.0", "v1.2.9"));
        assert!(newer("1.10", "1.9"));
        assert!(newer("2026.1026.0", "2026.1005.0-lazer"));
        assert!(newer("1.2.0", "1.2.0-rc1"));
        assert!(!newer("1.2.0-rc1", "1.2.0"));
        assert!(!newer("v1.2", "1.2.0"));
        assert!(!newer("1.2.0", "1.3.0"));
        assert!(!newer("same", "same"));
        // Without numbers, a different release is a new one.
        assert!(newer("stable", "nightly"));
    }

    #[test]
    fn prerelease_marks_are_whole_words() {
        assert!(prerelease("v2.0.0-beta.3"));
        assert!(prerelease("1.0-rc2"));
        assert!(prerelease("nightly-2026-10-28"));
        assert!(!prerelease("2026.1005.0-lazer"));
        assert!(!prerelease("betaflight-4.5"));
        assert!(!prerelease("v3.1"));
    }

    #[test]
    fn swap_replaces_the_version_where_it_stands_alone() {
        assert_eq!(
            swap("Game-1.2-linux-x64.zip", "v1.2", "v1.3").as_deref(),
            Some("Game-1.3-linux-x64.zip")
        );
        assert_eq!(
            swap("game_v1.2_win.zip", "v1.2", "v1.3").as_deref(),
            Some("game_v1.3_win.zip")
        );
        assert_eq!(
            swap("Game-1-x64.zip", "1", "2").as_deref(),
            Some("Game-2-x64.zip")
        );
        assert_eq!(
            swap("osu.AppImage", "2026.1005.0-lazer", "2026.1026.0"),
            None
        );
        assert_eq!(swap("Game-11.zip", "1", "2"), None);
        assert_eq!(swap("Game-1.2.1.zip", "1.2", "1.3"), None);
    }
}
