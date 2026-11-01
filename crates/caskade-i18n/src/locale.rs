//! Language tags: BCP 47 (`pt-BR`, `zh-Hant-TW`), from the OS or a
//! setting, matched to the catalogs there are.

/// The source language: messages are written in English, so it needs no
/// catalog and is the last fallback.
pub const SOURCE: &str = "en";

/// A BCP 47 tag in its usual case (`pt-BR`, `zh-Hant`), from a BCP 47 or
/// POSIX locale (`pt_BR.UTF-8`, `sr_RS@latin`). `None` for `C`, `POSIX`
/// and anything that isn't a language.
pub fn normalize(tag: &str) -> Option<String> {
    let tag = tag.split(['.', '@']).next().unwrap_or_default().trim();
    let mut parts = tag.split(['-', '_']);
    let language = parts.next()?;
    if !(2..=8).contains(&language.len())
        || !language.chars().all(|c| c.is_ascii_alphabetic())
        || language.eq_ignore_ascii_case("posix")
    {
        return None;
    }
    let mut normalized = language.to_ascii_lowercase();
    for part in parts {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        normalized.push('-');
        if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            // Script: Hant.
            let (first, rest) = part.split_at(1);
            normalized.push_str(&first.to_ascii_uppercase());
            normalized.push_str(&rest.to_ascii_lowercase());
        } else if part.len() == 2 || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit())) {
            // Region: BR, 419.
            normalized.push_str(&part.to_ascii_uppercase());
        } else {
            normalized.push_str(&part.to_ascii_lowercase());
        }
    }
    Some(normalized)
}

/// `tag` and the less specific tags it falls back to, most specific
/// first: `zh-Hant-TW`, `zh-Hant`, `zh`.
pub fn fallbacks(tag: &str) -> Vec<String> {
    let Some(tag) = normalize(tag) else {
        return Vec::new();
    };
    let mut chain = vec![tag.clone()];
    let mut rest = tag.as_str();
    while let Some((shorter, _)) = rest.rsplit_once('-') {
        chain.push(shorter.to_owned());
        rest = shorter;
    }
    chain
}

/// The first of `available` that the `requested` tags, in order of
/// preference, fall back to: `pt-BR` takes `pt-BR`, then `pt`, before the
/// next requested language. `None` when none matches; English is then the
/// caller's fallback (list [`SOURCE`] in `available` to match it too).
pub fn negotiate<'a, I, S>(requested: I, available: &[&'a str]) -> Option<&'a str>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    requested.into_iter().find_map(|tag| {
        fallbacks(tag.as_ref()).into_iter().find_map(|candidate| {
            available.iter().copied().find(|available| {
                normalize(available).is_some_and(|available| available == candidate)
            })
        })
    })
}

/// The OS's preferred languages, most preferred first, normalized. On
/// Linux they come from `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG`.
pub fn system() -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in sys_locale::get_locales().filter_map(|tag| normalize(&tag)) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bcp47_and_posix_tags() {
        assert_eq!(normalize("pt_BR.UTF-8").as_deref(), Some("pt-BR"));
        assert_eq!(normalize("sv").as_deref(), Some("sv"));
        assert_eq!(normalize("SV-se").as_deref(), Some("sv-SE"));
        assert_eq!(normalize("zh-hant-tw").as_deref(), Some("zh-Hant-TW"));
        assert_eq!(normalize("es-419").as_deref(), Some("es-419"));
        assert_eq!(normalize("sr_RS@latin").as_deref(), Some("sr-RS"));
        assert_eq!(normalize("C"), None);
        assert_eq!(normalize("POSIX"), None);
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("en--US"), None);
    }

    #[test]
    fn falls_back_to_less_specific_tags() {
        assert_eq!(fallbacks("pt_BR"), ["pt-BR", "pt"]);
        assert_eq!(fallbacks("zh-Hant-TW"), ["zh-Hant-TW", "zh-Hant", "zh"]);
        assert!(fallbacks("C").is_empty());
    }

    #[test]
    fn negotiates_in_order_of_preference() {
        let available = ["en", "pt", "sv", "zh-Hant"];
        assert_eq!(negotiate(["pt-BR"], &available), Some("pt"));
        assert_eq!(negotiate(["sv_SE.UTF-8"], &available), Some("sv"));
        assert_eq!(negotiate(["zh-Hant-HK"], &available), Some("zh-Hant"));
        // The first preference with a catalog wins, even English.
        assert_eq!(negotiate(["de", "sv", "en"], &available), Some("sv"));
        assert_eq!(negotiate(["en-GB", "sv"], &available), Some("en"));
        // A region never stands in for another.
        assert_eq!(negotiate(["pt"], &["pt-BR"]), None);
        assert_eq!(negotiate(["de", "fr"], &available), None);
        assert_eq!(negotiate(Vec::<String>::new(), &available), None);
    }
}
