//! The path browser (console mode's picker), over a folder of its own.

use super::*;

use crate::flows::paths::{PathPickerProvider, PathRequest, PickKind, use_path_picker};

/// What the picker was asked, and what it answered (`Some(None)`: cancelled).
static REQUEST: Mutex<Option<PathRequest>> = Mutex::new(None);
static ANSWER: Mutex<Option<Option<String>>> = Mutex::new(None);

fn picker_app() -> Element {
    rsx! {
        style { {include_str!("../../../assets/app.css")} }
        InputRoot {
            PathPickerProvider { in_app: true, Asker {} }
        }
    }
}

#[component]
fn Asker() -> Element {
    let picker = use_path_picker();
    use_hook(move || {
        let request = caskade_core::lock(&REQUEST).clone();
        spawn(async move {
            if let Some(request) = request {
                let answer = picker.pick(request).await;
                *caskade_core::lock(&ANSWER) = Some(answer);
            }
        });
    });
    rsx! { div { id: "asker" } }
}

/// `root/{alpha/, beta/, game.sh, notes.txt}`.
fn tree() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("alpha")).unwrap();
    std::fs::create_dir(root.path().join("beta")).unwrap();
    std::fs::write(root.path().join("game.sh"), "#!/bin/sh\n").unwrap();
    std::fs::write(root.path().join("notes.txt"), "notes").unwrap();
    root
}

fn browse(request: PathRequest) -> Window {
    *caskade_core::lock(&REQUEST) = Some(request);
    *caskade_core::lock(&ANSWER) = None;
    let mut window = Window::open_root(picker_app, "the browser", "[data-cs=folder-browser]");
    window.until("the listing", |w| w.count("[data-part=entry]") > 0);
    window
}

fn answer() -> Option<Option<String>> {
    caskade_core::lock(&ANSWER).clone()
}

fn path(of: &std::path::Path) -> String {
    of.to_string_lossy().into_owned()
}

#[test]
fn the_browser_opens_folders_goes_up_and_picks_the_folder_shown() {
    let _one = caskade_core::lock(&ONE);
    let root = tree();
    let mut request = PathRequest::new(PickKind::Folder, "Choose a library folder".into());
    request.start_at = Some(path(root.path()));
    let mut window = browse(request);
    assert_eq!(
        window
            .attr("[data-cs=folder-browser]", "data-kind")
            .as_deref(),
        Some("folder")
    );
    assert_eq!(
        window.text("[data-cs=folder-browser] h2"),
        "Choose a library folder"
    );
    assert_eq!(window.text("[data-part=location]"), path(root.path()));
    // Folders first; files can't be picked in folder mode.
    let names = window.texts("[data-part=entry] [data-part=name]");
    assert_eq!(names, ["alpha", "beta", "game.sh", "notes.txt"]);
    assert_eq!(window.count("[data-part=entry][data-disabled]"), 2);
    // Focus lands on the first folder; Enter opens it.
    assert_eq!(
        window
            .attr("[data-part=entry][data-focus-default]", "data-kind")
            .as_deref(),
        Some("folder")
    );
    window.focus("[data-part=entry][data-focus-default]");
    window.key(Key::Named(NamedKey::Enter));
    let alpha = path(&root.path().join("alpha"));
    window.until("alpha", |w| w.text("[data-part=location]") == alpha);
    assert_eq!(window.text("[data-part=empty]"), "This folder is empty.");
    // Up lands back on the folder just left.
    window.focus("[data-part=up]");
    window.key(Key::Named(NamedKey::Enter));
    window.until("the root again", |w| {
        w.text("[data-part=location]") == path(root.path())
    });
    assert_eq!(
        window.text("[data-part=entry][data-focus-default] [data-part=name]"),
        "alpha"
    );
    window.press("#folder-select");
    window.until("an answer", |_| answer().is_some());
    assert_eq!(answer(), Some(Some(path(root.path()))));
    assert!(
        window.find("[data-cs=folder-browser]").is_none(),
        "it closed"
    );
}

#[test]
fn in_file_mode_only_files_it_takes_can_be_picked() {
    let _one = caskade_core::lock(&ONE);
    let root = tree();
    let mut request = PathRequest::new(PickKind::File, "The game's program or shortcut".into());
    request.start_at = Some(path(root.path()));
    request.extensions = vec!["sh".into()];
    let mut window = browse(request);
    assert_eq!(
        window
            .attr("[data-cs=folder-browser]", "data-kind")
            .as_deref(),
        Some("file")
    );
    assert!(
        window.find("#folder-select").is_none(),
        "no Select this folder"
    );
    assert_eq!(
        window.count("[data-part=entry][data-disabled]"),
        1,
        "notes.txt"
    );
    let game = path(&root.path().join("game.sh"));
    window.focus("[data-part=entry][data-kind=file]:not([data-disabled])");
    window.key(Key::Named(NamedKey::Enter));
    window.until("an answer", |_| answer().is_some());
    assert_eq!(answer(), Some(Some(game)));
}

#[test]
fn a_pad_picks_the_folder_with_start_and_cancels_with_b() {
    use caskade_ui_input::Action;
    let _one = caskade_core::lock(&ONE);
    let root = tree();
    let mut request = PathRequest::new(PickKind::Folder, "Choose a library folder".into());
    request.start_at = Some(path(&root.path().join("beta")));
    let mut window = browse_empty(request.clone());
    window.focus("[data-part=up]");
    window.pad(Action::Menu);
    window.until("an answer", |_| answer().is_some());
    assert_eq!(answer(), Some(Some(path(&root.path().join("beta")))));

    request.start_at = Some(path(root.path()));
    let mut window = browse(request);
    window.focus("[data-part=entry][data-focus-default]");
    window.pad(Action::Back);
    window.until("an answer", |_| answer().is_some());
    assert_eq!(answer(), Some(None), "cancelled");
}

/// As [`browse`], for a folder with nothing in it.
fn browse_empty(request: PathRequest) -> Window {
    *caskade_core::lock(&REQUEST) = Some(request);
    *caskade_core::lock(&ANSWER) = None;
    let mut window = Window::open_root(picker_app, "the browser", "[data-cs=folder-browser]");
    window.until("the listing", |w| w.find("[data-part=empty]").is_some());
    window
}

#[test]
fn places_find_the_longest_match() {
    use crate::flows::folder_browser::current_place;
    use caskade_core::browse::Place;
    let places = [
        Place {
            label: "Home".into(),
            path: "/home/me".into(),
        },
        Place {
            label: "/".into(),
            path: "/".into(),
        },
        Place {
            label: "Games".into(),
            path: "/home/me/games/".into(),
        },
    ];
    assert_eq!(current_place(&places, "/home/me/games/x"), Some(2));
    assert_eq!(current_place(&places, "/home/me/docs"), Some(0));
    assert_eq!(current_place(&places, "/etc"), Some(1));
    assert_eq!(current_place(&places[..1], "/etc"), None);
}
