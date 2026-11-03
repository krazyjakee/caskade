//! Amazon Games, Riot Client and Rockstar Games Launcher (#88), each on a
//! Windows machine built at test time.

use std::path::Path;

use crate::fixtures::write;

use caskade_discovery::{
    ExternalId, HostEnv, InstallState, LaunchTarget, ScanCtx, Source,
    sources::{riot::RiotClient, rockstar::RockstarLauncher},
};

fn windows(root: &Path) -> std::path::PathBuf {
    write(
        root,
        "machine.toml",
        "schema_version = 1\nos = \"windows\"\n",
    );
    root.join("root/drives/c")
}

fn exec(target: &LaunchTarget) -> (&Path, &[String]) {
    match target {
        LaunchTarget::Exec { program, args, .. } => (program, args),
        other => panic!("{other:?}"),
    }
}

#[cfg(feature = "sqlite")]
#[test]
fn amazon_games() {
    use caskade_discovery::sources::amazon::AmazonGames;
    let temp = crate::fixtures::tempdir();
    let c = windows(temp.path());
    let data = c.join("Users/fixture/AppData/Local/Amazon Games");
    write(&data, "App/Amazon Games.exe", "inert");
    write(&c, "Amazon Games/Library/Fixture Racer/racer.exe", "inert");
    let db = data.join("Data/Games/Sql/GameInstallInfo.sqlite");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            r"CREATE TABLE DbSet (Id TEXT PRIMARY KEY, ProductTitle TEXT, InstallDirectory TEXT, Installed INTEGER);
              INSERT INTO DbSet VALUES
                ('amzn1.adg.product.fixture-0001', 'Fixture Racer', 'C:\Amazon Games\Library\Fixture Racer', 1),
                ('amzn1.adg.product.fixture-0002', 'Fixture Uninstalled', 'C:\Amazon Games\Library\Gone', 0);",
        )
        .unwrap();
    let env = HostEnv::fixture(temp.path()).unwrap();
    let games = AmazonGames.scan(&env, &ScanCtx::default()).unwrap();
    assert_eq!(games.len(), 1);
    let racer = &games[0];
    assert_eq!(racer.title, "Fixture Racer");
    assert_eq!(
        racer.ids,
        [ExternalId::Amazon {
            id: "amzn1.adg.product.fixture-0001".into()
        }]
    );
    assert_eq!(
        racer.launch,
        [LaunchTarget::Uri {
            uri: "amazon-games://play/amzn1.adg.product.fixture-0001".into()
        }]
    );
    assert_eq!(racer.state, InstallState::Installed);
    let launcher = &AmazonGames.detect(&env)[0];
    assert!(
        launcher.found
            && launcher
                .path
                .as_ref()
                .unwrap()
                .ends_with("App/Amazon Games.exe")
    );
}

