use super::{
    input_from_args,
    linux::{FILE_NAME, Handler, Mime, desktop_entry, without},
};
use std::{cell::RefCell, collections::HashMap, io, path::Path};

fn args(arg: &str) -> Vec<String> {
    vec!["caskade".into(), arg.into()]
}

#[test]
fn the_os_starts_caskade_with_one_magnet_link_or_torrent_file() {
    let cwd = Path::new("/home/me/Downloads");
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Game";
    assert_eq!(input_from_args(&args(magnet), cwd).as_deref(), Some(magnet));
    assert_eq!(
        input_from_args(&args("game.TORRENT"), cwd).as_deref(),
        Some("/home/me/Downloads/game.TORRENT")
    );
    assert_eq!(
        input_from_args(&args("file:///tmp/My%20Game.torrent"), cwd).as_deref(),
        Some("/tmp/My Game.torrent")
    );
    let remote = "https://example.com/files/game.torrent?token=1";
    assert_eq!(input_from_args(&args(remote), cwd).as_deref(), Some(remote));
    // Anything else is a normal start.
    for other in [
        "--console",
        "/tmp/game.zip",
        "https://example.com/game.zip",
        "ftp://example.com/game.torrent",
    ] {
        assert_eq!(input_from_args(&args(other), cwd), None, "{other}");
    }
    let two = vec!["caskade".into(), magnet.into(), "--console".into()];
    assert_eq!(input_from_args(&two, cwd), None);
    assert_eq!(input_from_args(&["caskade".into()], cwd), None);
}

#[test]
fn the_entry_quotes_the_program_for_exec() {
    let entry = desktop_entry("/opt/My Games/$caskade%", &[]).unwrap();
    assert!(
        entry.contains("Exec=\"/opt/My Games/\\\\$caskade%%\" %u\n"),
        "{entry}"
    );
    assert!(entry.contains("MimeType=x-scheme-handler/magnet;application/x-bittorrent;\n"));
    assert!(desktop_entry("/opt/bad\npath", &[]).is_err());
}

#[test]
fn removing_caskade_from_mimeapps_keeps_everything_else() {
    let list = "# mine\n[Default Applications]\nx-scheme-handler/magnet=caskade-torrent-links.desktop\ntext/plain=caskade-torrent-links.desktop\n[Added Associations]\napplication/x-bittorrent=qbittorrent.desktop;caskade-torrent-links.desktop;\n";
    assert_eq!(
        without(list, FILE_NAME).unwrap(),
        "# mine\n[Default Applications]\ntext/plain=caskade-torrent-links.desktop\n[Added Associations]\napplication/x-bittorrent=qbittorrent.desktop;\n"
    );
    assert_eq!(without("[Default Applications]\n", FILE_NAME), None);
}

/// The default apps, in memory.
#[derive(Default)]
struct FakeMime(RefCell<HashMap<String, String>>);

impl Mime for FakeMime {
    fn default_app(&self, mime: &str) -> Option<String> {
        self.0.borrow().get(mime).cloned()
    }
    fn set_default(&self, file: &str, mime: &str) -> io::Result<()> {
        self.0.borrow_mut().insert(mime.into(), file.into());
        Ok(())
    }
    fn refresh(&self, _: &Path) {}
}

fn handler(dir: &Path) -> Handler<FakeMime> {
    Handler {
        applications: dir.join("applications"),
        mimeapps: dir.join("mimeapps.list"),
        program: "/opt/caskade".into(),
        mime: FakeMime::default(),
    }
}

#[test]
fn caskade_takes_the_links_and_hands_them_back_to_the_app_before() {
    let dir = tempfile::tempdir().unwrap();
    let handler = handler(dir.path());
    handler
        .mime
        .set_default("qbittorrent.desktop", "x-scheme-handler/magnet")
        .unwrap();
    let before = handler.status().unwrap();
    assert!(!before.registered && !before.magnet && !before.torrent_files);

    handler.register().unwrap();
    // Twice: the second time must still remember qBittorrent.
    handler.register().unwrap();
    let status = handler.status().unwrap();
    assert!(status.registered && status.magnet && status.torrent_files);
    let entry = std::fs::read_to_string(dir.path().join("applications").join(FILE_NAME)).unwrap();
    assert!(entry.contains("X-Caskade-Previous-Magnet=qbittorrent.desktop\n"));
    assert!(!entry.contains("X-Caskade-Previous-Torrent"));

    std::fs::write(
        dir.path().join("mimeapps.list"),
        "[Default Applications]\napplication/x-bittorrent=caskade-torrent-links.desktop\n",
    )
    .unwrap();
    handler.unregister().unwrap();
    let after = handler.status().unwrap();
    assert!(!after.registered && !after.magnet);
    assert_eq!(
        handler
            .mime
            .default_app("x-scheme-handler/magnet")
            .as_deref(),
        Some("qbittorrent.desktop")
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mimeapps.list")).unwrap(),
        "[Default Applications]\n"
    );
}

#[test]
fn a_moved_caskade_rewrites_only_an_existing_entry() {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("applications").join(FILE_NAME);
    let mut handler = handler(dir.path());
    handler.refresh().unwrap();
    assert!(!entry.exists());

    handler
        .mime
        .set_default("transmission.desktop", "application/x-bittorrent")
        .unwrap();
    handler.register().unwrap();
    handler.program = "/home/me/Caskade.AppImage".into();
    handler.refresh().unwrap();
    let text = std::fs::read_to_string(&entry).unwrap();
    assert!(
        text.contains("Exec=/home/me/Caskade.AppImage %u\n"),
        "{text}"
    );
    assert!(text.contains("X-Caskade-Previous-Torrent=transmission.desktop\n"));
}
