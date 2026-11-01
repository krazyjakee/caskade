//! A Windows installer on Linux without umu-launcher and without a network:
//! umu is fetched only when a candidate runs a program under it.
#![cfg(all(feature = "fetch", feature = "windows-formats"))]
use caskade_installer::{
    InstallOutcome, Strategy,
    exec::CancellationToken,
    pipeline::{self, Context, input},
    runtime::umu::UmuBootstrap,
    testing::RecordingSink,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

fn installer() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/windows/inno60-basic/setup.exe")
}

/// No umu on the host; its download answers 503 (the network is down).
fn context(umu: &Path, server: &MockServer) -> Context {
    let mut ctx = Context::new(Arc::new(RecordingSink::default()), CancellationToken::new());
    ctx.host = Some(input::host(Some("linux-no-umu")).unwrap());
    ctx.umu = Some(UmuBootstrap::new(umu).with_url(format!("{}/umu.tar", server.uri())));
    ctx
}

async fn install(root: &Path, ctx: &Context) -> PathBuf {
    let outcome = pipeline::install(
        installer().to_string_lossy().into_owned(),
        root.into(),
        false,
        false,
        None,
        ctx,
    )
    .await
    .unwrap();
    let InstallOutcome::Installed(installed) = outcome else {
        panic!("{outcome:?}");
    };
    root.join(installed.dir)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_windows_installer_extracts_and_uninstalls_without_downloading_umu() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let data = tempfile::tempdir().unwrap();
    let roots = tempfile::tempdir().unwrap();
    let ctx = context(data.path(), &server);
    let game = install(&roots.path().join("first"), &ctx).await;
    let plans = ctx.control.plans().unwrap();
    assert_eq!(
        plans.candidates[plans.selected.unwrap() as usize].strategy,
        Strategy::Extract
    );
    assert!(server.received_requests().await.unwrap().is_empty());
    // The game's prefix runtime is never needed to delete its folder.
    pipeline::uninstall(game.clone(), vec![], &ctx)
        .await
        .unwrap();
    assert!(!game.exists());
    assert!(server.received_requests().await.unwrap().is_empty());

    // Asked to run the installer, which needs umu: the run fails (umu cannot
    // be downloaded), and the extract candidate installs the game instead.
    let run = plans
        .candidates
        .iter()
        .find(|candidate| candidate.strategy == Strategy::RunSilent)
        .expect("a Proton run candidate")
        .clone();
    let mut ctx = context(data.path(), &server);
    ctx.control.candidate = Some(run);
    let game = install(&roots.path().join("second"), &ctx).await;
    assert!(game.is_dir());
    assert!(UmuBootstrap::new(data.path()).installed().is_none());
}
