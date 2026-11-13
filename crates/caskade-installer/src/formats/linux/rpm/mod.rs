//! Pure-Rust RPM metadata and streaming newc payload handling.
mod payload;
use super::deb::{PackageDependency, PackageError, PackageInfo, Result, Span};
use crate::Format;
pub use payload::{extract, list};
use std::io::{Cursor, Read, Seek, SeekFrom};
pub const MAX_RPM_METADATA: usize = 16 * 1024 * 1024;
#[derive(Debug)]
pub struct Rpm {
    pub info: PackageInfo,
    pub payload: Span,
    pub compression: rpm::CompressionType,
}
fn be(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn section(reader: &mut impl Read, budget: &mut usize) -> Result<Vec<u8>> {
    let mut head = [0; 16];
    reader.read_exact(&mut head)?;
    if &head[..4] != b"\x8e\xad\xe8\x01" {
        return Err(PackageError::Invalid("invalid RPM header magic".into()));
    }
    let count = be(&head, 8) as usize;
    let size = be(&head, 12) as usize;
    let length = count
        .checked_mul(16)
        .and_then(|n| n.checked_add(size))
        .ok_or(PackageError::MetadataLimit)?;
    if count > 100_000 || length > *budget {
        return Err(PackageError::MetadataLimit);
    }
    *budget -= length;
    let mut data = vec![0; length];
    reader.read_exact(&mut data)?;
    let store = &data[count * 16..];
    let mut scanned = 0_usize;
    let mut tags = std::collections::BTreeSet::new();
    for entry in data[..count * 16].as_chunks::<16>().0 {
        if !tags.insert(be(entry, 0)) {
            return Err(PackageError::Invalid("duplicate RPM tag".into()));
        }
        let ty = be(entry, 4);
        let offset = be(entry, 8) as usize;
        let items = be(entry, 12) as usize;
        let remaining = store
            .get(offset..)
            .ok_or_else(|| PackageError::Invalid("RPM tag outside data store".into()))?;
        if items > 1_000_000 {
            return Err(PackageError::MetadataLimit);
        }
        let width = match ty {
            0 => 0,
            1 | 2 | 7 => 1,
            3 => 2,
            4 => 4,
            5 => 8,
            6 | 8 | 9 => 0,
            _ => return Err(PackageError::Unsupported("RPM tag data type".into())),
        };
        if width != 0 && items.checked_mul(width).is_none_or(|n| n > remaining.len()) {
            return Err(PackageError::Invalid("RPM tag count exceeds store".into()));
        }
        if matches!(ty, 6 | 8 | 9) {
            if (ty == 6 && items != 1) || items > 100_000 {
                return Err(PackageError::Invalid("RPM string count".into()));
            }
            let mut found = 0;
            let mut consumed = 0;
            for byte in remaining {
                consumed += 1;
                if *byte == 0 {
                    found += 1;
                    if found == items {
                        break;
                    }
                }
            }
            scanned = scanned
                .checked_add(consumed)
                .ok_or(PackageError::MetadataLimit)?;
            if scanned > MAX_RPM_METADATA * 4 {
                return Err(PackageError::MetadataLimit);
            }
            if found != items {
                return Err(PackageError::Invalid("unterminated RPM string data".into()));
            }
        }
    }
    Ok([head.as_slice(), data.as_slice()].concat())
}
/// Bounded header preflight protects rpm-rs from oversized allocations and tag
/// offset slicing before its parser is called. Exposed for robustness targets.
pub fn inspect(reader: &mut (impl Read + Seek)) -> Result<Rpm> {
    let size = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut lead = [0; 96];
    reader.read_exact(&mut lead)?;
    if &lead[..4] != b"\xed\xab\xee\xdb" {
        return Err(PackageError::Invalid("not RPM".into()));
    }
    let mut budget = MAX_RPM_METADATA;
    let signature = section(reader, &mut budget)?;
    let padding = (8 - signature.len() % 8) % 8;
    let mut pad = vec![0; padding];
    reader.read_exact(&mut pad)?;
    let header = section(reader, &mut budget)?;
    let offset = 96 + signature.len() as u64 + padding as u64 + header.len() as u64;
    if offset >= size {
        return Err(PackageError::Invalid("RPM has no payload".into()));
    }
    let bytes = [
        lead.as_slice(),
        signature.as_slice(),
        pad.as_slice(),
        header.as_slice(),
    ]
    .concat();
    let metadata = rpm::PackageMetadata::parse(&mut Cursor::new(bytes))
        .map_err(|e| PackageError::Invalid(e.to_string()))?;
    let name = metadata
        .get_name()
        .map_err(|e| PackageError::Invalid(e.to_string()))?
        .to_owned();
    super::deb::common::package_name(&name)?;
    let version = metadata
        .get_version()
        .map_err(|e| PackageError::Invalid(e.to_string()))?
        .to_owned();
    let names = metadata
        .header
        .get_entry_data_as_string_array(rpm::IndexTag::RPMTAG_REQUIRENAME);
    let flags = metadata
        .header
        .get_entry_data_as_u32_array(rpm::IndexTag::RPMTAG_REQUIREFLAGS);
    let versions = metadata
        .header
        .get_entry_data_as_string_array(rpm::IndexTag::RPMTAG_REQUIREVERSION);
    match (&names, &flags, &versions) {
        (Ok(names), Ok(flags), Ok(versions))
            if names.len() == flags.len() && names.len() == versions.len() => {}
        (
            Err(rpm::Error::TagNotFound(_)),
            Err(rpm::Error::TagNotFound(_)),
            Err(rpm::Error::TagNotFound(_)),
        ) => (),
        _ => {
            return Err(PackageError::Invalid(
                "inconsistent RPM dependency arrays".into(),
            ));
        }
    }
    let requires = metadata
        .get_requires()
        .map_err(|e| PackageError::Invalid(e.to_string()))?;
    let dependencies = requires
        .into_iter()
        .filter(|dep| !dep.name.starts_with("rpmlib(") && !script_only(dep.flags))
        .map(|dep| {
            vec![PackageDependency {
                name: dep.name,
                constraint: (!dep.version.is_empty()).then_some(dep.version),
            }]
        })
        .collect();
    let mut scripts = Vec::new();
    for (name, script) in [
        ("pre-install", metadata.get_pre_install_script()),
        ("post-install", metadata.get_post_install_script()),
        ("pre-uninstall", metadata.get_pre_uninstall_script()),
        ("post-uninstall", metadata.get_post_uninstall_script()),
        ("pre-trans", metadata.get_pre_trans_script()),
        ("post-trans", metadata.get_post_trans_script()),
    ] {
        if script.is_ok() {
            scripts.push(name.into());
        }
    }
    let compression = metadata
        .get_payload_compressor()
        .map_err(|e| PackageError::Unsupported(e.to_string()))?;
    Ok(Rpm {
        info: PackageInfo {
            name,
            version,
            dependencies,
            maintainer_scripts: scripts,
            absolute_paths: false,
            format: Format::Rpm,
        },
        payload: Span {
            offset,
            length: size - offset,
        },
        compression,
    })
}

#[derive(Default)]
pub struct RpmHandler;
impl crate::plan::Handler for RpmHandler {
    fn id(&self) -> crate::HandlerId {
        crate::HandlerId("rpm".into())
    }
    fn claims(&self, node: &crate::ArtifactNode) -> bool {
        node.format == Format::Rpm
    }
    fn propose(
        &self,
        node: &crate::ArtifactNode,
        context: &crate::plan::PlanContext<'_>,
    ) -> std::result::Result<crate::plan::Proposal, crate::plan::PlanError> {
        super::deb::adapter::propose(self.id(), node, context)
    }
}

// Only explicit script contexts may be skipped alongside the scripts themselves.
// Legacy PREREQ, auto-generated runtime requirements and unknown flag bits remain
// requirements. Comparison flags describe versions, not an additional context.
fn script_only(flags: rpm::DependencyFlags) -> bool {
    use rpm::DependencyFlags as F;
    let scripts = F::INTERP
        | F::SCRIPT_PRE
        | F::SCRIPT_POST
        | F::SCRIPT_PREUN
        | F::SCRIPT_POSTUN
        | F::SCRIPT_VERIFY
        | F::PRETRANS
        | F::POSTTRANS
        | F::PREUNTRANS
        | F::POSTUNTRANS
        | F::TRIGGERIN
        | F::TRIGGERUN
        | F::TRIGGERPOSTUN
        | F::TRIGGERPREIN;
    let comparisons = F::LESS | F::GREATER | F::EQUAL;
    flags.intersects(scripts) && (flags.bits() & !(scripts | comparisons).bits()) == 0
}

#[cfg(test)]
mod dependency_tests {
    use super::script_only;
    use rpm::DependencyFlags as F;
    #[test]
    fn only_explicit_script_contexts_are_omitted_when_scripts_are_skipped() {
        assert!(script_only(F::INTERP | F::SCRIPT_POST));
        assert!(script_only(F::SCRIPT_PRE | F::GREATER | F::EQUAL));
        for flags in [
            F::ANY,
            F::PREREQ,
            F::FIND_REQUIRES,
            F::SCRIPT_POST | F::FIND_REQUIRES,
            F::from_bits_retain(F::SCRIPT_POST.bits() | (1 << 31)),
        ] {
            assert!(!script_only(flags));
        }
    }
}
