use super::{DumpError, Result, scrub::Scrubber};
use crate::parsers::{
    Limits,
    binary_vdf::{self, Value},
    lnk, protobuf,
};
use prost::Message;
use std::collections::{BTreeMap, BTreeSet};

fn string(out: &mut Vec<u8>, value: &str) {
    out.extend(value.as_bytes());
    out.push(0);
}
fn scrub_value(value: &mut Value, key: &str, scrub: &Scrubber) {
    if super::scrub::secret(key) {
        *value = Value::String("<redacted>".into());
        return;
    }
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                *key = scrub.text(key);
                scrub_value(value, key, scrub);
            }
        }
        Value::String(text) | Value::WideString(text) => *text = scrub.field(key, text),
        Value::Uint64(number)
            if *number >= 76_561_190_000_000_000 && *number < 76_561_200_000_000_000 =>
        {
            *number = super::scrub::ACCOUNT.parse().expect("constant")
        }
        _ => {}
    }
}
pub(super) fn text_vdf(bytes: &[u8], scrub: &Scrubber) -> Result<Vec<u8>> {
    let mut value = crate::parsers::text_vdf::parse(bytes, &Limits::default())
        .map_err(|e| DumpError::Invalid(e.to_string()))?
        .value;
    scrub_value(&mut value, "", scrub);
    fn quoted(text: &str) -> String {
        format!(
            "\"{}\"",
            text.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
        )
    }
    fn write(value: &Value, out: &mut String) -> Result<()> {
        let Value::Object(entries) = value else {
            return Err(DumpError::Invalid("text VDF root is not an object".into()));
        };
        for (key, value) in entries {
            out.push_str(&quoted(key));
            match value {
                Value::Object(_) => {
                    out.push_str("\n{\n");
                    write(value, out)?;
                    out.push_str("}\n");
                }
                Value::String(text) => {
                    out.push(' ');
                    out.push_str(&quoted(text));
                    out.push('\n');
                }
                _ => return Err(DumpError::Invalid("non-text value in text VDF".into())),
            }
        }
        Ok(())
    }
    let mut out = String::new();
    write(&value, &mut out)?;
    Ok(out.into_bytes())
}
fn encode(value: &Value, table: Option<&BTreeMap<String, u32>>) -> Result<Vec<u8>> {
    let Value::Object(entries) = value else {
        return Err(DumpError::Invalid("VDF root is not an object".into()));
    };
    let mut out = vec![];
    for (key, value) in entries {
        let kind = match value {
            Value::Object(_) => 0,
            Value::String(_) => 1,
            Value::Int(_) => 2,
            Value::Float(_) => 3,
            Value::Pointer(_) => 4,
            Value::WideString(_) => 5,
            Value::Color(_) => 6,
            Value::Uint64(_) => 7,
            Value::Int64(_) => 10,
        };
        out.push(kind);
        if let Some(table) = table {
            out.extend(table[key].to_le_bytes());
        } else {
            string(&mut out, key);
        }
        match value {
            Value::Object(_) => out.extend(encode(value, table)?),
            Value::String(text) => string(&mut out, text),
            Value::Int(n) => out.extend(n.to_le_bytes()),
            Value::Float(n) => out.extend(n.to_le_bytes()),
            Value::Pointer(n) => out.extend(n.to_le_bytes()),
            Value::WideString(text) => {
                let words: Vec<_> = text.encode_utf16().collect();
                out.extend(
                    u16::try_from(words.len())
                        .map_err(|_| DumpError::Limit("wide string units"))?
                        .to_le_bytes(),
                );
                for word in words {
                    out.extend(word.to_le_bytes());
                }
            }
            Value::Color(bytes) => out.extend(bytes),
            Value::Uint64(n) => out.extend(n.to_le_bytes()),
            Value::Int64(n) => out.extend(n.to_le_bytes()),
        }
    }
    out.push(8);
    Ok(out)
}
pub(super) fn vdf(bytes: &[u8], scrub: &Scrubber) -> Result<Vec<u8>> {
    let mut data = binary_vdf::parse(bytes, &Limits::default())
        .map_err(|e| DumpError::Invalid(e.to_string()))?;
    scrub_value(&mut data, "", scrub);
    encode(&data, None)
}
pub(super) fn appinfo(
    bytes: &[u8],
    scrub: &Scrubber,
    installed: &BTreeSet<u32>,
) -> Result<Vec<u8>> {
    use sha1::{Digest, Sha1};
    let mut appinfo = binary_vdf::parse_appinfo_selected(bytes, &Limits::default(), installed)
        .map_err(|e| DumpError::Invalid(e.to_string()))?;
    let mut keys = BTreeSet::new();
    fn collect(value: &Value, keys: &mut BTreeSet<String>) {
        if let Value::Object(entries) = value {
            for (key, value) in entries {
                keys.insert(key.clone());
                collect(value, keys);
            }
        }
    }
    for app in &mut appinfo.apps {
        scrub_value(&mut app.data, "", scrub);
        collect(&app.data, &mut keys);
    }
    let table: BTreeMap<_, _> = keys
        .into_iter()
        .enumerate()
        .map(|(index, key)| (key, index as u32))
        .collect();
    let mut entries = vec![];
    for app in appinfo.apps {
        let data = encode(&app.data, (appinfo.version == 0x29).then_some(&table))?;
        entries.extend(app.app_id.to_le_bytes());
        entries.extend((60 + data.len() as u32).to_le_bytes());
        entries.extend(app.info_state.to_le_bytes());
        entries.extend(app.last_updated.to_le_bytes());
        entries.extend(0u64.to_le_bytes());
        let digest = Sha1::digest(&data);
        entries.extend(digest);
        entries.extend(app.change_number.to_le_bytes());
        entries.extend(digest);
        entries.extend(data);
    }
    entries.extend(0u32.to_le_bytes());
    let mut out = vec![];
    out.extend((0x07564400 | u32::from(appinfo.version)).to_le_bytes());
    out.extend(appinfo.universe.to_le_bytes());
    if appinfo.version == 0x29 {
        out.extend((16 + entries.len() as u64).to_le_bytes());
    }
    out.extend(entries);
    if appinfo.version == 0x29 {
        out.extend((table.len() as u32).to_le_bytes());
        // BTreeMap key order is the same order used to allocate indices.
        for name in table.keys() {
            string(&mut out, name);
        }
    }
    Ok(out)
}
pub(super) fn product(bytes: &[u8], scrub: &Scrubber) -> Result<Vec<u8>> {
    let mut db = protobuf::parse_product_db(bytes, &Limits::default())
        .map_err(|e| DumpError::Invalid(e.to_string()))?;
    for product in &mut db.products {
        if let Some(settings) = &mut product.settings {
            settings.install_path = scrub.text(&settings.install_path);
        }
    }
    // Re-encoding drops unknown fields that might hold account-specific data.
    Ok(db.encode_to_vec())
}
pub(super) fn link(bytes: &[u8], scrub: &Scrubber) -> Result<Vec<u8>> {
    let link = lnk::parse(bytes, &Limits::default())
        .map_err(|e| DumpError::Invalid(e.to_string()))?
        .value;
    // Build a fresh Unicode StringData link; opaque IDList/ExtraData and host
    // volume serials are never copied into a portable anonymized fixture.
    let fields = [
        (4, link.description),
        (8, link.target.or(link.relative_path)),
        (16, link.working_dir),
        (32, link.raw_arguments),
        (64, link.icon),
    ];
    let flags = 128
        | fields
            .iter()
            .filter(|(_, v)| v.is_some())
            .map(|(flag, _)| flag)
            .sum::<u32>();
    let mut out = vec![0; 76];
    out[..4].copy_from_slice(&76u32.to_le_bytes());
    out[4..20].copy_from_slice(&[1, 20, 2, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70]);
    out[20..24].copy_from_slice(&flags.to_le_bytes());
    out[60..64].copy_from_slice(&1u32.to_le_bytes());
    for (_, text) in fields {
        if let Some(text) = text {
            let text = scrub.text(&text);
            let words: Vec<_> = text.encode_utf16().collect();
            out.extend(
                u16::try_from(words.len())
                    .map_err(|_| DumpError::Limit("link string units"))?
                    .to_le_bytes(),
            );
            for word in words {
                out.extend(word.to_le_bytes());
            }
        }
    }
    out.extend(0u32.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structural_vdf_scrub_changes_lengths_and_removes_nested_credentials() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("root")).unwrap();
        std::fs::write(
            temp.path().join("machine.toml"),
            "schema_version=1\nos=\"linux\"\n[env]\nHOME=\"/home/private-test-long-name\"\n",
        )
        .unwrap();
        let env = crate::HostEnv::fixture(temp.path()).unwrap();
        let scrub = Scrubber::new(&env);
        let value = Value::Object(vec![
            (
                "Exe".into(),
                Value::String("/home/private-test-long-name/Games/Café/game".into()),
            ),
            ("SteamID".into(), Value::Uint64(76_561_198_123_456_789)),
            (
                "AuthToken".into(),
                Value::Object(vec![(
                    "opaque".into(),
                    Value::String("credential-sentinel".into()),
                )]),
            ),
        ]);
        let input = encode(&value, None).unwrap();
        let output = vdf(&input, &scrub).unwrap();
        assert_ne!(input.len(), output.len());
        let parsed = binary_vdf::parse(&output, &Limits::default()).unwrap();
        let Value::Object(fields) = parsed else {
            panic!("object expected")
        };
        assert_eq!(fields[0].1.as_str(), Some("/home/fixture/Games/Café/game"));
        assert_eq!(
            fields[1].1,
            Value::Uint64(super::super::scrub::ACCOUNT.parse().unwrap())
        );
        assert_eq!(fields[2].1.as_str(), Some("<redacted>"));
        assert!(!String::from_utf8_lossy(&output).contains("credential-sentinel"));
        assert_eq!(
            binary_vdf::parse(&input, &Limits::default()).unwrap(),
            value
        );
        let text = br#""root" { "AuthToken" { "nested" "credential-sentinel" } "Exe" "/home/private-test-long-name/game" }"#;
        let output = text_vdf(text, &scrub).unwrap();
        crate::parsers::text_vdf::parse(&output, &Limits::default()).unwrap();
        assert!(!String::from_utf8_lossy(&output).contains("credential-sentinel"));
    }
}
