use serde_json::Value;

use super::{Category, Disposition, SyncSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receive {
    /// Resolve referenced game/account/release identities before applying.
    MapIdentities,
    /// `overrides_json` in the portable payload contains user values. Apply
    /// them through game edits, preserving this device's automatic layer:
    /// the SQL column itself stores automatic values under manual field keys.
    GameEdits,
    /// Rebind executable locations on the receiving device.
    BindLaunchLocally,
    /// Game scripts always arrive disabled; global/profile scripts stay local.
    DisableGameScript,
    /// Resolve a definition hint only when this device has that emulator.
    ExistingEmulatorHint,
    /// A new server needs its own keychain secret. Removal also removes that
    /// server's downloaded ROMs, through the existing RomM removal operation.
    RommServer,
    /// Drop unknown game references; map merged games and never resurrect an
    /// approval deleted by a merge. One-time approvals are never persisted.
    AccountApproval,
}

#[derive(Debug, Clone, Copy)]
pub enum Condition {
    Any,
    Equals {
        pointer: &'static str,
        value: &'static str,
    },
}

#[derive(Debug)]
pub struct Rule {
    pub name: &'static str,
    pub category: Option<Category>,
    pub reason: &'static str,
    /// Dot-separated leaf paths; `*` matches exactly one array/map segment.
    /// SQL JSON columns must be decoded before field lookup (project does it).
    pub fields: &'static [&'static str],
    pub condition: Condition,
    pub receive: Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub disposition: Disposition,
    pub category: Option<Category>,
    pub reason: &'static str,
}

impl Rule {
    pub fn classification(&self) -> Classification {
        Classification {
            disposition: self
                .category
                .map_or(Disposition::Local, Category::disposition),
            category: self.category,
            reason: self.reason,
        }
    }

    /// An undeclared field is local, even in a table containing shared data.
    pub fn field(&self, path: &str) -> Classification {
        self.field_segments(&path.split('.').collect::<Vec<_>>())
    }

    /// Use separate segments for dynamic map keys containing dots, such as
    /// an add-on id. Their punctuation must not create extra nesting.
    pub fn field_segments(&self, path: &[&str]) -> Classification {
        if self.fields.iter().any(|pattern| matches(pattern, path)) {
            return self.classification();
        }
        Classification {
            disposition: Disposition::Local,
            category: None,
            reason: "undeclared fields, locations, automatic values and secrets stay on this device",
        }
    }

    /// A portable JSON projection of stored SQL/setting data for the later
    /// exchange to serialize. This doesn't send, insert, delete or apply
    /// anything. The exchange must check SyncSettings::allows before sending
    /// AND applying, then honor the receive contract for the payload.
    pub fn project(&self, row: &Value, settings: &SyncSettings) -> Option<Value> {
        let category = self.category?;
        if !settings.allows(category) {
            return None;
        }
        let mut row = decode(row);
        if self.receive == Receive::GameEdits {
            manual_game_values(&mut row);
        }
        if let Condition::Equals { pointer, value } = self.condition
            && row.pointer(pointer).and_then(Value::as_str) != Some(value)
        {
            return None;
        }
        let mut projected = project(self, &row, &[])?;
        if self.receive == Receive::DisableGameScript {
            projected
                .get_mut("record_json")?
                .as_object_mut()?
                .insert("enabled".into(), false.into());
        }
        Some(projected)
    }
}

fn matches(pattern: &str, path: &[&str]) -> bool {
    let pattern: Vec<_> = pattern.split('.').collect();
    pattern.len() == path.len()
        && pattern
            .iter()
            .zip(path)
            .all(|(expected, actual)| *expected == "*" || expected == actual)
}

fn decode(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let value = if key.ends_with("_json") {
                        value
                            .as_str()
                            .and_then(|value| serde_json::from_str(value).ok())
                            .unwrap_or_else(|| value.clone())
                    } else {
                        decode(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn manual_game_values(row: &mut Value) {
    let Some(current) = row.get("record_json").and_then(Value::as_object) else {
        if let Some(row) = row.as_object_mut() {
            row.remove("overrides_json");
        }
        return;
    };
    let mut manual = serde_json::Map::new();
    if let Some(markers) = row.get("overrides_json").and_then(Value::as_object) {
        for key in markers.keys() {
            if let Some(value) = current.get(key) {
                manual.insert(key.clone(), value.clone());
            }
        }
    }
    // Rating and notes have no automatic layer, hence no marker in SQL.
    for field in crate::model::GameField::ALL
        .into_iter()
        .filter(|field| !field.automatic())
    {
        if let Some(value) = current.get(field.key()) {
            manual.insert(field.key().into(), value.clone());
        }
    }
    row["overrides_json"] = Value::Object(manual);
}

fn project(rule: &Rule, value: &Value, path: &[&str]) -> Option<Value> {
    match value {
        Value::Object(object) => {
            let object: serde_json::Map<_, _> = object
                .iter()
                .filter_map(|(key, value)| {
                    let mut child = path.to_vec();
                    child.push(key);
                    Some((key.clone(), project(rule, value, &child)?))
                })
                .collect();
            if object.is_empty() && !has_descendants(rule, path) {
                None
            } else {
                Some(Value::Object(object))
            }
        }
        Value::Array(array) => {
            if !has_descendants(rule, path) {
                return None;
            }
            Some(Value::Array(
                array
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| {
                        let index = index.to_string();
                        let mut child = path.to_vec();
                        child.push(&index);
                        project(rule, value, &child)
                    })
                    .collect(),
            ))
        }
        _ if rule.field_segments(path).disposition != Disposition::Local => {
            if path.last() == Some(&"url") {
                let mut url = url::Url::parse(value.as_str()?).ok()?;
                // An endpoint is shared; inline userinfo/query tokens aren't.
                url.set_username("").ok()?;
                url.set_password(None).ok()?;
                url.set_query(None);
                url.set_fragment(None);
                Some(url.to_string().into())
            } else {
                Some(value.clone())
            }
        }
        _ => None,
    }
}

fn has_descendants(rule: &Rule, path: &[&str]) -> bool {
    rule.fields.iter().any(|pattern| {
        let pattern: Vec<_> = pattern.split('.').collect();
        pattern.len() > path.len()
            && pattern
                .iter()
                .zip(path)
                .all(|(expected, actual)| *expected == "*" || expected == actual)
    })
}
