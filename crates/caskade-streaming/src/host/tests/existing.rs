//! A user's own host, used instead of installing one (#258).

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_users_own_host_is_used_with_consent_and_only_caskades_apps_change() {
    use crate::{
        existing::{Existing, Found},
        publish::{self, Offer},
    };
    let f = fixture(fast()).await;
    // Their Sunshine, its certificate where its install keeps it, and its
    // own app.
    let theirs = f
        .temp
        .path()
        .join("home/.config/sunshine/credentials/cacert.pem");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::copy(HostFiles::linux(&f.staging.join("config")).cert(), &theirs).unwrap();
    let apps = Apps::default();
    caskade_core::lock(&apps.0)
        .push(serde_json::json!({ "name": "Desktop", "image-path": "desktop.png" }));
    let user = Credentials {
        user: "sunshine".into(),
        password: "theirs".into(),
    };
    Mock::given(method("GET"))
        .and(path("/api/config"))
        .and(basic_auth("sunshine", "theirs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": true, "version": "2025.628.4510"
        })))
        .mount(&f.api)
        .await;
    Mock::given(path("/api/config"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&f.api)
        .await;
    Mock::given(path("/api/apps"))
        .and(basic_auth("sunshine", "theirs"))
        .respond_with(apps.clone())
        .mount(&f.api)
        .await;
    Mock::given(wiremock::matchers::path_regex("^/api/apps/[0-9]+$"))
        .and(basic_auth("sunshine", "theirs"))
        .respond_with(apps.clone())
        .mount(&f.api)
        .await;
    let found = Existing {
        found: vec![Found::Package],
        cert: Some(theirs.clone()),
    };
    let host = StreamingHost::with(
        f.host.inner.tools.clone(),
        f.host.inner.elevation.clone(),
        f.settings.clone(),
        Parts {
            existing: Arc::new(move || Some(found.clone())),
            os: OperatingSystem::Linux,
            runner: Arc::new(process::ChildRunner::new(f.temp.path().join("run.json"))),
            secrets: f.secrets.clone(),
            timing: fast(),
            api_base: f.host.inner.api_base.clone(),
            http_port: f.host.inner.http_port,
            checks: Arc::new(HostChecks::fine),
        },
    );

    // Switching on offers theirs: nothing installed, no prompt.
    assert_eq!(host.enable().await.unwrap(), HostOutcome::ExistingFound);
    assert!(f.host.inner.tools.managed(TOOL).await.unwrap().is_none());
    assert!(caskade_core::lock(&f.system.prompts).is_empty());
    assert!(!host.hosting().await);

    // A wrong password changes nothing; theirs works.
    let wrong = Credentials {
        password: "guess".into(),
        ..user.clone()
    };
    assert!(matches!(
        host.use_existing(wrong).await,
        Err(Error::Forbidden(_))
    ));
    assert!(!host.hosting().await);
    assert_eq!(
        host.use_existing(user.clone()).await.unwrap(),
        HostOutcome::On
    );
    assert!(host.hosting().await);
    assert_eq!(host.status().version.as_deref(), Some("2025.628.4510"));
    assert_eq!(f.secrets.existing().await.unwrap(), Some(user));

    // Caskade's games go in beside theirs.
    let caskade = publish::Caskade {
        program: std::env::current_exe().unwrap(),
        flatpak_host: false,
    };
    let api = host.api().await.unwrap();
    let offers = [Offer::Game {
        title: "Hades".into(),
        game_id: "g1".into(),
        image: None,
    }];
    let published = publish::sync(&api, &caskade, &offers).await.unwrap();
    assert_eq!(
        published,
        publish::Published {
            added: 1,
            removed: 0
        }
    );
    // Again: nothing to do.
    assert_eq!(
        publish::sync(&api, &caskade, &offers).await.unwrap(),
        Default::default()
    );
    let names: Vec<String> = caskade_core::lock(&apps.0)
        .iter()
        .map(|app| app["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Desktop", "Hades"]);
    assert_eq!(
        caskade_core::lock(&apps.0)[1]["cmd"],
        caskade.launch_command("g1")
    );

    // Hosting off: only Caskade's app goes; theirs, and their host, stay.
    assert_eq!(host.disable().await.unwrap(), HostOutcome::Off);
    let left = caskade_core::lock(&apps.0).clone();
    assert_eq!(
        left,
        [serde_json::json!({ "name": "Desktop", "image-path": "desktop.png" })]
    );
    assert!(!host.hosting().await);
    assert!(
        caskade_core::lock(&f.system.prompts).is_empty(),
        "never a prompt"
    );
}