#[test]
fn riot_client() {
    let temp = crate::fixtures::tempdir();
    let c = windows(temp.path());
    let riot = c.join("ProgramData/Riot Games");
    write(
        &riot,
        "RiotClientInstalls.json",
        r#"{"associated_client": {}, "rc_default": "C:/Riot Games/Riot Client/RiotClientServices.exe"}"#,
    );
    write(&c, "Riot Games/Riot Client/RiotClientServices.exe", "inert");
    write(&c, "Riot Games/VALORANT/live/VALORANT.exe", "inert");
    write(&c, "Riot Games/Fixture Brawl/brawl.exe", "inert");
    write(
        &riot,
        "Metadata/valorant.live/valorant.live.product_settings.yaml",
        "product_install_full_path: \"C:/Riot Games/VALORANT/live\"\nproduct_install_root: \"C:/Riot Games\"\nshortcut_name: VALORANT.lnk\nshould_repair: false\n",
    );
    write(
        &riot,
        "Metadata/fixture_brawl.pbe/fixture_brawl.pbe.product_settings.yaml",
        "product_install_full_path: \"C:/Riot Games/Fixture Brawl\"\nshortcut_name: Fixture Brawl (PBE).lnk\n",
    );
    write(
        &riot,
        "Metadata/riot_client.live/riot_client.live.product_settings.yaml",
        "product_install_full_path: \"C:/Riot Games/Riot Client\"\n",
    );
    write(
        &riot,
        "Metadata/broken.live/broken.live.product_settings.yaml",
        "product_install_full_path: [unclosed\n",
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let ctx = ScanCtx::default();
    let games = RiotClient.scan(&env, &ctx).unwrap();
    assert_eq!(ctx.take_warnings().len(), 1);
    let titles: Vec<_> = games
        .iter()
        .map(|game| (game.key.as_str(), game.title.as_str()))
        .collect();
    // The Riot Client itself is not a game.
    assert_eq!(
        titles,
        [
            ("fixture_brawl", "Fixture Brawl (PBE)"),
            ("valorant", "VALORANT")
        ]
    );
    let valorant = &games[1];
    assert_eq!(
        valorant.ids,
        [ExternalId::Riot {
            product: "valorant".into()
        }]
    );
    let (program, args) = exec(&valorant.launch[0]);
    assert!(program.ends_with("Riot Client/RiotClientServices.exe"));
    assert_eq!(
        args,
        ["--launch-product=valorant", "--launch-patchline=live"]
    );
    assert_eq!(exec(&games[0].launch[0]).1[1], "--launch-patchline=pbe");
    assert!(
        valorant
            .install_dir
            .as_ref()
            .unwrap()
            .ends_with("Riot Games/VALORANT/live")
    );
    assert!(RiotClient.detect(&env)[0].path.is_some());
}

#[test]
fn rockstar_games_launcher() {
    let temp = crate::fixtures::tempdir();
    let c = windows(temp.path());
    write(
        &c,
        "Program Files/Rockstar Games/Launcher/Launcher.exe",
        "inert",
    );
    write(
        &c,
        "Program Files/Rockstar Games/Fixture Heist/heist.exe",
        "inert",
    );
    write(
        temp.path(),
        "registry/hklm32.reg",
        "Windows Registry Editor Version 5.00\n\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\Rockstar Games\\Launcher]\n\"InstallFolder\"=\"C:\\\\Program Files\\\\Rockstar Games\\\\Launcher\"\n\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\Rockstar Games\\Rockstar Games Social Club]\n\"InstallFolder\"=\"C:\\\\Program Files\\\\Rockstar Games\\\\Social Club\"\n\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\Rockstar Games\\Fixture Heist]\n\"InstallFolder\"=\"C:\\\\Program Files\\\\Rockstar Games\\\\Fixture Heist\"\n\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\Rockstar Games\\Fixture Removed]\n\"InstallFolder\"=\"D:\\\\Gone\"\n\n\
[HKEY_LOCAL_MACHINE\\SOFTWARE\\Rockstar Games\\No Folder]\n\"Language\"=\"en-US\"\n",
    );
    let env = HostEnv::fixture(temp.path()).unwrap();
    let games = RockstarLauncher.scan(&env, &ScanCtx::default()).unwrap();
    let titles: Vec<_> = games.iter().map(|game| game.title.as_str()).collect();
    assert_eq!(titles, ["Fixture Heist", "Fixture Removed"]);
    let heist = &games[0];
    assert_eq!(
        heist.ids,
        [ExternalId::Rockstar {
            title: "Fixture Heist".into()
        }]
    );
    let (program, args) = exec(&heist.launch[0]);
    assert!(program.ends_with("Rockstar Games/Launcher/Launcher.exe"));
    assert_eq!(args[0], "-launchTitleInFolder");
    // A host path: compared by component, as `program` is.
    assert!(std::path::Path::new(&args[1]).ends_with("Rockstar Games/Fixture Heist"));
    assert_eq!(heist.state, InstallState::Installed);
    assert!(matches!(games[1].state, InstallState::Broken { .. }));
    assert!(RockstarLauncher.detect(&env)[0].found);
}
