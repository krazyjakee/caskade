//! Updating, repairing and switching off a host that has paired devices.

use super::*;

/// Hosting switched on and answering; returns once it is Ready.
async fn on(f: &Fixture) {
    let host = f.host.clone();
    let enabling = tokio::spawn(async move { host.enable().await });
    until("credentials made", || at_once(f.secrets.get()).is_some()).await;
    answer(f).await;
    assert_eq!(enabling.await.unwrap().unwrap(), HostOutcome::On);
    until("ready", || f.host.status().state == HostState::Ready).await;
}

/// The host's `/serverinfo`: streaming or not.
async fn streaming(f: &Fixture, busy: bool) {
    f.serverinfo.reset().await;
    let state = if busy {
        "SUNSHINE_SERVER_BUSY"
    } else {
        "SUNSHINE_SERVER_FREE"
    };
    Mock::given(method("GET"))
        .and(path("/serverinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            "<root status_code=\"200\"><hostname>Den</hostname><uniqueid>ABC</uniqueid><state>{state}</state><currentgame>{}</currentgame></root>",
            if busy { "881" } else { "0" }
        )))
        .mount(&f.serverinfo)
        .await;
}

/// A newer release, as GitHub serves it.
async fn release(f: &Fixture, version: &str) {
    f.github.reset().await;
    Mock::given(method("HEAD"))
        .and(path("/example/host/releases/latest"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("/example/host/releases/tag/v{version}")),
        )
        .mount(&f.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/example/host/releases/expanded_assets/v{version}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<a href="/example/host/releases/download/v{version}/host-{version}.zip">host</a>"#
        )))
        .mount(&f.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/example/host/releases/download/v{version}/host-{version}.zip"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip(&[
            (
                &format!("host-{version}/usr/bin/sunshine"),
                &format!("{HOST}# {version}\n"),
            ),
            (
                &format!("host-{version}/usr/share/sunshine/apps.json"),
                "{}",
            ),
        ])))
        .mount(&f.github)
        .await;
}

/// The host's own files as they are after pairing a device and publishing
/// a game.
fn paired(files: &HostFiles) {
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(files.state()).unwrap()).unwrap();
    state["root"] = serde_json::json!({
        "uniqueid": "18D1CF0D",
        "named_devices": [{ "name": "Deck", "cert": "PEM", "uuid": "U1" }]
    });
    std::fs::write(files.state(), state.to_string()).unwrap();
    std::fs::write(files.apps(), r#"{"env":{},"apps":[{"name":"Celeste"}]}"#).unwrap();
}

fn kept_pairing(files: &HostFiles) -> bool {
    let state = std::fs::read_to_string(files.state()).unwrap();
    state.contains("Deck") && state.contains("18D1CF0D")
}

