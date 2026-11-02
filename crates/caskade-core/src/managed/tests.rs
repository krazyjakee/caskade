//! A helper tool installed, found, checked, updated, skipped and removed
//! against a fake GitHub (wiremock): the `releases/latest` redirect, the
//! `expanded_assets` page and fixture archives.

use super::*;
use crate::{
    AppPaths,
    bus::{Bus, BusOptions},
    db::LibraryStore,
    emulation::{EmulatorInstall, OperatingSystem},
    notifications::{MemberKind, Notifications},
    updates::GitHub,
};
use std::{io::Write, path::Path};
use tokio_util::sync::CancellationToken;
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const VIEWER: &str = r#"
id = "moonlight"
name = "Viewer"
dir = "streaming/client"

[detect.linux]
executable = "^viewer$"

[[install.linux]]
when = ["wayland", "gamescope", "aarch64"]
source = "flatpak"
app = "org.example.Viewer"

[[install.linux]]
when = ["x86_64"]
source = "github"
repo = "example/viewer"
asset = '^Viewer-[\d.]+-x86_64\.zip$'
"#;

fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// Serves `tag` of example/viewer as the latest release: an arm64 build
/// first (which this machine must pass over), its own build, the source.
async fn release(server: &MockServer, tag: &str, version: &str) {
    server.reset().await;
    let file = format!("Viewer-{version}-x86_64.zip");
    Mock::given(method("HEAD"))
        .and(path("/example/viewer/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("/example/viewer/releases/tag/{tag}")),
        )
        .mount(server)
        .await;
    let assets = format!(
        r#"<ul><li><a href="/example/viewer/releases/download/{tag}/Viewer-{version}-arm64.zip">a</a></li>
        <li><a href="/example/viewer/releases/download/{tag}/{file}" rel="nofollow">x</a></li>
        <li><a href="/example/viewer/archive/refs/tags/{tag}.zip">source</a></li></ul>"#
    );
    Mock::given(method("GET"))
        .and(path(format!(
            "/example/viewer/releases/expanded_assets/{tag}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_string(assets))
        .mount(server)
        .await;
    let zip = zip_of(&[
        (&format!("viewer-{version}/viewer"), version),
        (&format!("viewer-{version}/resources/ui.dat"), version),
    ]);
    Mock::given(method("GET"))
        .and(path(format!(
            "/example/viewer/releases/download/{tag}/{file}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip))
        .mount(server)
        .await;
}

struct Fixture {
    temp: tempfile::TempDir,
    store: LibraryStore,
    tools: ManagedTools,
}

fn host(arch: Arch, wayland: bool) -> Host {
    Host {
        os: OperatingSystem::Linux,
        arch,
        wayland,
        gamescope: false,
    }
}

async fn fixture(server: &MockServer, host: Host) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = LibraryStore::in_memory().await.unwrap();
    let catalog = ToolCatalog::new(vec![ToolDefinition::parse(VIEWER).unwrap()]).unwrap();
    let hosts = Hosts {
        github: GitHub::new(Url::parse(&format!("{}/", server.uri())).unwrap()),
        dolphin: Url::parse(&format!("{}/dolphin/", server.uri())).unwrap(),
    };
    // A stand-in `flatpak` that logs its arguments; `info` of an app it
    // installed succeeds.
    let flatpak = temp.path().join("flatpak");
    let log = temp.path().join("flatpak.log");
    std::fs::write(
        &flatpak,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{log}'\ncase \"$1\" in\n  install) touch '{log}.installed' ;;\n  uninstall) rm -f '{log}.installed' ;;\n  info) test -e '{log}.installed' ;;\nesac\n",
            log = log.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&flatpak, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let tools = ManagedTools::new(
        store.clone(),
        Bus::new(BusOptions::default()),
        catalog,
        &paths,
    )
    .with_host(host, hosts, flatpak);
    Fixture { temp, store, tools }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

async fn install(tools: &ManagedTools) -> crate::Result<ManagedTool> {
    let mut seen = Vec::new();
    let installed = tools
        .install("moonlight", &CancellationToken::new(), |done, total| {
            seen.push((done, total))
        })
        .await;
    if installed.is_ok()
        && tools
            .source("moonlight")
            .is_some_and(|s| s.kind() == "github")
    {
        let (done, total) = seen.last().copied().unwrap();
        assert_eq!(Some(done), total, "progress reached the whole file");
    }
    installed
}

#[tokio::test]
async fn a_tool_installs_is_found_updates_with_the_users_files_and_is_removed() {
    let server = MockServer::start().await;
    release(&server, "v1.0", "1.0").await;
    let f = fixture(&server, host(Arch::X86_64, false)).await;
    let dir = f.temp.path().join("data/streaming/client");

    let managed = install(&f.tools).await.unwrap();
    assert_eq!(managed.installed.as_deref(), Some("v1.0"));
    assert_eq!(managed.file.as_deref(), Some("Viewer-1.0-x86_64.zip"));
    // Found by its detect rules, the release's lone top folder lifted out.
    let found = EmulatorInstall::Folder {
        dir: dir.clone(),
        executable: "viewer".into(),
    };
    assert_eq!(managed.install.as_ref(), Some(&found));
    assert_eq!(
        f.tools.find("moonlight").await.unwrap(),
        Some(found.clone())
    );
    assert_eq!(read(&dir.join("viewer")), "1.0");

    // The user's settings; a file the release shipped is the release's.
    std::fs::write(dir.join("viewer.ini"), "quality=high").unwrap();
    std::fs::create_dir_all(dir.join("state")).unwrap();
    std::fs::write(dir.join("state/hosts.json"), "[]").unwrap();
    std::fs::write(dir.join("resources/ui.dat"), "edited").unwrap();

    // Nothing newer yet; then a release appears and is offered.
    assert!(!f.tools.check("moonlight").await.unwrap().update_available());
    release(&server, "v2.0", "2.0").await;
    let checked = f.tools.check("moonlight").await.unwrap();
    assert_eq!(checked.latest.as_deref(), Some("v2.0"));
    assert!(checked.update_available());
    // Offered in "Updates available" under its own kind, in Caskade's words.
    let feed = Notifications::new(f.store.clone());
    feed.sync_updates().await.unwrap();
    let item = feed.list(false, 0, 10).await.unwrap().items.remove(0);
    let members = feed.members(&item.id, 0, 10).await.unwrap().items;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].kind, MemberKind::ToolUpdate);
    assert_eq!(members[0].label, "Game streaming to this device v2.0");
    assert!(!members[0].label.to_lowercase().contains("moonlight"));
    // Skipped, it isn't offered; a newer one is again.
    assert!(
        !f.tools
            .skip("moonlight", None)
            .await
            .unwrap()
            .update_available()
    );
    feed.sync_updates().await.unwrap();
    for item in feed.list(false, 0, 10).await.unwrap().items {
        let members = feed.members(&item.id, 0, 10).await.unwrap().items;
        assert!(members.iter().all(|m| m.kind != MemberKind::ToolUpdate));
    }
    release(&server, "v3.0", "3.0").await;
    assert!(f.tools.check("moonlight").await.unwrap().update_available());
    // Checking never installs anything.
    assert_eq!(read(&dir.join("viewer")), "1.0");

    let updated = f
        .tools
        .update("moonlight", &CancellationToken::new(), |_, _| {})
        .await
        .unwrap();
    assert_eq!(updated.installed.as_deref(), Some("v3.0"));
    assert!(!updated.update_available());
    assert_eq!(read(&dir.join("viewer")), "3.0");
    assert_eq!(read(&dir.join("viewer.ini")), "quality=high");
    assert_eq!(read(&dir.join("state/hosts.json")), "[]");
    assert_eq!(read(&dir.join("resources/ui.dat")), "3.0");
    assert_eq!(f.tools.find("moonlight").await.unwrap(), Some(found));
    assert!(!f.temp.path().join("data/.caskade-tools").exists());

    // Removed: the program goes, the user's files stay.
    f.tools.remove("moonlight").await.unwrap();
    assert!(f.tools.managed("moonlight").await.unwrap().is_none());
    assert_eq!(f.tools.find("moonlight").await.unwrap(), None);
    assert!(!dir.join("viewer").exists());
    assert!(!dir.join(".caskade/shipped.json").exists());
    assert!(!dir.join("resources").exists());
    assert_eq!(read(&dir.join("viewer.ini")), "quality=high");
    assert_eq!(read(&dir.join("state/hosts.json")), "[]");
    assert!(f.tools.remove("moonlight").await.is_err());
}

#[tokio::test]
async fn a_failed_update_keeps_the_installed_version() {
    let server = MockServer::start().await;
    release(&server, "v1.0", "1.0").await;
    let f = fixture(&server, host(Arch::X86_64, false)).await;
    install(&f.tools).await.unwrap();
    // v2.0's file is announced but not there.
    release(&server, "v2.0", "2.0").await;
    server.reset().await;
    Mock::given(method("HEAD"))
        .and(path("/example/viewer/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "/example/viewer/releases/tag/v2.0"),
        )
        .mount(&server)
        .await;
    assert!(
        f.tools
            .update("moonlight", &CancellationToken::new(), |_, _| {})
            .await
            .is_err()
    );
    let managed = f.tools.managed("moonlight").await.unwrap().unwrap();
    assert_eq!(managed.installed.as_deref(), Some("v1.0"));
    assert!(f.tools.find("moonlight").await.unwrap().is_some());
    // A failed check is kept as the reason, and the version found stays.
    let checked = f.tools.check("moonlight").await.unwrap();
    assert!(checked.error.is_some());
    assert_eq!(checked.latest.as_deref(), Some("v1.0"));
}

#[tokio::test]
async fn the_machine_picks_the_source_flathub_or_none() {
    let server = MockServer::start().await;
    // A Wayland session takes the Flatpak.
    let f = fixture(&server, host(Arch::X86_64, true)).await;
    let managed = install(&f.tools).await.unwrap();
    assert_eq!(managed.source, "flatpak");
    let flatpak = EmulatorInstall::Flatpak {
        app_id: "org.example.Viewer".into(),
    };
    assert_eq!(f.tools.find("moonlight").await.unwrap(), Some(flatpak));
    // Flatpak updates its own: no check reaches GitHub.
    assert!(f.tools.check("moonlight").await.unwrap().latest.is_none());
    f.tools.remove("moonlight").await.unwrap();
    let log = read(&f.temp.path().join("flatpak.log"));
    assert!(
        log.contains("install --user --noninteractive -y flathub org.example.Viewer"),
        "{log}"
    );
    assert!(log.contains("uninstall --user --noninteractive -y org.example.Viewer"));
    assert_eq!(f.tools.find("moonlight").await.unwrap(), None);

    // Nothing suits a machine the definition doesn't name.
    let f = fixture(&server, host(Arch::Other, false)).await;
    assert!(install(&f.tools).await.is_err());
    assert!(f.tools.managed("moonlight").await.unwrap().is_none());
}

/// A removal deletes only what the release shipped, inside the folder.
#[test]
fn removal_spares_what_the_release_didnt_ship() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("tool");
    let outside = temp.path().join("outside.txt");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::write(dir.join("bin/tool"), "").unwrap();
    std::fs::write(&outside, "keep").unwrap();
    unpack::record_shipped(&dir).unwrap();
    // A tampered record naming a file outside the folder.
    let record = dir.join(".caskade/shipped.json");
    std::fs::write(&record, r#"["bin/tool", "../outside.txt"]"#).unwrap();
    std::fs::write(dir.join("settings.conf"), "mine").unwrap();
    assert_eq!(unpack::remove_shipped(&dir).unwrap(), 1);
    assert_eq!(read(&outside), "keep");
    assert_eq!(read(&dir.join("settings.conf")), "mine");
    assert!(!dir.join("bin").exists());
    // A folder Caskade didn't record loses nothing.
    assert_eq!(unpack::remove_shipped(&dir).unwrap(), 0);
    assert!(dir.join("settings.conf").exists());
}

/// A tool that asks for it gets its AppImage unpacked (`--appimage-extract`),
/// and its program is found inside.
#[tokio::test]
async fn an_appimage_tool_is_unpacked_and_found_inside() {
    let temp = tempfile::tempdir().unwrap();
    let image = temp.path().join("Viewer-1.0-x86_64.AppImage");
    std::fs::write(
        &image,
        "#!/bin/sh\n[ \"$1\" = --appimage-extract ] || exit 9\nmkdir -p squashfs-root/usr/bin\nprintf run > squashfs-root/usr/bin/viewer\nprintf x > squashfs-root/AppRun\n",
    )
    .unwrap();
    let rules = crate::emulation::DetectRules {
        executable: Some("^viewer$".into()),
        ..Default::default()
    };
    let target = temp.path().join("data/streaming/client");
    let release = |extract_appimage| Release {
        file: image.clone(),
        staging: temp.path().join("data/.staging"),
        aside: temp.path().join("data/.old"),
        target: target.clone(),
        rules: rules.clone(),
        os: OperatingSystem::Linux,
        old_version: "previous".into(),
        name: "Viewer".into(),
        missing: "missing".into(),
        extract_appimage,
        cancel: CancellationToken::new(),
    };
    let install = install_release(release(true)).await.unwrap();
    assert_eq!(
        install,
        EmulatorInstall::Folder {
            dir: target.join("usr/bin"),
            executable: "viewer".into(),
        }
    );
    assert_eq!(read(&target.join("usr/bin/viewer")), "run");
    assert!(!target.join("Viewer-1.0-x86_64.AppImage").exists());
    // Without asking, the AppImage itself is the install.
    let install = install_release(release(false)).await.unwrap();
    assert!(matches!(install, EmulatorInstall::AppImage { .. }));
}
