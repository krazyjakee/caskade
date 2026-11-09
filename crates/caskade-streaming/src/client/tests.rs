//! The client's own state: a stand-in Moonlight that keeps its state where
//! the real one does (portable or not), installed from a fake release.

use super::*;
use caskade_core::{
    AppPaths,
    bus::{Bus, BusOptions},
    db::LibraryStore,
    emulation::OperatingSystem,
    managed::{Arch, Host, Hosts, ToolCatalog, ToolDefinition},
    updates::GitHub,
};
use std::io::Write;
use url::Url;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

/// Like moonlight-qt: portable when `portable.dat` is in the working
/// directory, else the user's own settings. Each run records its argument.
const MOONLIGHT: &str = r#"#!/bin/sh
if [ -f "$PWD/portable.dat" ]; then
  dir="$PWD/Moonlight Game Streaming Project"; file="$dir/Moonlight.ini"
else
  dir="$HOME/.config/Moonlight Game Streaming Project"; file="$dir/Moonlight.conf"
fi
mkdir -p "$dir"
echo "host=$2" >> "$file"
"#;

const APP_RUN: &str = "#!/bin/sh\nexec \"$(dirname \"$0\")/usr/bin/moonlight\" \"$@\"\n";

const TOOL_TOML: &str = r#"
id = "moonlight"
name = "Moonlight"
dir = "streaming/client"

[detect.linux]
executable = "^moonlight$"

[[install.linux]]
when = ["wayland"]
source = "flatpak"
app = "com.moonlight_stream.Moonlight"

[[install.linux]]
source = "github"
repo = "example/client"
asset = '^client-[\d.]+\.zip$'
"#;

fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default().unix_permissions(0o755),
            )
            .unwrap();
        writer.write_all(body.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

async fn client(wayland: bool) -> (tempfile::TempDir, StreamingClient, MockServer) {
    client_with(wayland, MOONLIGHT).await
}

/// A client installed from a fake release whose Moonlight is `moonlight`.
pub(crate) async fn client_with(
    wayland: bool,
    moonlight: &str,
) -> (tempfile::TempDir, StreamingClient, MockServer) {
    let temp = tempfile::tempdir().unwrap();
    let github = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/example/client/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "/example/client/releases/tag/v6.2.0"),
        )
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path("/example/client/releases/expanded_assets/v6.2.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<a href="/example/client/releases/download/v6.2.0/client-6.2.0.zip">c</a>"#,
        ))
        .mount(&github)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/example/client/releases/download/v6.2.0/client-6.2.0.zip",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip(&[
            ("client/AppRun", APP_RUN),
            ("client/usr/bin/moonlight", moonlight),
        ])))
        .mount(&github)
        .await;
    let store = LibraryStore::in_memory().await.unwrap();
    let catalog = ToolCatalog::new(vec![ToolDefinition::parse(TOOL_TOML).unwrap()]).unwrap();
    let hosts = Hosts {
        github: GitHub::new(Url::parse(&format!("{}/", github.uri())).unwrap()),
        dolphin: Url::parse(&format!("{}/dolphin/", github.uri())).unwrap(),
    };
    let machine = Host {
        os: OperatingSystem::Linux,
        arch: Arch::X86_64,
        wayland,
        gamescope: false,
    };
    let flatpak = temp.path().join("flatpak");
    std::fs::write(
        &flatpak,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$1\" = info ] && exit 0\nexit 0\n",
            temp.path().join("flatpak.log").display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&flatpak, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let tools = ManagedTools::new(
        store,
        Bus::new(BusOptions::default()),
        catalog,
        &AppPaths::under(temp.path()),
    )
    .with_host(machine, hosts, flatpak.clone());
    let client = StreamingClient::new(tools).with_home(temp.path().join("home"), flatpak);
    (temp, client, github)
}

#[test]
fn quiet_settings_keep_moonlights_own_lines() {
    let text = "[General]\ncertificate=\"@ByteArray(x)\"\nrichpresence=true\nuniqueid=ab\n\n[hosts]\n1\\hostname=Den\n";
    let updated = set_general(text, &QUIET);
    assert!(updated.contains("certificate=\"@ByteArray(x)\"\n"));
    assert!(updated.contains("uniqueid=ab\n"));
    assert!(updated.contains("richpresence=false\n"));
    assert!(!updated.contains("richpresence=true"));
    for (key, value) in QUIET {
        assert_eq!(updated.matches(&format!("{key}=")).count(), 1, "{key}");
        assert!(updated.contains(&format!("{key}={value}")));
    }
    // The other section stays as it was, after [General]'s keys.
    assert!(updated.ends_with("[hosts]\n1\\hostname=Den\n"), "{updated}");
    // A new file gets the section.
    assert!(set_general("", &QUIET).starts_with("[General]\nconfwarnings=false\n"));
}

