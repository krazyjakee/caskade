use super::{
    InstallRecord, ManifestError, Result,
    paths::{normalize_link, normalize_path},
};
use crate::{ArtifactNode, Input, Program, RuntimeSpec, SourcePath, Step};
use std::collections::BTreeSet;
pub(super) fn identifier(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty()
        || value != value.trim()
        || value.chars().any(char::is_control)
        || value.contains(['/', '\\'])
    {
        return Err(ManifestError::Invalid(format!(
            "{name} must be a nonempty identifier"
        )));
    }
    Ok(())
}
pub(super) fn input(input: &mut Input) -> Result<()> {
    match input {
        Input::Path(path) => normalize_path(path, false)?,
        Input::Urls(urls) => {
            // A magnet link (#159) stands alone.
            let magnet = urls.len() == 1 && urls[0].scheme() == "magnet";
            if urls.is_empty()
                || !magnet
                    && urls
                        .iter()
                        .any(|url| !matches!(url.scheme(), "http" | "https"))
            {
                return Err(ManifestError::Invalid(
                    "source URLs must be a nonempty HTTP(S) set or one magnet link".into(),
                ));
            }
        }
    }
    Ok(())
}

fn runtime(runtime: &RuntimeSpec) -> Result<()> {
    match runtime {
        RuntimeSpec::Native => Ok(()),
        RuntimeSpec::ProtonPrefix { id } | RuntimeSpec::WinePrefix { id } => {
            identifier(id, "runtime prefix id")
        }
    }
}

fn artifact(node: &mut ArtifactNode) -> Result<()> {
    normalize_path(&mut node.path, true)?;
    for child in &mut node.children {
        artifact(child)?;
    }
    Ok(())
}

fn source(source: &mut SourcePath, allow_root: bool) -> Result<()> {
    match source {
        SourcePath::Artifact { path, .. } | SourcePath::Game(path) => {
            normalize_path(path, allow_root)
        }
    }
}

fn program(program: &mut Program) -> Result<()> {
    match program {
        // Artifact "." denotes the selected source file itself. A game root
        // is a directory and can never be executed as a program.
        Program::Source(path @ SourcePath::Artifact { .. }) => source(path, true),
        Program::Source(path @ SourcePath::Game(_)) => source(path, false),
        Program::Tool(tool) => identifier(tool, "tool name"),
    }
}

