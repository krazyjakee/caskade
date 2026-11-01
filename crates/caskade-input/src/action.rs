use serde::{Deserialize, Serialize};
use specta::Type;

/// What the user asked for, whatever the device. The dotted names are the
/// contract the frontend and plugins bind to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
pub enum Action {
    #[serde(rename = "nav.up")]
    NavUp,
    #[serde(rename = "nav.down")]
    NavDown,
    #[serde(rename = "nav.left")]
    NavLeft,
    #[serde(rename = "nav.right")]
    NavRight,
    /// A (Cross).
    #[serde(rename = "confirm")]
    Confirm,
    /// B (Circle).
    #[serde(rename = "back")]
    Back,
    /// X (Square).
    #[serde(rename = "details")]
    Details,
    /// Y (Triangle).
    #[serde(rename = "search")]
    Search,
    /// LB.
    #[serde(rename = "tab.prev")]
    TabPrev,
    /// RB.
    #[serde(rename = "tab.next")]
    TabNext,
    /// LT.
    #[serde(rename = "page.prev")]
    PagePrev,
    /// RT.
    #[serde(rename = "page.next")]
    PageNext,
    /// Start.
    #[serde(rename = "menu")]
    Menu,
    /// Select (Back/View/Share).
    #[serde(rename = "view")]
    View,
    /// The pad's Home/Guide/PS button.
    #[serde(rename = "guide")]
    Guide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Pad,
    Keyboard,
}

/// Which button labels to show in hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GlyphFamily {
    Xbox,
    Playstation,
    Nintendo,
    #[default]
    Generic,
}

impl GlyphFamily {
    /// From the USB vendor id, then the name.
    pub fn detect(vendor: Option<u16>, name: &str) -> Self {
        match vendor {
            Some(0x054c) => return Self::Playstation,
            Some(0x057e) => return Self::Nintendo,
            // Microsoft, and Valve (the Steam Deck and Steam Controller
            // have an Xbox layout).
            Some(0x045e | 0x28de) => return Self::Xbox,
            _ => {}
        }
        let name = name.to_lowercase();
        if ["playstation", "dualsense", "dualshock", "ps4", "ps5"]
            .iter()
            .any(|word| name.contains(word))
        {
            Self::Playstation
        } else if ["nintendo", "switch", "joy-con", "pro controller"]
            .iter()
            .any(|word| name.contains(word))
        {
            Self::Nintendo
        } else if ["xbox", "x-box", "xinput", "steam deck"]
            .iter()
            .any(|word| name.contains(word))
        {
            Self::Xbox
        } else {
            Self::Generic
        }
    }
}

/// One action as the UI receives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InputAction {
    pub action: Action,
    pub source: Source,
    /// A held direction firing again.
    pub repeat: bool,
    /// The labels of the pad that sent it.
    pub glyphs: GlyphFamily,
}

/// A connected pad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PadInfo {
    pub id: u32,
    pub name: String,
    pub glyphs: GlyphFamily,
    /// The pad used last.
    pub active: bool,
}
