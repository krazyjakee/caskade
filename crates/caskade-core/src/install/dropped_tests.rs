use super::*;
use std::fs;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("home")).unwrap();
        Self { dir }
    }
    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }
    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn game(&self, relative: &str) -> PathBuf {
        self.write(relative, b"#!/bin/sh\necho game\n")
    }
    fn plan(&self, path: &Path) -> Result<DropPlan> {
        plan_at(path, &HostEnv::live(), &[self.path("home")])
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn launch(path: &str, args: &[&str]) -> Option<DropLaunch> {
    Some(DropLaunch {
        path: path.into(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        working_dir: None,
    })
}

#[test]
fn a_folder_installs_as_it_is() {
    let drop = Fixture::new();
    drop.game("Celeste/celeste.sh");
    let folder = drop.path("Celeste");
    let plan = drop.plan(&folder).unwrap();
    assert_eq!(plan.kind, DropKind::Folder);
    assert_eq!(plan.input, text(&folder));
    assert_eq!(plan.launch, None);
}

#[test]
fn a_game_executable_installs_its_folder_and_launches_itself() {
    let drop = Fixture::new();
    let exe = drop.game("Celeste/celeste.sh");
    drop.game("Celeste/tools/editor.sh");
    let plan = drop.plan(&exe).unwrap();
    assert_eq!(plan.kind, DropKind::Executable);
    assert_eq!(plan.dropped, text(&exe));
    assert_eq!(plan.input, text(&drop.path("Celeste")));
    assert_eq!(plan.launch, launch("celeste.sh", &[]));
    let hints = plan.hints();
    assert_eq!(hints.launch_targets[0].path, PathBuf::from("celeste.sh"));
    assert_eq!(hints.installer_title, None);
}

#[test]
fn an_executable_in_a_binaries_folder_installs_the_game_above_it() {
    let drop = Fixture::new();
    let exe = drop.game("Unreal Game/Binaries/Win64/Game-Shipping.sh");
    let plan = drop.plan(&exe).unwrap();
    assert_eq!(plan.input, text(&drop.path("Unreal Game")));
    assert_eq!(plan.launch, launch("Binaries/Win64/Game-Shipping.sh", &[]));
}

#[test]
fn an_executable_loose_in_a_user_folder_is_refused() {
    let drop = Fixture::new();
    let exe = drop.game("home/game.sh");
    let error = drop.plan(&exe).unwrap_err().to_string();
    assert!(error.contains("isn't in a folder of its own"), "{error}");
}

#[test]
fn installers_and_archives_install_as_they_are() {
    let drop = Fixture::new();
    let archive = drop.write(
        "home/Game.zip",
        b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    );
    let installer = drop.game("home/install.sh");
    for path in [archive, installer] {
        let plan = drop.plan(&path).unwrap();
        assert_eq!(plan.kind, DropKind::File, "{path:?}");
        assert_eq!(plan.input, text(&path));
        assert_eq!(plan.launch, None);
    }
}

#[test]
fn an_uninstaller_is_refused() {
    let drop = Fixture::new();
    let exe = drop.game("Game/uninstall.sh");
    let error = drop.plan(&exe).unwrap_err().to_string();
    assert!(error.contains("uninstaller"), "{error}");
}

#[test]
fn a_desktop_entry_installs_its_program_with_its_name_and_arguments() {
    let drop = Fixture::new();
    drop.game("Games/Hades/Hades.sh");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Hades\nExec=\"{}\" --windowed\nPath={}\n",
        drop.path("Games/Hades/Hades.sh").display(),
        drop.path("Games/Hades").display(),
    );
    let shortcut = drop.write("home/Hades.desktop", entry.as_bytes());
    let plan = drop.plan(&shortcut).unwrap();
    assert_eq!(plan.kind, DropKind::Shortcut);
    assert_eq!(plan.dropped, text(&shortcut));
    assert_eq!(plan.title.as_deref(), Some("Hades"));
    assert_eq!(plan.input, text(&drop.path("Games/Hades")));
    assert_eq!(plan.launch, launch("Hades.sh", &["--windowed"]));
    assert_eq!(plan.hints().installer_title.as_deref(), Some("Hades"));
}

#[test]
fn a_launchers_shortcut_points_at_the_launchers_settings() {
    let drop = Fixture::new();
    let shortcut = drop.write(
        "home/Portal.desktop",
        b"[Desktop Entry]\nType=Application\nName=Portal\nExec=steam steam://rungameid/400\n",
    );
    let error = drop.plan(&shortcut).unwrap_err().to_string();
    assert!(error.contains("Settings → Launchers"), "{error}");
}

#[test]
fn a_wine_menu_entry_installs_the_game_inside_the_prefix() {
    let drop = Fixture::new();
    let prefix = drop.path("prefix");
    drop.game("prefix/drive_c/Games/Foo/foo.exe");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Foo\n\
         Exec=env WINEPREFIX=\"{}\" wine C:\\\\\\\\Games\\\\\\\\Foo\\\\\\\\foo.exe\n",
        prefix.display()
    );
    let shortcut = drop.write("home/Foo.desktop", entry.as_bytes());
    let plan = drop.plan(&shortcut).unwrap();
    assert_eq!(plan.input, text(&drop.path("prefix/drive_c/Games/Foo")));
    assert_eq!(plan.launch, launch("foo.exe", &[]));
    assert_eq!(plan.title.as_deref(), Some("Foo"));
}

#[test]
fn a_web_link_installs_its_url() {
    let drop = Fixture::new();
    let shortcut = drop.write(
        "home/Game.url",
        b"[InternetShortcut]\r\nURL=https://example.com/game.zip\r\n",
    );
    let plan = drop.plan(&shortcut).unwrap();
    assert_eq!(plan.kind, DropKind::Shortcut);
    assert_eq!(plan.input, "https://example.com/game.zip");
    assert_eq!(plan.title.as_deref(), Some("Game"));
}

#[test]
fn a_relative_or_missing_path_is_refused() {
    let drop = Fixture::new();
    assert!(drop.plan(Path::new("game.sh")).is_err());
    assert!(drop.plan(&drop.path("missing.sh")).is_err());
    let error = drop
        .plan(Path::new("//host/share/game.exe"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("network share"), "{error}");
}