#[tokio::test]
async fn caskades_client_keeps_its_own_state_and_never_touches_the_users() {
    let (temp, client, _github) = client(false).await;
    let home = temp.path().join("home");
    // The user's own Moonlight, with a host of theirs.
    let theirs = home.join(".config/Moonlight Game Streaming Project/Moonlight.conf");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, "[hosts]\nmine=Office\n").unwrap();

    let ready = client.ensure(&CancellationToken::new()).await.unwrap();
    assert_eq!(ready.state, temp.path().join("data/streaming/client/state"));
    assert!(ready.state.join(PORTABLE).is_file());
    let status = client
        .command(&ready, &["pair".into(), "10.0.0.2".into()])
        .unwrap()
        .env("HOME", &home)
        .status()
        .unwrap();
    assert!(status.success());

    assert_eq!(
        std::fs::read_to_string(&theirs).unwrap(),
        "[hosts]\nmine=Office\n"
    );
    let ours = std::fs::read_to_string(ready.state.join(SETTINGS_DIR).join(SETTINGS_FILE)).unwrap();
    assert!(ours.contains("host=10.0.0.2"), "{ours}");
    assert!(ours.contains("richpresence=false") && ours.contains("uidisplaymode=0"));

    // Again, from its install: the state and what it learnt stay.
    let again = client.ensure(&CancellationToken::new()).await.unwrap();
    assert_eq!(again, ready);
    let ours = std::fs::read_to_string(ready.state.join(SETTINGS_DIR).join(SETTINGS_FILE)).unwrap();
    assert!(ours.contains("host=10.0.0.2"));
}

#[tokio::test]
async fn the_flatpak_runs_in_a_folder_of_its_own_data() {
    let (temp, client, _github) = client(true).await;
    let ready = client.ensure(&CancellationToken::new()).await.unwrap();
    let state = temp
        .path()
        .join("home/.var/app/com.moonlight_stream.Moonlight/caskade");
    assert_eq!(ready.state, state);
    assert!(state.join(PORTABLE).is_file());
    let command = client.command(&ready, &["list".into()]).unwrap();
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "run".to_owned(),
            format!("--cwd={}", state.display()),
            "com.moonlight_stream.Moonlight".to_owned(),
            "list".to_owned()
        ]
    );
}

/// A newer client release, as GitHub serves it.
async fn newer(github: &MockServer, version: &str) {
    github.reset().await;
    Mock::given(method("HEAD"))
        .and(path("/example/client/releases/latest"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "location",
            format!("/example/client/releases/tag/v{version}"),
        ))
        .mount(github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/example/client/releases/expanded_assets/v{version}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<a href="/example/client/releases/download/v{version}/client-{version}.zip">c</a>"#
        )))
        .mount(github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/example/client/releases/download/v{version}/client-{version}.zip"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip(&[
            ("client/AppRun", APP_RUN),
            (
                "client/usr/bin/moonlight",
                &format!("{MOONLIGHT}# {version}\n"),
            ),
        ])))
        .mount(github)
        .await;
}

#[tokio::test]
async fn an_update_waits_for_the_stream_and_keeps_the_clients_state() {
    let (_temp, client, github) = client(false).await;
    let ready = client.ensure(&CancellationToken::new()).await.unwrap();
    let ini = ready.state.join(SETTINGS_DIR).join(SETTINGS_FILE);
    // Paired with a host, its identity made.
    let mut text = std::fs::read_to_string(&ini).unwrap();
    text.push_str("\n[hosts]\n1\\hostname=Den\n1\\uuid=ABC\nsize=1\n");
    text = text.replace("[General]\n", "[General]\nuniqueid=0123abcd\n");
    std::fs::write(&ini, &text).unwrap();
    newer(&github, "6.3.0").await;

    // A stream holds the client: no update.
    let streaming = client.busy.clone().lock_owned().await;
    assert!(client.update(&CancellationToken::new()).await.is_err());
    drop(streaming);

    client.update(&CancellationToken::new()).await.unwrap();
    let managed = client.tools.managed(TOOL).await.unwrap().unwrap();
    assert_eq!(managed.installed.as_deref(), Some("v6.3.0"));
    let after = std::fs::read_to_string(&ini).unwrap();
    assert!(after.contains("uniqueid=0123abcd"), "{after}");
    assert!(after.contains("1\\uuid=ABC"), "{after}");
    for (key, value) in QUIET {
        assert!(after.contains(&format!("{key}={value}")), "{key}");
    }
}
