//! The `en-XA` pseudo-locale: the English accented, bracketed and about
//! 35 % longer, to find text that isn't marked and labels that overflow.
//! The same transform as the webview's (Lingui's `pseudoLocalize` over the
//! `pseudolocale` package, with `packages/i18n`'s options), applied to each
//! text piece of a message, so both UIs show the same thing.

/// The pseudo-locale's tag.
pub const LOCALE: &str = "en-XA";

/// What surrounds each text piece, and how much longer it gets.
const PREPEND: &str = "[";
const APPEND: &str = "]";
const EXTEND: f64 = 0.35;
const EXTEND_CHARACTER: char = '~';
/// Marks a tag in the text while it's transformed, as Lingui does; it
/// counts toward the length the padding is worked out from.
const DELIMITER: &str = "%&&&%";

fn accented(c: char) -> char {
    let code = match c {
        'A' => 192,
        'a' => 224,
        'B' => 223,
        'b' => 384,
        'C' => 262,
        'c' => 263,
        'D' => 270,
        'd' => 271,
        'E' => 274,
        'e' => 275,
        'F' => 401,
        'f' => 402,
        'G' => 284,
        'g' => 285,
        'H' => 292,
        'h' => 293,
        'I' => 296,
        'i' => 297,
        'J' => 309,
        'j' => 308,
        'K' => 310,
        'k' => 311,
        'L' => 313,
        'l' => 314,
        'M' => 7742,
        'm' => 7743,
        'N' => 323,
        'n' => 324,
        'O' => 332,
        'o' => 333,
        'P' => 420,
        'p' => 421,
        'Q' => 490,
        'q' => 491,
        'R' => 340,
        'r' => 341,
        'S' => 346,
        's' => 347,
        'T' => 354,
        't' => 355,
        'U' => 360,
        'u' => 361,
        'W' => 372,
        'w' => 373,
        'Y' => 374,
        'y' => 375,
        'Z' => 377,
        'z' => 378,
        _ => return c,
    };
    char::from_u32(code).unwrap_or(c)
}

/// The length of a tag (`<0>`, `</0>`, `<br/>`) at the start of `text`.
fn tag_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut at = 1;
    if bytes.first() != Some(&b'<') {
        return None;
    }
    if bytes.get(at) == Some(&b'/') {
        at += 1;
    }
    let start = at;
    while bytes
        .get(at)
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        at += 1;
    }
    if at == start {
        return None;
    }
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if bytes.get(at) == Some(&b'/') {
        at += 1;
    }
    (bytes.get(at) == Some(&b'>')).then_some(at + 1)
}

/// One text piece of a message in the pseudo-locale.
pub fn localize(text: &str) -> String {
    // The piece with its tags marked, as JavaScript sees it, for the length.
    let mut marked = String::with_capacity(text.len() * 2);
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(len) = tag_len(rest) {
            marked.push_str(DELIMITER);
            marked.push_str(&rest[..len]);
            marked.push_str(DELIMITER);
            rest = &rest[len..];
            continue;
        }
        let c = rest.chars().next().unwrap_or_default();
        marked.push(accented(c));
        rest = &rest[c.len_utf8()..];
    }
    let units = marked.encode_utf16().count();
    let side = (units as f64 * EXTEND / 2.0).floor() as usize;
    let pad: String = std::iter::repeat_n(EXTEND_CHARACTER, side).collect();
    format!(
        "{PREPEND}{pad}{}{pad}{APPEND}",
        marked.replace(DELIMITER, "")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_webviews_pseudo_locale() {
        // Lingui's `pseudoLocalize` with packages/i18n's options, in Node.
        assert_eq!(localize("Quick menu"), "[~Ǫũĩćķ ḿēńũ~]");
        assert_eq!(localize("Stop "), "[Śţōƥ ]");
        assert_eq!(
            localize(" opens the quick menu"),
            "[~~~ ōƥēńś ţĥē ǫũĩćķ ḿēńũ~~~]"
        );
        assert_eq!(
            localize("Go to <0>Library</0>"),
            "[~~~~~~~Ĝō ţō <0>Ĺĩƀŕàŕŷ</0>~~~~~~~]"
        );
    }
}
