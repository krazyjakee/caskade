//! What a device says about itself: its name, kind and OS.
//!
//! Announced on the local network so nearby devices can list it before
//! pairing, and exchanged while pairing. Never published to n0's DNS: only
//! the mDNS announcement carries it (see [`crate::lan`]).

use iroh::address_lookup::UserData;
use serde::{Deserialize, Serialize};

/// The kind of device, as the user chose it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    #[default]
    Desktop,
    Laptop,
    Handheld,
    Tv,
}

/// The device's operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceOs {
    Linux,
    Windows,
    Mac,
}

impl DeviceOs {
    /// The OS this build runs on.
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }
}

/// A device's name, kind and OS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub kind: DeviceKind,
    pub os: DeviceOs,
}

/// The longest name a device announces or accepts, in characters.
pub const MAX_NAME_CHARS: usize = 64;

/// The announcement's wire form: short keys and a version, to fit mDNS's
/// 245-byte user data.
#[derive(Serialize, Deserialize)]
struct Announcement<'a> {
    v: u8,
    n: std::borrow::Cow<'a, str>,
    k: DeviceKind,
    o: DeviceOs,
}

impl DeviceInfo {
    /// This device, named `name` (trimmed and cut to [`MAX_NAME_CHARS`]).
    pub fn new(name: &str, kind: DeviceKind) -> Self {
        Self {
            name: clean_name(name),
            kind,
            os: DeviceOs::current(),
        }
    }

    /// The mDNS user data announcing this device. The name is shortened
    /// further if its JSON escaping would overflow the limit.
    pub(crate) fn to_user_data(&self) -> UserData {
        let mut name: String = clean_name(&self.name);
        loop {
            let text = serde_json::to_string(&Announcement {
                v: 1,
                n: name.as_str().into(),
                k: self.kind,
                o: self.os,
            })
            .unwrap_or_default();
            if let Ok(data) = UserData::try_from(text) {
                return data;
            }
            name.pop();
        }
    }

    /// Reads a peer's announcement; `None` for anything that isn't one.
    pub(crate) fn from_user_data(data: &UserData) -> Option<Self> {
        let announcement: Announcement<'_> = serde_json::from_str(data.as_ref()).ok()?;
        (announcement.v == 1).then(|| Self {
            name: clean_name(&announcement.n),
            kind: announcement.k,
            os: announcement.o,
        })
    }
}

/// Trims a name, drops control characters and cuts it to
/// [`MAX_NAME_CHARS`]. Names come from peers, so this runs on every one read.
pub fn clean_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_announcement_round_trips() {
        let info = DeviceInfo {
            name: "Living room".into(),
            kind: DeviceKind::Tv,
            os: DeviceOs::Linux,
        };
        let data = info.to_user_data();
        assert_eq!(DeviceInfo::from_user_data(&data), Some(info));
    }

    #[test]
    fn a_long_or_escaped_name_still_fits() {
        let info = DeviceInfo {
            name: "\"".repeat(200),
            kind: DeviceKind::Desktop,
            os: DeviceOs::Windows,
        };
        let data = info.to_user_data();
        assert!(data.as_ref().len() <= UserData::MAX_LENGTH);
        let read = DeviceInfo::from_user_data(&data).unwrap();
        assert!(read.name.chars().all(|c| c == '"'));
        assert!(!read.name.is_empty());
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Den\u{7}  PC \n"), "Den  PC");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn other_user_data_is_ignored() {
        let data = UserData::try_from("hello".to_owned()).unwrap();
        assert_eq!(DeviceInfo::from_user_data(&data), None);
        let data = UserData::try_from(r#"{"v":2,"n":"a","k":"tv","o":"mac"}"#.to_owned()).unwrap();
        assert_eq!(DeviceInfo::from_user_data(&data), None);
    }
}
