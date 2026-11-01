use crate::{
    Format, PayloadPlatform,
    probe::{Detection, FormatProbe, ProbeError, ProbeSource},
};
use std::path::Path;

#[derive(Default)]
pub struct DiscProbe;
impl FormatProbe for DiscProbe {
    fn id(&self) -> &str {
        "disc"
    }
    fn detect_named(
        &self,
        source: &mut ProbeSource<'_>,
        name: &Path,
    ) -> Result<Option<Detection>, ProbeError> {
        if name
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("cue"))
            && source.size() <= 1024 * 1024
        {
            let bytes = source.read_at(0, source.size() as usize)?;
            if let Ok(text) =
                crate::safety::decode_name(&bytes, crate::safety::NameEncoding::Detect)
                && let Ok(track) = super::parse_cue(&text)
            {
                let mut result = Detection::new(Format::Cue, PayloadPlatform::Unknown, true);
                result.facts.insert(
                    "disc.track".into(),
                    serde_json::to_value(track).expect("track serializes"),
                );
                return Ok(Some(result));
            }
        }
        if source.size() >= 8 && source.read_at(0, 8)? == b"MComprHD" {
            return Ok(Some(Detection::new(
                Format::Chd,
                PayloadPlatform::Unknown,
                true,
            )));
        }
        // ISO9660 CD001 and UDF volume-recognition descriptors. No mount or
        // sidecar process runs during this bounded magic-only probe.
        for sector in 16..=32 {
            let offset = sector * 2048 + 1;
            if source.size() < offset + 5 {
                break;
            }
            let id = source.read_at(offset, 5)?;
            if matches!(id.as_slice(), b"CD001" | b"NSR02" | b"NSR03") {
                let mut result = Detection::new(Format::Iso, PayloadPlatform::Unknown, true);
                result.facts.insert(
                    "disc.filesystem".into(),
                    if id == b"CD001" { "iso9660" } else { "udf" }.into(),
                );
                return Ok(Some(result));
            }
        }
        Ok(None)
    }
}
