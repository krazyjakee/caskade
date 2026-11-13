//! The game menu's Your devices: playing, copying and sharing a game on
//! other devices.

/// The game menu's Your devices → Share with someone…: for a release
/// installed here with its files here, while the app shares; not for a
/// store's copy, and not on one entry's row.
#[test]
fn share_with_someone_is_in_the_menu_for_a_copy_here() {
    use crate::game_menu::entries::EntryView;
    use crate::game_menu::menu::{Facts, Target, items};
    use crate::ui::MenuEntry;
    let entry = |source: &str| EntryView {
        id: "e1".into(),
        source: source.into(),
        installed: true,
        location: Some("/games/Celeste".into()),
        can_launch: true,
        emulated: false,
        stream: false,
    };
    let target = |entry_id: Option<&str>| Target {
        game_id: "g1".into(),
        title: "Celeste".into(),
        entry_id: entry_id.map(str::to_owned),
    };
    let share = |target: &Target, facts: &Facts| {
        items(target, facts)
            .iter()
            .find_map(|section| match section {
                MenuEntry::Section { title, items } => items
                    .iter()
                    .find(|item| item.id == "share")
                    .map(|item| (title.clone(), item.label.clone())),
                _ => None,
            })
    };
    let facts = |source: &str, shares: bool| Facts {
        entries: vec![entry(source)],
        shares,
        ..Facts::default()
    };
    assert_eq!(
        share(&target(None), &facts("manual", true)),
        Some((
            Some("Your devices".to_owned()),
            "Share with someone…".to_owned()
        ))
    );
    assert_eq!(share(&target(None), &facts("store:steam", true)), None);
    assert_eq!(share(&target(None), &facts("manual", false)), None);
    assert_eq!(share(&target(Some("e1")), &facts("manual", true)), None);
}

/// Your devices, as GameMenu.tsx lists it: Play on each device that has
/// it (waking one out of reach that can be woken; not one playing it
/// now), Copy to each paired device reached directly that hasn't it, then
/// Share, then the copies there with what to know about each.
#[test]
fn your_devices_plays_there_copies_there_and_lists_the_copies() {
    use crate::game_menu::entries::EntryView;
    use crate::game_menu::menu::{Facts, Target, items};
    use crate::ui::MenuEntry;
    use caskade_core::db::CopyLocation;
    use caskade_ui_data::devices::{CopyBlocked, DeviceLink, DeviceOs, PairedDevice, PeerCopy};
    let peer = |id: &str, name: &str, link: DeviceLink| PeerCopy {
        device_id: id.into(),
        device_name: name.into(),
        kind: Default::default(),
        os: DeviceOs::Linux,
        link,
        size_bytes: None,
        location: CopyLocation::Managed,
        blocked: None,
        through_proton: false,
        playing: false,
        wakeable: false,
    };
    let paired = |id: &str, name: &str, link: DeviceLink| PairedDevice {
        id: id.into(),
        name: name.into(),
        kind: Default::default(),
        os: DeviceOs::Linux,
        link,
        addresses: Vec::new(),
        last_sync_ms: None,
        paired_at_ms: 0,
        syncing: false,
        sync_error: None,
    };
    let deck = peer("deck", "Deck", DeviceLink::Lan);
    let tv = PeerCopy {
        wakeable: true,
        blocked: Some(CopyBlocked::OtherSystem),
        os: DeviceOs::Windows,
        ..peer("tv", "TV", DeviceLink::Offline)
    };
    let laptop = PeerCopy {
        playing: true,
        ..peer("laptop", "Laptop", DeviceLink::Direct)
    };
    let away = peer("away", "Away", DeviceLink::Offline);
    let facts = Facts {
        entries: vec![EntryView {
            id: "e1".into(),
            source: "manual".into(),
            installed: true,
            location: Some("/games/Celeste".into()),
            can_launch: true,
            emulated: false,
            stream: false,
        }],
        shares: true,
        peers: vec![deck, tv, laptop, away],
        paired: vec![
            paired("deck", "Deck", DeviceLink::Lan),
            paired("desk", "Desk", DeviceLink::Direct),
            paired("far", "Far", DeviceLink::Relay),
        ],
        ..Facts::default()
    };
    let target = Target {
        game_id: "g1".into(),
        title: "Celeste".into(),
        entry_id: None,
    };
    let section = items(&target, &facts)
        .into_iter()
        .find_map(|entry| match entry {
            MenuEntry::Section { title, items } if title.as_deref() == Some("Your devices") => {
                Some(items)
            }
            _ => None,
        })
        .expect("Your devices");
    let shown: Vec<(String, String, Option<String>)> = section
        .into_iter()
        .map(|item| (item.id, item.label, item.description))
        .collect();
    assert_eq!(
        shown,
        [
            ("play-on:deck".into(), "Play on Deck".into(), None),
            (
                "play-on:tv".into(),
                "Play on TV".into(),
                Some("Wakes it first".into())
            ),
            (
                "send:desk".into(),
                "Copy to Desk".into(),
                Some("It asks there before copying".into())
            ),
            (
                "share".into(),
                "Share with someone…".into(),
                Some("A link for someone else's Caskade".into())
            ),
            ("device:deck".into(), "On Deck".into(), None),
            (
                "device:tv".into(),
                "On TV".into(),
                Some("TV's copy is for Windows and can't run here.".into())
            ),
            (
                "device:laptop".into(),
                "On Laptop".into(),
                Some("Playing on Laptop now.".into())
            ),
            (
                "device:away".into(),
                "On Away".into(),
                Some("Away is out of reach.".into())
            ),
        ]
    );
}
