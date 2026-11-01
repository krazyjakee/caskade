/// Best-effort fallback for source filenames, never preferred over explicit
/// installer/store/package metadata. Keeps title numbers such as "The Witcher 3"
/// while discarding recognizable version/build/platform suffixes.
pub fn clean_title(name: &str) -> String {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut name = name.to_string();
    for extension in [
        ".tar.gz",
        ".tar.bz2",
        ".tar.xz",
        ".tar.zst",
        ".appimage",
        ".exe",
        ".msi",
        ".zip",
        ".7z",
        ".rar",
        ".tar",
        ".dmg",
        ".pkg",
        ".app",
        ".sh",
        ".run",
        ".deb",
        ".rpm",
    ] {
        if name.to_lowercase().ends_with(extension) {
            name.truncate(name.len() - extension.len());
            break;
        }
    }
    loop {
        let lower = name.to_lowercase();
        if let Some(prefix) = ["setup_", "setup-", "setup ", "install_", "install-", "gog_"]
            .iter()
            .find(|prefix| lower.starts_with(**prefix))
        {
            name.drain(..prefix.len());
        } else {
            break;
        }
    }
    // Numeric/architecture/language suffixes in GOG's parenthesized convention.
    loop {
        let trimmed = name.trim_end_matches(['_', '-', ' ']);
        let Some(start) = trimmed.rfind('(').filter(|_| trimmed.ends_with(')')) else {
            break;
        };
        let suffix = &trimmed[start + 1..trimmed.len() - 1];
        let lower = suffix.to_lowercase();
        if suffix.chars().all(|c| c.is_ascii_digit())
            || [
                "32bit", "64bit", "32-bit", "64-bit", "x86", "x64", "en", "english", "gog",
            ]
            .contains(&lower.as_str())
        {
            name = trimmed[..start].trim_end_matches(['_', '-', ' ']).into();
        } else {
            break;
        }
    }
    let words: Vec<_> = name
        .split(['_', ' '])
        .filter(|word| !word.is_empty())
        .collect();
    let mut kept = vec![];
    for (index, word) in words.iter().enumerate() {
        let lower = word.to_lowercase();
        let version = lower.strip_prefix('v').unwrap_or(&lower);
        if version.chars().next().is_some_and(|c| c.is_ascii_digit()) && version.contains('.')
            || lower.starts_with('v') && version.bytes().next().is_some_and(|b| b.is_ascii_digit())
            || is_date(&lower)
            || ["gog", "release"].contains(&lower.as_str()) && !kept.is_empty()
            || lower.starts_with("gog-")
            || ["32", "64"].contains(&lower.as_str())
                && words.get(index + 1).is_some_and(|next| next.contains('.'))
            || lower.starts_with("build") && lower.chars().any(|c| c.is_ascii_digit())
            || [
                "windows", "win32", "win64", "linux", "macos", "osx", "darwin", "x86_64", "x64",
                "x86", "amd64", "aarch64", "64bit", "32bit",
            ]
            .contains(&lower.as_str())
            || lower == "v" && !kept.is_empty()
        {
            break;
        }
        kept.push(*word);
    }
    let title = kept.join(" ").replace("--", " ");
    title
        .split_whitespace()
        .map(display_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_date(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
}

fn display_word(word: &str) -> String {
    if word.chars().any(char::is_uppercase) {
        return word.into();
    }
    if [
        "ftl", "tes", "sw", "pop", "ii", "iii", "iv", "vi", "vii", "viii", "ix", "hd", "dx", "dlc",
        "goty", "bfg",
    ]
    .contains(&word)
    {
        return word.to_uppercase();
    }
    let word = match word {
        "trektm" => "trek",
        "warstm" => "wars",
        _ => word,
    };
    word.split('-')
        .map(|part| {
            let mut result = String::new();
            let mut previous_letter = false;
            for (index, character) in part.chars().enumerate() {
                if character.is_ascii_digit() && previous_letter {
                    result.push(' ');
                }
                if index == 0 {
                    result.extend(character.to_uppercase());
                } else {
                    result.push(character);
                }
                previous_letter = character.is_alphabetic();
            }
            result
        })
        .collect::<Vec<_>>()
        .join("-")
}
