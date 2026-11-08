//! The shipped tool definitions against the releases they were written for:
//! every machine picks exactly the file the plan
//! names, and every asset pattern matches one recorded file. An upstream
//! rename shows up here, and in the live test, as a one-line fix.

use caskade_core::{
    emulation::{InstallSource, OperatingSystem},
    managed::{Arch, Host},
};
use caskade_sources::tools::catalog;

/// Every file of Sunshine v2026.914.233613 (2026-09-15), as GitHub lists it.
const SUNSHINE: &[&str] = &[
    "flathub.tar.gz",
    "sunshine_2026.914.233613-1+debiantrixie_amd64.deb",
    "sunshine_2026.914.233613-1+debiantrixie_arm64.deb",
    "Sunshine-2026.914.233613-1.fc43.aarch64.rpm",
    "Sunshine-2026.914.233613-1.fc43.x86_64.rpm",
    "Sunshine-2026.914.233613-1.fc46.x86_64.rpm",
    "Sunshine-2026.914.233613-1.suse.tw.x86_64.rpm",
    "sunshine_2026.914.233613-1+ubuntu24.04_amd64.deb",
    "sunshine-2026.914.233613-1-x86_64.pkg.tar.zst",
    "Sunshine_2026.914.233613_aarch64.AppImage",
    "sunshine_2026.914.233613_alpine3.24_x86_64.apk",
    "Sunshine_2026.914.233613_x86_64.AppImage",
    "sunshine_aarch64.flatpak",
    "sunshine_debug_x86_64.flatpak",
    "Sunshine-FreeBSD-15.1-amd64.pkg",
    "Sunshine-macOS-arm64.dmg",
    "Sunshine-macOS-x86_64.dmg",
    "sunshine.pkg.tar.gz",
    "sunshine.rb",
    "Sunshine-Windows-AMD64-debuginfo.7z",
    "Sunshine-Windows-AMD64-installer.msi",
    "Sunshine-Windows-AMD64-lite.zip",
    "Sunshine-Windows-ARM64-debuginfo.7z",
    "Sunshine-Windows-ARM64-installer.msi",
    "Sunshine-Windows-ARM64-lite.zip",
    "sunshine_x86_64.flatpak",
];

/// Every file of moonlight-qt v6.2.0 (2026-10-04).
const MOONLIGHT: &[&str] = &[
    "Moonlight-6.2.0.dmg",
    "Moonlight-6.2.0-x86_64.AppImage",
    "MoonlightPortable-arm64-6.2.0.zip",
    "MoonlightPortable-x64-6.2.0.zip",
    "MoonlightSetup-6.2.0.exe",
    "MoonlightSrc-6.2.0.tar.gz",
    "Moonlight-SteamLink-6.2.0.zip",
];

fn assets(tool: &str) -> &'static [&'static str] {
    match tool {
        "sunshine" => SUNSHINE,
        "moonlight" => MOONLIGHT,
        other => panic!("no recorded release for {other}"),
    }
}

fn host(os: OperatingSystem, arch: Arch, wayland: bool, gamescope: bool) -> Host {
    Host {
        os,
        arch,
        wayland,
        gamescope,
    }
}

/// What a machine installs: the recorded file, or `flatpak:<app>`.
fn pick(tool: &str, host: &Host) -> Option<String> {
    let catalog = catalog().unwrap();
    let source = catalog.tool(tool).unwrap().source(host)?;
    Some(match source {
        InstallSource::Github {
            asset: Some(pattern),
            ..
        } => {
            let pattern = regex::Regex::new(pattern).unwrap();
            let found: Vec<&str> = assets(tool)
                .iter()
                .copied()
                .filter(|name| pattern.is_match(name))
                .collect();
            assert_eq!(found.len(), 1, "{tool} on {host:?}: {found:?}");
            found[0].to_owned()
        }
        InstallSource::Flatpak { app } => format!("flatpak:{app}"),
        other => panic!("{tool}: unexpected source {other:?}"),
    })
}

