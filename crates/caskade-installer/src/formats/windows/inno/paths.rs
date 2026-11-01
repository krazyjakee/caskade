use super::{InnoError, Result};
use std::path::{Path, PathBuf};

/// None denotes a non-{app} destination, which extraction deliberately excludes.
/// Traversal and unresolved constants within {app} are errors, never rewritten.
pub fn app_path(value: &str) -> Result<Option<PathBuf>> {
    let value = value.replace('\\', "/");
    if value.eq_ignore_ascii_case("{app}") {
        return Ok(Some(PathBuf::from(".")));
    }
    let Some(prefix) = value.get(..6) else {
        return Ok(None);
    };
    if !prefix.eq_ignore_ascii_case("{app}/") {
        return Ok(None);
    }
    let rest = &value[6..];
    if rest.contains(['{', '}']) {
        return Err(InnoError::Unsupported(
            "unresolved constant in {app} path".into(),
        ));
    }
    Ok(Some(crate::safety::sanitize_path(rest)?))
}

pub(super) fn selected_path(
    name: &str,
    select: &globset::GlobSet,
    strip: Option<&Path>,
) -> Result<Option<String>> {
    if !select.is_empty() && !select.is_match(name) {
        return Ok(None);
    }
    let path = Path::new(name);
    let path = if let Some(strip) = strip {
        match path.strip_prefix(strip) {
            Ok(path) => path,
            Err(_) => return Ok(None),
        }
    } else {
        path
    };
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    let name = path
        .to_str()
        .ok_or_else(|| InnoError::Invalid("non-Unicode entry".into()))?;
    Ok(Some(
        crate::safety::sanitize_path(name)?
            .to_string_lossy()
            .replace('\\', "/"),
    ))
}

/// Windows argv quoting, parsed on any host; no shell evaluation or expansion.
pub fn split_arguments(value: &str) -> Result<Vec<String>> {
    if value.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
        return Err(InnoError::Invalid("control character in arguments".into()));
    }
    let input: Vec<_> = value.chars().collect();
    let mut result = Vec::new();
    let mut position = 0;
    while position < input.len() {
        while position < input.len() && input[position].is_ascii_whitespace() {
            position += 1;
        }
        if position == input.len() {
            break;
        }
        let mut argument = String::new();
        let mut quoted = false;
        while position < input.len() && (quoted || !input[position].is_ascii_whitespace()) {
            let mut slashes = 0;
            while position < input.len() && input[position] == '\\' {
                slashes += 1;
                position += 1;
            }
            if position < input.len() && input[position] == '"' {
                argument.extend(std::iter::repeat_n('\\', slashes / 2));
                if slashes % 2 == 1 {
                    argument.push('"');
                } else if quoted && input.get(position + 1) == Some(&'"') {
                    argument.push('"');
                    position += 1;
                } else {
                    quoted = !quoted;
                }
                position += 1;
            } else {
                argument.extend(std::iter::repeat_n('\\', slashes));
                if position < input.len() {
                    argument.push(input[position]);
                    position += 1;
                }
            }
        }
        if quoted {
            return Err(InnoError::Invalid("unclosed argument quote".into()));
        }
        result.push(argument);
    }
    Ok(result)
}
