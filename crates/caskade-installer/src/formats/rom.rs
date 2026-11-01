//! Minimal Platform-definition adapter until the emulation source registry exists.
//! Callers can replace this catalogue with emulation/plugin definitions by ID.
use crate::{
    ArtifactNode, Candidate, Fidelity, Format, HandOff, HandlerId, InstallOutcome, PayloadPlatform,
    Requirements, Role, Strategy,
    plan::{Handler, PlanContext, PlanError, Proposal},
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct HeaderSignature {
    pub offset: u64,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
pub struct PlatformDefinition {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    #[serde(default)]
    pub signatures: Vec<HeaderSignature>,
    /// Reject suffix-only matches for formats with common non-ROM extensions.
    #[serde(default)]
    pub require_header: bool,
}
#[derive(Debug, Clone)]
pub struct RomProbe {
    platforms: Vec<PlatformDefinition>,
}
impl Default for RomProbe {
    fn default() -> Self {
        #[derive(Deserialize)]
        struct Table {
            platforms: Vec<PlatformDefinition>,
        }
        Self::new(
            toml::from_str::<Table>(include_str!("../../data/rom-platforms.toml"))
                .expect("embedded Platform table is valid")
                .platforms,
        )
    }
}
impl RomProbe {
    pub fn new(platforms: Vec<PlatformDefinition>) -> Self {
        Self { platforms }
    }
    pub fn platforms(&self) -> &[PlatformDefinition] {
        &self.platforms
    }
    fn identify(
        &self,
        source: &mut ProbeSource<'_>,
        name: &Path,
    ) -> Result<Option<Detection>, ProbeError> {
        // Strong signatures win over suffixes; every header read is bounded.
        for platform in &self.platforms {
            for signature in &platform.signatures {
                if signature.bytes.is_empty() || signature.bytes.len() > 4096 {
                    continue;
                }
                if signature
                    .offset
                    .checked_add(signature.bytes.len() as u64)
                    .is_some_and(|end| end <= source.size())
                    && source.read_at(signature.offset, signature.bytes.len())? == signature.bytes
                {
                    return Ok(Some(detection(platform, "header")));
                }
            }
        }
        let header = source.read_at(0, (source.size().min(16)) as usize)?;
        // Never route a renamed executable/archive/disc to emulation by suffix.
        if [
            b"MZ".as_slice(),
            b"PK",
            b"7z",
            b"Rar!",
            b"\x7fELF",
            b"MSCF",
            b"\x1f\x8b",
        ]
        .iter()
        .any(|magic| header.starts_with(magic))
        {
            return Ok(None);
        }
        let ext = name.extension().and_then(|s| s.to_str()).unwrap_or("");
        Ok(self
            .platforms
            .iter()
            .find(|p| !p.require_header && p.extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
            .map(|platform| detection(platform, "extension")))
    }
}
fn detection(platform: &PlatformDefinition, evidence: &str) -> Detection {
    let mut result = Detection::new(
        Format::Rom,
        PayloadPlatform::ConsoleRom {
            platform: Some(platform.id.clone()),
        },
        false,
    );
    result.role = Some(Role::Data);
    result.facts = BTreeMap::from([
        ("rom.platform".into(), platform.id.clone().into()),
        ("rom.evidence".into(), evidence.into()),
    ]);
    result
}
impl FormatProbe for RomProbe {
    fn id(&self) -> &str {
        "rom"
    }
    fn detect_named(
        &self,
        source: &mut ProbeSource<'_>,
        name: &Path,
    ) -> Result<Option<Detection>, ProbeError> {
        self.identify(source, name)
    }
}
#[derive(Debug, Default)]
pub struct RomHandler;
impl RomHandler {
    /// Explicit application-boundary outcome; hand-offs never enter Executor.
    pub fn handoff(&self, artifact: &ArtifactNode) -> Option<InstallOutcome> {
        let PayloadPlatform::ConsoleRom {
            platform: Some(platform),
        } = &artifact.platform
        else {
            return None;
        };
        if artifact.format != Format::Rom
            || artifact.path.is_absolute()
            || artifact
                .path
                .components()
                .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            return None;
        }
        Some(InstallOutcome::HandOff(HandOff::Rom {
            platform: platform.clone(),
            files: vec![artifact.path.clone()],
        }))
    }
}
impl Handler for RomHandler {
    fn id(&self) -> HandlerId {
        HandlerId("rom".into())
    }
    fn claims(&self, artifact: &ArtifactNode) -> bool {
        self.handoff(artifact).is_some()
    }
    fn propose(&self, artifact: &ArtifactNode, _: &PlanContext<'_>) -> Result<Proposal, PlanError> {
        if !self.claims(artifact) {
            return Ok(Proposal::default());
        }
        Ok(Proposal {
            candidates: vec![Candidate {
                handler: self.id(),
                strategy: Strategy::HandOff,
                confidence: if artifact.facts.get("rom.evidence").and_then(|v| v.as_str())
                    == Some("header")
                {
                    1.0
                } else {
                    0.9
                },
                fidelity: Fidelity::Exact,
                requires: Requirements {
                    est_disk_bytes: Some(0),
                    ..Default::default()
                },
                steps: vec![],
                warnings: vec![],
            }],
            question: None,
        })
    }
}
