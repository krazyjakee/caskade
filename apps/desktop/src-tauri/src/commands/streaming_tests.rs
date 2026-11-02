//! The host's lifetime on Tauri's mock runtime: closing the main window to
//! the tray keeps it running; quitting stops it.

use super::*;
use caskade_core::{
    AppPaths,
    bus::Bus,
    db::LibraryStore,
    elevation::Elevation,
    emulation::OperatingSystem,
    managed::{ManagedTools, ToolCatalog},
    settings::SettingsService,
    streaming::{
        host::{
            Parts, Timing,
            process::{HostLaunch, HostRunner},
        },
        secrets::HostSecrets,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::test::{mock_builder, mock_context, noop_assets};

/// A host process that only remembers whether it runs.
#[derive(Default)]
struct Fake(AtomicBool);

impl HostRunner for Fake {
    fn start(&self, _: &HostLaunch) -> caskade_core::Result<()> {
        self.0.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn stop(&self) -> caskade_core::Result<()> {
        self.0.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn running(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[test]
fn closing_to_the_tray_keeps_the_host_and_quitting_stops_it() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(temp.path());
    let store = tauri::async_runtime::block_on(LibraryStore::in_memory()).unwrap();
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths).unwrap();
    let fake = Arc::new(Fake::default());
    let host = StreamingHost::with(
        ManagedTools::new(store, Bus::default(), ToolCatalog::default(), &paths),
        Elevation::new(&paths),
        settings.clone(),
        Parts {
            existing: Arc::new(|| None),
            os: OperatingSystem::Linux,
            runner: fake.clone(),
            secrets: HostSecrets::memory(),
            timing: Timing::default(),
            api_base: "https://127.0.0.1:1/".parse().unwrap(),
            http_port: 1,
        },
    );
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    crate::tray::attach(app.handle(), &settings);
    app.state::<crate::tray::Tray>().set_shown(true);
    app.manage(host);
    // As if it were running.
    fake.start(&HostLaunch {
        program: "host".into(),
        args: Vec::new(),
        env: Vec::new(),
        cwd: temp.path().into(),
    })
    .unwrap();

    // Closed to the tray: hidden, still hosting.
    window.close().unwrap();
    assert!(fake.running());
    assert!(app.get_webview_window("main").is_some());

    // Quit: the app exits and the host stops with it.
    crate::run_event(app.handle(), tauri::RunEvent::Exit);
    assert!(!fake.running());
}

#[test]
fn a_pairing_that_fails_says_why_in_caskades_words() {
    use caskade_core::streaming::pairing::Paired;
    let (outcome, message) = pairing_outcome(&Ok(Paired::WrongCode), "Den");
    assert_eq!(outcome, PairingOutcome::WrongCode);
    assert_eq!(
        message.as_deref(),
        Some("The code entered on Den didn't match. Try again with a new code.")
    );
    assert_eq!(
        pairing_outcome(&Ok(Paired::Declined), "Den").1.as_deref(),
        Some("Den declined.")
    );
    assert_eq!(
        pairing_outcome(&Ok(Paired::Expired), "Den").0,
        PairingOutcome::Expired
    );
    let paired = Paired::Paired {
        server_cert_pem: String::new(),
    };
    assert_eq!(
        pairing_outcome(&Ok(paired), "Den"),
        (PairingOutcome::Paired, None)
    );
    for (_, message) in [
        pairing_outcome(&Ok(Paired::WrongCode), "Den"),
        pairing_outcome(&Ok(Paired::Expired), "Den"),
    ] {
        let message = message.unwrap().to_lowercase();
        assert!(!message.contains("moonlight") && !message.contains("sunshine"));
    }
}
