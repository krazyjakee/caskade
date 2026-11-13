//! Rich text: a translated message with numbered tags (`<0>…</0>`,
//! `<1/>`), the form the translations made for the webview's `<Trans>`
//! carry, into a tree the UI turns
//! into elements. The translation may move the tags with their words; a
//! tag that isn't closed shows as text.

/// A piece of rich text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Text(String),
    /// The element the code numbered `index`, around its translated
    /// contents (none for `<1/>`).
    Element {
        index: usize,
        children: Vec<Node>,
    },
}

/// The tag at the start of `text`: its index, whether it closes, whether
/// it's empty (`<1/>`), and its length.
fn tag(text: &str) -> Option<(usize, bool, bool, usize)> {
    let rest = text.strip_prefix('<')?;
    let (closing, rest) = match rest.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let index = rest[..digits].parse().ok()?;
    let after = &rest[digits..];
    let (empty, after) = match after.strip_prefix('/') {
        Some(after) if !closing => (true, after),
        _ => (false, after),
    };
    after.starts_with('>').then(|| {
        let len = text.len() - after.len() + 1;
        (index, closing, empty, len)
    })
}

/// `text` as a tree of text and numbered elements.
pub fn parse(text: &str) -> Vec<Node> {
    let mut at = 0;
    let nodes = parse_nodes(text, &mut at, None);
    // Text after a stray closing tag stays.
    if at < text.len() {
        let mut nodes = nodes;
        push_text(&mut nodes, &text[at..]);
        return nodes;
    }
    nodes
}

fn push_text(nodes: &mut Vec<Node>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(Node::Text(last)) = nodes.last_mut() {
        last.push_str(text);
    } else {
        nodes.push(Node::Text(text.to_owned()));
    }
}

fn parse_nodes(text: &str, at: &mut usize, open: Option<usize>) -> Vec<Node> {
    let mut nodes = Vec::new();
    while *at < text.len() {
        let rest = &text[*at..];
        let Some(next) = rest.find('<') else {
            push_text(&mut nodes, rest);
            *at = text.len();
            break;
        };
        push_text(&mut nodes, &rest[..next]);
        *at += next;
        let rest = &text[*at..];
        match tag(rest) {
            Some((index, true, _, len)) if Some(index) == open => {
                *at += len;
                return nodes;
            }
            Some((index, false, true, len)) => {
                *at += len;
                nodes.push(Node::Element {
                    index,
                    children: Vec::new(),
                });
            }
            Some((index, false, false, len)) => {
                let start = *at;
                *at += len;
                let children = parse_nodes(text, at, Some(index));
                // Never closed: the tag shows as text.
                if !closed_before(text, start, *at, index) {
                    *at = start + len;
                    push_text(&mut nodes, &text[start..start + len]);
                    continue;
                }
                nodes.push(Node::Element { index, children });
            }
            _ => {
                push_text(&mut nodes, "<");
                *at += 1;
            }
        }
    }
    nodes
}

/// Whether the element opened at `start` was closed by `end`.
fn closed_before(text: &str, start: usize, end: usize, index: usize) -> bool {
    text[start..end].ends_with(&format!("</{index}>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Node {
        Node::Text(value.to_owned())
    }

    #[test]
    fn parses_numbered_tags_and_keeps_stray_ones_as_text() {
        assert_eq!(
            parse("<0>Guide</0> opens the quick menu"),
            [
                Node::Element {
                    index: 0,
                    children: vec![text("Guide")]
                },
                text(" opens the quick menu")
            ]
        );
        assert_eq!(
            parse("a <1/> b <0>c <2>d</2></0>"),
            [
                text("a "),
                Node::Element {
                    index: 1,
                    children: vec![]
                },
                text(" b "),
                Node::Element {
                    index: 0,
                    children: vec![
                        text("c "),
                        Node::Element {
                            index: 2,
                            children: vec![text("d")]
                        }
                    ]
                }
            ]
        );
        assert_eq!(parse("1 < 2 and <0>open"), [text("1 < 2 and <0>open")]);
        assert_eq!(parse("stray </0> close"), [text("stray </0> close")]);
    }
}
