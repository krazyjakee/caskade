//! Explicit one-step migrations. v0 is the pre-release fixture contract, not a
//! previously published format. Keep a fixture for each supported version.
use super::{INSTALL_SCHEMA_VERSION, ManifestError, ROOT_SCHEMA_VERSION, Result};
use serde_json::{Map, Value};

fn document<'a>(
    value: &'a mut Value,
    name: &'static str,
    supported: u32,
) -> Result<(&'a mut Map<String, Value>, u64)> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| ManifestError::Invalid(format!("{name} must be a JSON object")))?;
    let version = object
        .remove("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            ManifestError::Invalid(format!("{name} requires an unsigned schema_version"))
        })?;
    if version > u64::from(supported) {
        return Err(ManifestError::UnsupportedVersion {
            document: name,
            version,
            supported,
        });
    }
    Ok((object, version))
}

pub(super) fn install(mut value: Value) -> Result<Value> {
    let (object, mut version) = document(&mut value, "install manifest", INSTALL_SCHEMA_VERSION)?;
    while version < u64::from(INSTALL_SCHEMA_VERSION) {
        match version {
            0 => {
                // v0 used a handler-id -> version map and `hash` for SHA-256;
                // it predates the explicit Unscanned relocation placeholder.
                let handlers = object
                    .remove("handlers")
                    .and_then(|v| v.as_object().cloned())
                    .ok_or_else(|| {
                        ManifestError::Invalid("v0 handlers must be an object".into())
                    })?;
                object.insert(
                    "handlers".into(),
                    Value::Array(
                        handlers
                            .into_iter()
                            .map(|(id, version)| serde_json::json!({"id": id, "version": version}))
                            .collect(),
                    ),
                );
                let files = object
                    .get_mut("files")
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| ManifestError::Invalid("v0 files must be an array".into()))?;
                for file in files {
                    let file = file.as_object_mut().ok_or_else(|| {
                        ManifestError::Invalid("v0 file must be an object".into())
                    })?;
                    if let Some(hash) = file.remove("hash")
                        && file.insert("sha256".into(), hash).is_some()
                    {
                        return Err(ManifestError::Invalid(
                            "v0 file has both hash and sha256".into(),
                        ));
                    }
                }
                if object
                    .insert(
                        "relocation".into(),
                        serde_json::json!({"verdict": "unscanned"}),
                    )
                    .is_some()
                {
                    return Err(ManifestError::Invalid(
                        "v0 cannot already contain relocation data".into(),
                    ));
                }
                version = 1;
            }
            _ => {
                return Err(ManifestError::UnsupportedVersion {
                    document: "install manifest",
                    version,
                    supported: INSTALL_SCHEMA_VERSION,
                });
            }
        }
    }
    Ok(value)
}

pub(super) fn root(mut value: Value) -> Result<Value> {
    let (object, mut version) = document(&mut value, "root manifest", ROOT_SCHEMA_VERSION)?;
    while version < u64::from(ROOT_SCHEMA_VERSION) {
        match version {
            0 => {
                let id = object
                    .remove("id")
                    .ok_or_else(|| ManifestError::Invalid("v0 root has no id".into()))?;
                if object.insert("root_id".into(), id).is_some() {
                    return Err(ManifestError::Invalid(
                        "v0 root has both id and root_id".into(),
                    ));
                }
                version = 1;
            }
            _ => {
                return Err(ManifestError::UnsupportedVersion {
                    document: "root manifest",
                    version,
                    supported: ROOT_SCHEMA_VERSION,
                });
            }
        }
    }
    Ok(value)
}
