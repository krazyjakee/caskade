use super::*;
/// Decode a recorded Windows command into argv, never a shell expression.
/// An unquoted executable containing spaces is accepted only when its complete
/// .exe path exists in the injected filesystem (Windows legacy ARP convention).
pub fn command_target(env: &HostEnv, command: &str) -> Option<LaunchTarget> {
    if command.len() > 32768 || command.chars().any(char::is_control) {
        return None;
    }
    let expanded = env.expand_vars(command);
    let command = expanded.to_str()?.trim();
    let mut parts = crate::parsers::lnk::split_arguments(command).ok()?;
    if !command.starts_with('"') {
        let lower = command.to_ascii_lowercase();
        for (offset, _) in lower.match_indices(".exe") {
            let end = offset + 4;
            if command
                .get(end..)
                .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
            {
                let candidate = &command[..end];
                if let Some(path) = resolve_path(env, candidate)
                    && path.is_file()
                {
                    parts = vec![candidate.to_owned()];
                    parts.extend(crate::parsers::lnk::split_arguments(command[end..].trim()).ok()?);
                    break;
                }
            }
        }
    }
    let program = parts.first()?;
    if program.contains("://") {
        return (parts.len() == 1).then(|| LaunchTarget::Uri {
            uri: program.clone(),
        });
    }
    let program = if absolute_windows(program) || Path::new(program).is_absolute() {
        resolve_path(env, program)?
    } else {
        // Keep a bare executable for the recorded platform's PATH. Relative
        // executable directories are ambiguous and must not resolve at cwd.
        if program.contains(['/', '\\', ':']) || program.starts_with('-') {
            return None;
        }
        PathBuf::from(program)
    };
    Some(LaunchTarget::Exec {
        program,
        args: parts.into_iter().skip(1).collect(),
        cwd: None,
        env: BTreeMap::new(),
    })
}
pub(super) fn absolute_windows(value: &str) -> bool {
    (value.len() >= 3
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes()[1] == b':'
        && matches!(value.as_bytes()[2], b'\\' | b'/'))
        || value.starts_with(r"\\")
}
pub(super) fn resolve_path(env: &HostEnv, value: &str) -> Option<PathBuf> {
    let expanded = env.expand_vars(value.trim().trim_matches('"'));
    let text = expanded.to_str()?;
    if text.chars().any(char::is_control)
        || !(absolute_windows(text) || Path::new(text).is_absolute())
    {
        return None;
    }
    env.resolve_path(Path::new(text)).ok()
}
pub(super) fn icon_path(env: &HostEnv, value: &str) -> Option<PathBuf> {
    let value = value.trim().trim_start_matches('@');
    let value = value
        .rsplit_once(',')
        .filter(|(_, index)| index.trim().parse::<i32>().is_ok())
        .map_or(value, |(path, _)| path);
    resolve_path(env, value)
}
