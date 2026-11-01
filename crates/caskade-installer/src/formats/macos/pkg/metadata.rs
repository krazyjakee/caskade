use super::{PkgError, Result, Xar, XarLimits, invalid};
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek},
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    pub path: PathBuf,
    pub identifier: String,
    pub version: String,
    pub install_location: String,
    pub has_scripts: bool,
    pub scripts_required: bool,
    pub system_target: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub components: Vec<Component>,
    pub product: bool,
    pub requires_delegate: bool,
}
fn attributes(
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<std::collections::BTreeMap<String, String>> {
    let mut values = std::collections::BTreeMap::new();
    for attribute in element.attributes().with_checks(true) {
        if values.len() >= 64 {
            return Err(PkgError::Limit);
        }
        let attribute = attribute.map_err(|_| invalid("PackageInfo attributes"))?;
        let key = attribute.key.as_ref().to_owned();
        let raw = attribute.value.as_ref();
        let value = quick_xml::escape::unescape(raw)
            .map_err(|_| invalid("PackageInfo entity"))?
            .into_owned();
        if key.len() > 128 || value.len() > 4096 {
            return Err(PkgError::Limit);
        }
        values.insert(key, value);
    }
    Ok(values)
}
fn component(bytes: &[u8], path: PathBuf, has_scripts: bool) -> Result<Component> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("PackageInfo encoding"))?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut depth = 0_u32;
    let mut identifier = None;
    let mut version = None;
    let mut location = None;
    let mut scripts_required = false;
    let mut count = 0;
    loop {
        count += 1;
        if count > 100_000 {
            return Err(PkgError::Limit);
        }
        let event = reader
            .read_event()
            .map_err(|_| invalid("PackageInfo XML"))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(element) | Event::Empty(element) => {
                let name = element.name();
                let name = name.as_ref();
                if depth == 0 {
                    if name != "pkg-info" || identifier.is_some() {
                        return Err(invalid("PackageInfo root"));
                    }
                    let mut attrs = attributes(&element)?;
                    identifier = attrs.remove("identifier");
                    version = attrs.remove("version");
                    location = attrs.remove("install-location");
                } else if matches!(
                    name,
                    "preinstall" | "postinstall" | "preupgrade" | "postupgrade"
                ) {
                    scripts_required = true;
                }
                if !empty {
                    depth += 1;
                    if depth > 64 {
                        return Err(PkgError::Limit);
                    }
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("PackageInfo balance"))?;
            }
            Event::DocType(_) => return Err(invalid("PackageInfo DTD")),
            Event::Eof => break,
            _ => (),
        }
    }
    if depth != 0 {
        return Err(invalid("incomplete PackageInfo"));
    }
    let identifier = identifier.ok_or_else(|| invalid("PackageInfo identifier"))?;
    if identifier.is_empty()
        || identifier.len() > 256
        || !identifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(invalid("package identifier"));
    }
    let version = version
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("PackageInfo version"))?;
    let location = location.unwrap_or_else(|| "/".into());
    let system_target = location != "/"
        && location != "/Applications"
        && !location.strip_prefix("/Applications/").is_some_and(|path| {
            crate::safety::sanitize_path(path).is_ok_and(|p| {
                p.components().count() == 1
                    && p.extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            })
        });
    Ok(Component {
        path,
        identifier,
        version,
        install_location: location,
        has_scripts,
        scripts_required,
        system_target,
    })
}
pub fn inspect(source: &mut (impl Read + Seek), limits: XarLimits) -> Result<(Xar, Package)> {
    let xar = Xar::inspect(source, limits)?;
    let mut components = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut total = 0_usize;
    for entry in &xar.toc.entries {
        if entry.directory || entry.path.file_name() != Some(std::ffi::OsStr::new("PackageInfo")) {
            continue;
        }
        if components.len() >= 128 {
            return Err(PkgError::Limit);
        }
        let root = entry.path.parent().unwrap_or(Path::new(""));
        if xar.entry(&root.join("Payload")).is_none() {
            return Err(PkgError::Unsupported("component without a payload".into()));
        }
        let bytes = xar.read_metadata(source, entry, 1024 * 1024)?;
        total = total.checked_add(bytes.len()).ok_or(PkgError::Limit)?;
        if total > 4 * 1024 * 1024 {
            return Err(PkgError::Limit);
        }
        let component = component(
            &bytes,
            root.into(),
            xar.entry(&root.join("Scripts")).is_some(),
        )?;
        if !ids.insert(component.identifier.clone()) {
            return Err(invalid("duplicate component identifier"));
        }
        components.push(component);
    }
    if components.is_empty() {
        return Err(invalid("no component PackageInfo/Payload pair"));
    }
    let product = xar.entry(Path::new("Distribution")).is_some();
    // A Distribution can execute JavaScript and choose optional/remote
    // components. Expansion must not guess those installer decisions.
    let requires_delegate = product
        || components
            .iter()
            .any(|component| component.system_target || component.scripts_required);
    Ok((
        xar,
        Package {
            components,
            product,
            requires_delegate,
        },
    ))
}