#[test]
fn every_machine_installs_the_file_the_plan_names() {
    use Arch::*;
    use OperatingSystem::*;
    let table = [
        (
            "sunshine",
            host(Windows, X86_64, false, false),
            Some("Sunshine-Windows-AMD64-lite.zip"),
        ),
        (
            "sunshine",
            host(Windows, Aarch64, false, false),
            Some("Sunshine-Windows-ARM64-lite.zip"),
        ),
        (
            "sunshine",
            host(Linux, X86_64, false, false),
            Some("Sunshine_2026.914.233613_x86_64.AppImage"),
        ),
        (
            "sunshine",
            host(Linux, X86_64, true, false),
            Some("Sunshine_2026.914.233613_x86_64.AppImage"),
        ),
        (
            "sunshine",
            host(Linux, Aarch64, true, false),
            Some("Sunshine_2026.914.233613_aarch64.AppImage"),
        ),
        (
            "sunshine",
            host(Linux, Other, false, false),
            Some("flatpak:dev.lizardbyte.app.Sunshine"),
        ),
        // No macOS hosts in the first version.
        ("sunshine", host(Macos, Aarch64, false, false), None),
        (
            "moonlight",
            host(Windows, X86_64, false, false),
            Some("MoonlightPortable-x64-6.2.0.zip"),
        ),
        (
            "moonlight",
            host(Windows, Aarch64, false, false),
            Some("MoonlightPortable-arm64-6.2.0.zip"),
        ),
        (
            "moonlight",
            host(Linux, X86_64, false, false),
            Some("Moonlight-6.2.0-x86_64.AppImage"),
        ),
        (
            "moonlight",
            host(Linux, X86_64, true, false),
            Some("flatpak:com.moonlight_stream.Moonlight"),
        ),
        (
            "moonlight",
            host(Linux, X86_64, false, true),
            Some("flatpak:com.moonlight_stream.Moonlight"),
        ),
        (
            "moonlight",
            host(Linux, Aarch64, false, false),
            Some("flatpak:com.moonlight_stream.Moonlight"),
        ),
        (
            "moonlight",
            host(Linux, Other, false, false),
            Some("flatpak:com.moonlight_stream.Moonlight"),
        ),
        (
            "moonlight",
            host(Macos, Aarch64, false, false),
            Some("Moonlight-6.2.0.dmg"),
        ),
        (
            "moonlight",
            host(Macos, X86_64, false, false),
            Some("Moonlight-6.2.0.dmg"),
        ),
    ];
    for (tool, host, expected) in table {
        assert_eq!(pick(tool, &host).as_deref(), expected, "{tool} on {host:?}");
    }
}

#[test]
fn every_asset_pattern_matches_one_recorded_file() {
    let catalog = catalog().unwrap();
    let mut checked = 0;
    for tool in catalog.tools() {
        for sources in [
            &tool.install.windows,
            &tool.install.linux,
            &tool.install.macos,
        ] {
            for source in sources {
                let InstallSource::Github {
                    repo, asset, url, ..
                } = &source.source
                else {
                    continue;
                };
                assert!(url.is_none(), "{}: a URL template", tool.id);
                assert!(
                    matches!(
                        repo.as_str(),
                        "LizardByte/Sunshine" | "moonlight-stream/moonlight-qt"
                    ),
                    "{}: {repo}",
                    tool.id
                );
                let pattern = regex::Regex::new(asset.as_deref().unwrap()).unwrap();
                let found = assets(&tool.id)
                    .iter()
                    .filter(|name| pattern.is_match(name))
                    .count();
                assert_eq!(found, 1, "{}: {pattern}", tool.id);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 8);
}

/// Every release a machine downloads resolves on github.com today, through
/// the `releases/latest` redirect and the assets page (no API). Nothing is
/// downloaded.
/// `cargo test -p caskade-sources --features live-tests --test it tools::live_tool_sources -- --ignored --nocapture`
#[cfg(feature = "live-tests")]
#[tokio::test]
#[ignore = "hits github.com"]
async fn live_tool_sources_resolve() {
    use caskade_core::managed::{Hosts, resolve};
    let catalog = catalog().unwrap();
    let hosts = Hosts::default();
    let mut failed = Vec::new();
    for tool in catalog.tools() {
        for sources in [
            &tool.install.windows,
            &tool.install.linux,
            &tool.install.macos,
        ] {
            for source in sources {
                if matches!(source.source, InstallSource::Flatpak { .. }) {
                    continue;
                }
                match resolve(&hosts, &source.source).await {
                    Ok(resolved) => println!(
                        "{:<10} {:<22} {}",
                        tool.id,
                        resolved.version.unwrap_or_default(),
                        resolved.file
                    ),
                    Err(error) => failed.push(format!("{}: {error}", tool.id)),
                }
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Installs this machine's real releases into a temp data folder, finds
/// each with its detect rules, and removes it. Downloads ~60 MB on Linux.
/// `CASKADE_LIVE_TOOLS=moonlight` picks one.
#[cfg(feature = "live-tests")]
#[tokio::test]
#[ignore = "downloads the streaming programs from their projects"]
async fn live_tools_install_are_found_and_removed() {
    use caskade_core::{AppPaths, bus::Bus, db::LibraryStore, managed::ManagedTools};
    let temp = tempfile::tempdir().unwrap();
    let store = LibraryStore::in_memory().await.unwrap();
    let tools = ManagedTools::new(
        store,
        Bus::default(),
        catalog().unwrap(),
        &AppPaths::under(temp.path()),
    );
    let wanted =
        std::env::var("CASKADE_LIVE_TOOLS").unwrap_or_else(|_| "sunshine,moonlight".into());
    let cancel = caskade_core::managed::CancellationToken::new();
    for id in wanted.split(',') {
        if tools
            .source(id)
            .is_none_or(|source| matches!(source, InstallSource::Flatpak { .. }))
        {
            println!("{id}: no download for this machine");
            continue;
        }
        let managed = tools.install(id, &cancel, |_, _| {}).await.unwrap();
        let found = tools.find(id).await.unwrap();
        println!(
            "{id}: {} from {} → {found:?}",
            managed.installed.as_deref().unwrap_or("?"),
            managed.file.as_deref().unwrap_or("?")
        );
        assert!(found.is_some(), "{id} not found after installing");
        tools.remove(id).await.unwrap();
        assert!(tools.find(id).await.unwrap().is_none());
    }
}
