//! Original shared macOS launcher fixture setup (#96/#97/#103).
//! Inputs are synthetic metadata; no host launcher state is read.
#![allow(dead_code)]
use std::{fs, path::Path};
pub fn write(root: &Path, path: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(path.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
pub fn machine() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("root")).unwrap();
    write(t.path(), "machine.toml", "schema_version=1\nos='macos'\n");
    t
}
pub fn app(root: &Path, path: &str, id: &str, exe: &str) {
    let mut dict = plist::Dictionary::new();
    for (key, value) in [
        ("CFBundleIdentifier", id),
        ("CFBundleName", "Fixture bundle"),
        ("CFBundleExecutable", exe),
    ] {
        dict.insert(key.into(), plist::Value::String(value.into()));
    }
    let mut bytes = vec![];
    plist::Value::Dictionary(dict)
        .to_writer_xml(&mut bytes)
        .unwrap();
    write(root, &format!("{path}/Contents/Info.plist"), bytes);
    write(
        root,
        &format!("{path}/Contents/MacOS/{exe}"),
        b"inert target",
    );
}
pub fn steam(root: &Path) {
    let base = "Users/fixture/Library/Application Support/Steam/steamapps";
    write(
        root,
        &format!("{base}/libraryfolders.vdf"),
        r#""libraryfolders" { "0" { "path" "/Users/fixture/Library/Application Support/Steam" "apps" { "620" "1024" } } }"#,
    );
    write(
        root,
        &format!("{base}/appmanifest_620.acf"),
        r#""AppState" { "appid" "620" "name" "Fixture Steam" "StateFlags" "4" "installdir" "FixtureSteam" "SizeOnDisk" "1024" }"#,
    );
    write(
        root,
        &format!("{base}/common/FixtureSteam/game"),
        b"inert game",
    );
}
pub fn epic(root: &Path) {
    write(root,"Users/fixture/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests/MacEpic.item",serde_json::to_vec(&serde_json::json!({"AppName":"MacEpic","DisplayName":"Fixture Epic","CatalogNamespace":"FixtureNs","CatalogItemId":"FixtureItem","InstallLocation":"/Users/fixture/Games/FixtureEpic","LaunchExecutable":"game","bIsIncompleteInstall":false})).unwrap());
    write(root, "Users/fixture/Games/FixtureEpic/game", b"inert game");
    app(
        root,
        "Applications/Epic Games Launcher.app",
        "com.epicgames.EpicGamesLauncher",
        "EpicLauncherMac",
    );
}
pub fn heroic(root: &Path) {
    write(root,"Users/fixture/Library/Application Support/heroic/legendaryConfig/legendary/installed.json",serde_json::to_vec(&serde_json::json!({"MacHeroic":{"app_name":"MacHeroic","title":"Fixture Heroic","install_path":"/Users/fixture/Games/FixtureHeroic","executable":"game","platform":"Mac"}})).unwrap());
    write(
        root,
        "Users/fixture/Games/FixtureHeroic/game",
        b"inert game",
    );
}
pub fn battlenet(root: &Path) {
    use caskade_discovery::parsers::protobuf::{ProductDb, ProductInstall, UserSettings};
    use prost::Message;
    let db = ProductDb {
        products: vec![ProductInstall {
            uid: "wow".into(),
            product_code: "wow".into(),
            settings: Some(UserSettings {
                install_path: "/Applications/World of Warcraft".into(),
                play_region: String::new(),
                text_language: String::new(),
                speech_language: String::new(),
            }),
            cached_product_state: None,
        }],
    };
    write(
        root,
        "Users/Shared/Battle.net/Agent/product.db",
        db.encode_to_vec(),
    );
    write(root, "Applications/World of Warcraft/game", b"inert game");
    app(
        root,
        "Applications/Battle.net.app",
        "net.battle.net.app",
        "BattleNetMac",
    );
}
pub fn riot(root: &Path) {
    let base = "Users/Shared/Riot Games";
    write(
        root,
        &format!("{base}/RiotClientInstalls.json"),
        r#"{"rc_default":"/Applications/Riot Client.app/Contents/MacOS/RiotClientServices"}"#,
    );
    write(
        root,
        &format!(
            "{base}/Metadata/league_of_legends.live/league_of_legends.live.product_settings.yaml"
        ),
        "product_install_full_path: /Applications/League of Legends.app\nshortcut_name: League of Legends\n",
    );
    app(
        root,
        "Applications/Riot Client.app",
        "com.riotgames.RiotGames.RiotClient",
        "RiotClientServices",
    );
    write(
        root,
        "Applications/League of Legends.app/Contents/MacOS/LeagueClient",
        b"inert game",
    );
}
#[cfg(feature = "sqlite")]
pub fn gog(root: &Path) {
    let path = root.join("Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    rusqlite::Connection::open(path).unwrap().execute_batch("CREATE TABLE InstalledBaseProducts(productId INTEGER, installationPath TEXT);CREATE TABLE LimitedDetails(productId INTEGER,title TEXT);CREATE TABLE GameTimes(releaseKey TEXT, minutesInGame INTEGER);CREATE TABLE LastPlayedDates(gameReleaseKey TEXT,lastPlayedDate TEXT);INSERT INTO InstalledBaseProducts VALUES(1207658924,'/Users/fixture/Games/FixtureGog');INSERT INTO LimitedDetails VALUES(1207658924,'Fixture GOG');").unwrap();
    write(
        root,
        "Users/fixture/Games/FixtureGog/goggame-1207658924.info",
        r#"{"gameId":"1207658924","name":"Fixture GOG","playTasks":[{"isPrimary":true,"type":"FileTask","path":"Game.app/Contents/MacOS/game"}]}"#,
    );
    write(
        root,
        "Users/fixture/Games/FixtureGog/Game.app/Contents/MacOS/game",
        b"inert game",
    );
    app(
        root,
        "Applications/GOG Galaxy.app",
        "com.gog.galaxy",
        "GalaxyMac",
    );
}
#[cfg(feature = "sqlite")]
pub fn itch(root: &Path) {
    let path = root.join("Users/fixture/Library/Application Support/itch/butler.db");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    rusqlite::Connection::open(path).unwrap().execute_batch("CREATE TABLE install_locations(id TEXT PRIMARY KEY,path TEXT);CREATE TABLE games(id INTEGER PRIMARY KEY,title TEXT);CREATE TABLE uploads(id INTEGER PRIMARY KEY,windows TEXT,linux TEXT,osx TEXT);CREATE TABLE caves(id TEXT PRIMARY KEY,game_id INTEGER,upload_id INTEGER,install_location_id TEXT,install_folder_name TEXT,custom_install_folder TEXT,seconds_run INTEGER,installed_size INTEGER);INSERT INTO install_locations VALUES('mac','/Users/fixture/Games');INSERT INTO games VALUES(123,'Fixture Itch');INSERT INTO uploads VALUES(100,NULL,NULL,'all');INSERT INTO caves VALUES('mac-cave',123,100,'mac','FixtureItch',NULL,0,2048);").unwrap();
    write(root, "Users/fixture/Games/FixtureItch/game", b"inert game");
}