#[tokio::test(flavor = "multi_thread")]
async fn an_update_waits_for_the_stream_then_keeps_pairings_and_settings() {
    let f = fixture(fast()).await;
    streaming(&f, false).await;
    on(&f).await;
    let files = HostFiles::linux(&f.staging.join("config"));
    paired(&files);
    let prompts = caskade_core::lock(&f.system.prompts).len();
    release(&f, "1.1").await;

    // Never during a stream: the host keeps running the old version.
    streaming(&f, true).await;
    assert!(f.host.update().await.is_err());
    assert!(f.host.inner.runner.running());
    assert_eq!(
        f.host
            .inner
            .tools
            .managed(TOOL)
            .await
            .unwrap()
            .unwrap()
            .installed
            .as_deref(),
        Some("v1.0")
    );

    // Once the game ends: stopped, swapped, configured and started again.
    streaming(&f, false).await;
    assert_eq!(f.host.update().await.unwrap(), HostOutcome::On);
    let managed = f.host.inner.tools.managed(TOOL).await.unwrap().unwrap();
    assert_eq!(managed.installed.as_deref(), Some("v1.1"));
    assert!(
        std::fs::read_to_string(f.host.program(false).await.unwrap())
            .unwrap()
            .contains("# 1.1")
    );
    assert!(kept_pairing(&files), "paired devices stay");
    assert!(
        std::fs::read_to_string(files.apps())
            .unwrap()
            .contains("Celeste"),
        "published games stay"
    );
    assert!(
        std::fs::read_to_string(files.conf())
            .unwrap()
            .contains("sunshine_name = Livingroom")
    );
    until("ready after the update", || {
        f.host.status().state == HostState::Ready
    })
    .await;
    // The rules were all there: no prompt.
    assert_eq!(caskade_core::lock(&f.system.prompts).len(), prompts);
    f.host.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn repair_fixes_each_breakage() {
    let f = fixture(fast()).await;
    streaming(&f, false).await;
    on(&f).await;
    let files = HostFiles::linux(&f.staging.join("config"));
    paired(&files);
    let prompts = || caskade_core::lock(&f.system.prompts).len();

    // A program file gone.
    let shipped = f
        .host
        .program(false)
        .await
        .unwrap()
        .parent()
        .unwrap()
        .join("../share/sunshine/apps.json");
    std::fs::remove_file(&shipped).unwrap();
    assert_eq!(f.host.repair().await.unwrap(), HostOutcome::On);
    assert!(shipped.exists());
    until("ready after files", || {
        f.host.status().state == HostState::Ready
    })
    .await;
    assert!(kept_pairing(&files));

    // The configuration drifted.
    std::fs::write(
        files.conf(),
        "sunshine_name = Someone else\nupnp = enabled\n",
    )
    .unwrap();
    f.host.repair().await.unwrap();
    let conf = std::fs::read_to_string(files.conf()).unwrap();
    assert!(conf.contains("sunshine_name = Livingroom") && conf.contains("upnp = disabled"));
    until("ready after config", || {
        f.host.status().state == HostState::Ready
    })
    .await;

    // The OS removed the rules: one prompt puts back only those.
    let before = prompts();
    caskade_core::lock(&f.system.present).remove(&Change::UdevRules);
    f.host.repair().await.unwrap();
    assert_eq!(prompts(), before + 1);
    assert_eq!(
        caskade_core::lock(&f.system.prompts).last().unwrap().steps,
        [Step::Add(Change::UdevRules)]
    );
    assert!(caskade_core::lock(&f.system.present).contains(&Change::UdevRules));
    until("ready after rules", || {
        f.host.status().state == HostState::Ready
    })
    .await;

    // The keychain lost the credentials: new ones, set on the host.
    let old = f.secrets.get().await.unwrap().unwrap();
    f.secrets.forget().await.unwrap();
    let host = f.host.clone();
    let repairing = tokio::spawn(async move { host.repair().await });
    until("new credentials", || {
        at_once(f.secrets.get()).is_some_and(|new| new != old)
    })
    .await;
    f.api.reset().await;
    answer(&f).await;
    assert_eq!(repairing.await.unwrap().unwrap(), HostOutcome::On);
    let new = f.secrets.get().await.unwrap().unwrap();
    assert!(
        std::fs::read_to_string(files.state())
            .unwrap()
            .contains(&new.user)
    );
    until("ready with new credentials", || {
        f.host.status().state == HostState::Ready
    })
    .await;

    // A refused sign-in (the host's copy drifted): set again.
    f.host.shutdown();
    std::fs::write(files.state(), r#"{"username":"someone-else"}"#).unwrap();
    f.host.repair().await.unwrap();
    assert!(
        std::fs::read_to_string(files.state())
            .unwrap()
            .contains(&new.user)
    );
    until("ready after the sign-in", || {
        f.host.status().state == HostState::Ready
    })
    .await;
    f.host.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn off_leaves_no_program_or_rules_and_on_again_pairs_with_no_new_code() {
    let f = fixture(fast()).await;
    streaming(&f, false).await;
    on(&f).await;
    let files = HostFiles::linux(&f.staging.join("config"));
    paired(&files);
    let credentials = f.secrets.get().await.unwrap().unwrap();

    assert_eq!(f.host.disable().await.unwrap(), HostOutcome::Off);
    // No program file, rule or record of the administrator step remains.
    let program = f.host.program(false).await;
    assert!(program.is_err(), "{program:?}");
    assert!(caskade_core::lock(&f.system.present).is_empty());
    assert!(!f.staging.join("host-1.0").exists());
    assert!(!f.staging.join(".caskade/shipped.json").exists());
    assert!(
        f.host
            .inner
            .elevation
            .record(Plan::StreamingHost)
            .unwrap()
            .added
            .is_empty()
    );
    assert!(f.host.inner.tools.managed(TOOL).await.unwrap().is_none());
    // Its paired devices and sign-in stay.
    assert!(kept_pairing(&files));

    // On again: installed again, the same identity, nothing to pair.
    assert_eq!(f.host.enable().await.unwrap(), HostOutcome::On);
    assert!(f.host.program(false).await.unwrap().is_file());
    assert!(kept_pairing(&files));
    assert_eq!(f.secrets.get().await.unwrap().unwrap(), credentials);
    until("ready again", || f.host.status().state == HostState::Ready).await;
    f.host.shutdown();
}
