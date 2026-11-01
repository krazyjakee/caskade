use crate::{
    Format,
    host::HostCaps,
    runtime::{ProcessCommand, Runtime},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("package I/O: {0}")]
    Io(#[from] io::Error),
    #[error("unsafe package entry: {0}")]
    Safety(#[from] crate::safety::SafetyError),
    #[error("package archive: {0}")]
    Archive(#[from] crate::formats::archive::ArchiveError),
    #[error("invalid package: {0}")]
    Invalid(String),
    #[error("unsupported package feature: {0}")]
    Unsupported(String),
    #[error("package metadata limit exceeded")]
    MetadataLimit,
    #[error("system package manager required: {0}")]
    RequiresDelegate(String),
    #[error("dependency query: {0}")]
    Runtime(#[from] crate::runtime::RuntimeError),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageDependency {
    pub name: String,
    pub constraint: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    /// Each outer item is required; any alternative in an inner group suffices.
    pub dependencies: Vec<Vec<PackageDependency>>,
    pub maintainer_scripts: Vec<String>,
    pub absolute_paths: bool,
    pub format: Format,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyReport {
    pub installed: Vec<String>,
    pub missing: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedReference {
    pub path: PathBuf,
    pub key: String,
    pub original: String,
    pub replacement: String,
    pub origin: String,
}
pub(crate) fn package_name(name: &str) -> super::Result<()> {
    if name.is_empty()
        || name.len() > 256
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+.-_:".contains(&b))
    {
        return Err(PackageError::Invalid("invalid package name".into()));
    }
    Ok(())
}
pub(super) fn debian_dependencies(value: &str) -> super::Result<Vec<Vec<PackageDependency>>> {
    if value.len() > 64 * 1024 {
        return Err(PackageError::MetadataLimit);
    }
    let mut result = Vec::new();
    for group in value.split(',') {
        let mut alternatives = Vec::new();
        for dependency in group.split('|') {
            let dependency = dependency.trim();
            let end = dependency
                .find([' ', '(', '[', '<'])
                .unwrap_or(dependency.len());
            let name = &dependency[..end];
            package_name(name)?;
            let rest = dependency[end..].trim();
            let constraint = if rest.is_empty() {
                None
            } else {
                Some(rest.into())
            };
            alternatives.push(PackageDependency {
                name: name.into(),
                constraint,
            });
            if alternatives.len() > 64 {
                return Err(PackageError::MetadataLimit);
            }
        }
        if result.len() >= 1024 {
            return Err(PackageError::MetadataLimit);
        }
        result.push(alternatives);
    }
    Ok(result)
}
pub fn dependency_report(info: &PackageInfo, host: &HostCaps) -> DependencyReport {
    let mut report = DependencyReport::default();
    for group in &info.dependencies {
        if let Some(dep) = group
            .iter()
            .find(|dep| dep.constraint.is_none() && host.packages.installed.contains(&dep.name))
        {
            report.installed.push(dep.name.clone());
        } else {
            report.missing.push(
                group
                    .iter()
                    .map(|d| d.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
        }
    }
    report
}
/// Boundary operation, never called by a probe or planner. Runtime and command
/// locations are injected; nonzero package queries conservatively mean absent.
/// Version/architecture-qualified dependencies require the manager's solver.
pub fn check_dependencies(
    runtime: &dyn Runtime,
    host: &HostCaps,
    info: &PackageInfo,
    cwd: &Path,
) -> super::Result<DependencyReport> {
    let mut inventory = host.clone();
    let query = if info.format == Format::Deb {
        "dpkg-query"
    } else {
        "rpm"
    };
    let program = host
        .packages
        .managers
        .get(query)
        .ok_or_else(|| PackageError::Unsupported(format!("{query} is not configured")))?;
    for group in &info.dependencies {
        for dep in group {
            if inventory.packages.installed.contains(&dep.name) || dep.constraint.is_some() {
                continue;
            }
            if info.format == Format::Deb {
                package_name(&dep.name)?;
            } else if dep.name.is_empty()
                || dep.name.len() > 1024
                || dep.name.starts_with('-')
                || dep.name.chars().any(char::is_control)
            {
                return Err(PackageError::Invalid(
                    "invalid RPM capability dependency".into(),
                ));
            }
            let args = if query == "dpkg-query" {
                vec!["--status".into(), "--".into(), dep.name.clone()]
            } else {
                vec![
                    "-q".into(),
                    "--whatprovides".into(),
                    "--".into(),
                    dep.name.clone(),
                ]
            };
            let process = runtime.spawn(&ProcessCommand {
                program: program.clone(),
                args,
                env: BTreeMap::new(),
                cwd: cwd.into(),
            })?;
            let exit = match runtime.wait(process, Duration::from_secs(10)) {
                Ok(exit) => exit,
                Err(error) => {
                    let _ = runtime.kill(process);
                    return Err(error.into());
                }
            };
            if exit.code == 0 {
                inventory.packages.installed.insert(dep.name.clone());
            }
        }
    }
    Ok(dependency_report(info, &inventory))
}
pub(crate) struct Budget<R> {
    inner: R,
    remaining: u64,
}
impl<R> Budget<R> {
    pub fn new(inner: R, remaining: u64) -> Self {
        Self { inner, remaining }
    }
}
impl<R: Read> Read for Budget<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut byte = [0];
            return if self.inner.read(&mut byte)? == 0 {
                Ok(0)
            } else {
                Err(io::Error::other(PackageError::MetadataLimit))
            };
        }
        let length = buffer.len().min(self.remaining.min(64 * 1024) as usize);
        let count = self.inner.read(&mut buffer[..length])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}