fn steps(steps: &mut [Step]) -> Result<()> {
    for step in steps {
        match step {
            Step::Extract { strip, to, .. } => {
                if let Some(strip) = strip {
                    normalize_path(strip, true)?;
                }
                normalize_path(to, true)?;
            }
            Step::Copy { from, to } | Step::Move { from, to } => {
                source(from, true)?;
                normalize_path(to, true)?;
            }
            Step::Run {
                program: executable,
                runtime: runner,
                timeout_secs,
                expect,
                ..
            } => {
                program(executable)?;
                runtime(runner)?;
                if *timeout_secs == 0 {
                    return Err(ManifestError::Invalid("run timeout must be nonzero".into()));
                }
                for path in expect {
                    normalize_path(path, false)?;
                }
            }
            Step::RunInteractive {
                program: executable,
                runtime: runner,
                target_dir,
                ..
            } => {
                program(executable)?;
                runtime(runner)?;
                normalize_path(target_dir, true)?;
            }
            Step::Mount { mount_point, .. } | Step::Unmount { mount_point } => {
                normalize_path(mount_point, true)?
            }
            Step::SetExecutable { path }
            | Step::WriteFile { path, .. }
            | Step::CopyRange { path, .. } => normalize_path(path, false)?,
            Step::Symlink { path, target } => {
                normalize_path(path, false)?;
                normalize_link(path, target)?;
            }
            Step::RegistryImport {
                runtime: runner, ..
            } => {
                runtime(runner)?;
                if *runner == RuntimeSpec::Native {
                    return Err(ManifestError::Invalid(
                        "registry replay requires a prefix runtime".into(),
                    ));
                }
            }
            Step::Delegate {
                tool,
                uninstall,
                only_if,
                ..
            } => {
                identifier(tool, "delegate tool")?;
                for command in uninstall {
                    identifier(&command.tool, "uninstall tool")?;
                    if let Some(check) = &command.only_if {
                        identifier(&check.tool, "uninstall check tool")?;
                    }
                }
                if let Some(check) = only_if {
                    identifier(&check.tool, "delegate check tool")?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn record(record: &mut InstallRecord) -> Result<()> {
    if record.title.trim().is_empty() {
        return Err(ManifestError::Invalid("title must not be empty".into()));
    }
    if let Some(alias) = &record.alias
        && crate::runtime::alias::AliasStore::new("")
            .path(alias)
            .is_err()
    {
        return Err(ManifestError::Invalid(format!(
            "invalid alias id {alias:?}"
        )));
    }
    input(&mut record.source.input)?;
    if let Some(url) = &record.source.final_url
        && (!matches!(url.scheme(), "http" | "https")
            || !matches!(record.source.input, Input::Urls(_)))
    {
        return Err(ManifestError::Invalid(
            "final URL requires an HTTP(S) URL source".into(),
        ));
    }
    // The live plan may retain an absolute source selection. Persist its portable
    // provenance snapshot instead; destination paths are never relativized by
    // dropping prefixes, and still fail validation when absolute or traversing.
    record.plan.input = record.source.input.clone();
    if record.plan.schema_version != 1 {
        return Err(ManifestError::Invalid(
            "unsupported executed-plan schema version".into(),
        ));
    }
    identifier(&record.plan.root_id, "plan root id")?;
    normalize_path(&mut record.plan.relative_dir, false)?;
    for node in &mut record.plan.artifacts {
        artifact(node)?;
    }
    let candidate = &mut record.plan.candidate;
    if !candidate.confidence.is_finite() || !(0.0..=1.0).contains(&candidate.confidence) {
        return Err(ManifestError::Invalid(
            "candidate confidence must be finite and in 0..=1".into(),
        ));
    }
    if let Some(runner) = &candidate.requires.runtime {
        runtime(runner)?;
    }
    steps(&mut candidate.steps)?;
    for warning in &mut candidate.warnings {
        if let Some(path) = &mut warning.path {
            normalize_path(path, false)?;
        }
    }
    let mut handlers = BTreeSet::new();
    for handler in &record.handlers {
        identifier(&handler.id.0, "handler id")?;
        identifier(&handler.version, "handler version")?;
        if !handlers.insert(handler.id.clone()) {
            return Err(ManifestError::Invalid("duplicate handler id".into()));
        }
    }
    if !handlers.contains(&candidate.handler) {
        return Err(ManifestError::Invalid(
            "executed handler has no recorded version".into(),
        ));
    }
    let mut files = BTreeSet::new();
    for file in &record.files {
        if !files.insert(file.path.clone()) {
            return Err(ManifestError::Invalid(format!(
                "duplicate installed file: {}",
                file.path
            )));
        }
        if file.path.as_str() == super::INSTALL_MANIFEST_PATH {
            return Err(ManifestError::Invalid(
                "manifest cannot include itself in its file list".into(),
            ));
        }
    }
    for target in &mut record.launch_targets {
        normalize_path(&mut target.path, false)?;
        normalize_path(&mut target.working_dir, true)?;
        runtime(&target.runtime)?;
        if !target.score.is_finite() {
            return Err(ManifestError::Invalid("launch score must be finite".into()));
        }
    }
    runtime(&record.runtime)?;
    steps(&mut record.uninstall_steps)?;
    for effect in &record.external_effects {
        identifier(&effect.location().scope, "external scope")?;
        if let super::ExternalEffect::Registry { effect } = effect {
            effect.validate()?;
        }
    }
    Ok(())
}
