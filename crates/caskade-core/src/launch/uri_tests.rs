#![cfg(target_os = "linux")]
//! Native Linux URI dispatcher proof in private XDG directories. This uses a
//! fixture handler for the steam scheme, not the user's Steam account/library.
use super::{test_support::*, *};
use crate::model::LaunchSpec;
use async_trait::async_trait;
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};
struct PrivateXdg {
    env: BTreeMap<String, String>,
}
#[async_trait]
impl LaunchHooks for PrivateXdg {
    async fn before(&self, _: &LaunchTicket, command: &mut LaunchCommand) -> Result<bool> {
        command.env.extend(self.env.clone());
        Ok(true)
    }
}
#[tokio::test]
async fn steam_uri_uses_native_xdg_dispatch_and_records_fixture_child_playtime() {
    let temp = tempfile::tempdir().unwrap();
    let game = temp.path().join("game");
    let apps = temp.path().join("data/applications");
    let config = temp.path().join("config");
    fs::create_dir_all(&game).unwrap();
    fs::create_dir_all(&apps).unwrap();
    fs::create_dir_all(&config).unwrap();
    let exe = game.join("fixture-game");
    crate::test_programs::copy_program("/bin/sleep", &exe);
    let token = temp.path().join("uri-token");
    let handler = temp.path().join("steam-handler");
    // Paths are authored by tempfile and passed as argv: the handler contains
    // no user input and never edits global MIME associations.
    crate::test_programs::write_program(
        &handler,
        r#"#!/bin/sh
printf '%s' "$1" > "$2"
exec "$3" 1.0
"#,
    );
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Caskade URI Fixture\nExec={} %u {} {}\nTerminal=false\nMimeType=x-scheme-handler/steam;\n",
        handler.display(),
        token.display(),
        exe.display()
    );
    fs::write(apps.join("caskade-steam-fixture.desktop"), desktop).unwrap();
    let mime = "[Default Applications]\nx-scheme-handler/steam=caskade-steam-fixture.desktop;\n";
    fs::write(config.join("mimeapps.list"), mime).unwrap();
    fs::write(apps.join("mimeapps.list"), mime).unwrap();
    let mut entry = entry(PathBuf::from("unused"), vec![], Some(game));
    entry.launch = Some(LaunchSpec::Uri {
        uri: "steam://rungameid/424242".into(),
    });
    entry.tracking = Some(caskade_discovery::TrackingHint {
        steam_app_id: Some(424242),
        ..Default::default()
    });
    let store = Arc::new(MemoryStore::default());
    store
        .entries
        .lock()
        .unwrap()
        .insert(entry.id.clone(), entry.clone());
    let env = BTreeMap::from([
        (
            "XDG_CONFIG_HOME".into(),
            config.to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".into(),
            temp.path().join("data").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_DIRS".into(),
            temp.path().join("data").to_string_lossy().into_owned(),
        ),
        ("XDG_CURRENT_DESKTOP".into(), "X-Generic".into()),
        ("DE".into(), "generic".into()),
        // xdg-open's generic dispatch wants a graphical session, but this
        // one must not be the developer's (#122): a display, Wayland
        // socket and session bus that don't exist, so a fallback (a
        // browser) can't open anywhere.
        ("DISPLAY".into(), ":caskade-test-none".into()),
        ("WAYLAND_DISPLAY".into(), "caskade-test-none".into()),
        (
            "DBUS_SESSION_BUS_ADDRESS".into(),
            "unix:path=/nonexistent/caskade-test-bus".into(),
        ),
        ("HOME".into(), temp.path().to_string_lossy().into_owned()),
    ]);
    let mut config = fast_config();
    config.launch_timeout = std::time::Duration::from_secs(5);
    let service = LaunchService::from_repository(store)
        .with_config(config)
        .with_aliases(None)
        .with_hooks(Arc::new(PrivateXdg { env }));
    let session = service
        .launch(&entry.id)
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(token).unwrap(),
        "steam://rungameid/424242"
    );
    assert!(session.duration_ms >= 200, "{}", session.duration_ms);
}
#[test]
fn only_game_launcher_schemes_reach_the_uri_dispatcher() {
    // CSK-09: OS protocol handlers that open remote content or run programs.
    for bad in [
        r"search-ms:query=x&crumb=location:\\evil\share",
        "ms-msdt:/id PCWDiagnostic /skip force",
        r"ms-word:ofe|u|\\host\share\doc.docx",
        "ms-officecmd:{}",
        "shell:startup",
        r"shell:AppsFolder\..\..\evil!App",
        r"shell:AppsFolder\Family!App\..\x",
        "smb://host/share",
        "mailto:x@example.org",
    ] {
        assert!(uri_command(bad).is_err(), "{bad}");
    }
    for good in [
        "steam://rungameid/620",
        "STEAM://rungameid/620",
        "com.epicgames.launcher://apps/ns%3Aitem%3Aapp?action=launch&silent=true",
        "heroic://launch/legendary/Fixture",
        "lutris:rungameid/12",
        "battlenet://WTCG",
        "uplay://launch/635/0",
        "goggalaxy://openGameView/1207658924",
        "itch://caves/3/launch",
        "amazon-games://play/amzn1.adg.product.x",
        "origin2://game/launch?offerIds=OFB-EAST:1",
        "link2ea://launchgame/x",
        "flatpak:org.example.Game",
        r"shell:AppsFolder\Microsoft.MinecraftUWP_8wekyb3d8bbwe!App",
    ] {
        assert!(uri_command(good).is_ok(), "{good}");
    }
}
