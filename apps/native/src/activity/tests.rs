//! Activity's jobs (#101), ported from the webview's `activity`,
//! `JobRecovery` and `RefreshCard` tests over the fixtures' fake job queue
//! and details refresh: jobs show live progress, cancel, remove and retry
//! (as before or another install way); a followed job's question is
//! answered; a failed install says why and recovers; the details refresh's
//! card cancels, retries the failed games and continues the rest.

use crate::frame::tests::{ONE, START, Window, source};
use caskade_core::install::JobSnapshot;
use caskade_core::metadata::RefreshStatus;
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use serde_json::json;

fn fakes() -> (
    &'static caskade_ui_data::jobs::FakeQueue,
    &'static caskade_ui_data::jobs::FakeRefresh,
) {
    let fakes = source().job_fakes.as_ref().expect("the fixtures' fakes");
    (fakes.queue.as_ref(), fakes.refresh.as_ref())
}

fn inbox() -> &'static caskade_ui_data::jobs::requests::FakeInbox {
    let fakes = source().job_fakes.as_ref().expect("the fixtures' fakes");
    fakes.requests.as_ref()
}

/// Empties the fakes for the next test, whatever happened.
struct Clean;

impl Drop for Clean {
    fn drop(&mut self) {
        let (queue, refresh) = fakes();
        queue.clear();
        refresh.clear();
        inbox().clear();
    }
}

fn job(id: &str, value: serde_json::Value) -> JobSnapshot {
    let mut base = json!({
        "id": id,
        "revision": 1,
        "kind": "install",
        "status": "downloading",
        "title": "Celeste",
        "input": "https://example.invalid/celeste.zip",
        "rootId": null,
        "phase": "download",
        "progress": {"bytes": 2048, "totalBytes": 8192, "files": 0, "etaSecs": null, "message": "Downloading celeste.zip"},
        "warnings": [],
        "question": null,
        "gameId": null,
        "error": null,
        "candidates": [],
        "selectedCandidateIndex": null,
        "targetDir": null,
        "dependencies": [],
        "verification": null,
        "failure": null,
        "createdAtMs": 1,
        "updatedAtMs": 1
    });
    for (key, field) in value.as_object().unwrap() {
        base[key] = field.clone();
    }
    serde_json::from_value(base).unwrap()
}

fn activity() -> Window {
    *START.lock().unwrap_or_else(|e| e.into_inner()) = "/activity?tab=jobs";
    let mut window = Window::open();
    *START.lock().unwrap_or_else(|e| e.into_inner()) = "/";
    window.settle_until("Activity", |window| {
        window.find("data-view", "activity").is_some()
    });
    window
}

fn card(window: &Window, title: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "activity-job")
        .into_iter()
        .find(|&card| window.text(card).contains(title))
        .unwrap_or_else(|| panic!("no job {title}"))
}

fn button_in(window: &Window, root: blitz_dom::NodeId, text: &str) -> blitz_dom::NodeId {
    window
        .find_all("data-cs", "button")
        .into_iter()
        .find(|&button| inside(window, button, root) && window.text(button).trim() == text)
        .unwrap_or_else(|| panic!("no {text} button"))
}

fn inside(window: &Window, node: blitz_dom::NodeId, root: blitz_dom::NodeId) -> bool {
    let doc = window.doc.inner.borrow();
    let mut at = Some(node);
    while let Some(here) = at {
        if here == root {
            return true;
        }
        at = doc.get_node(here).and_then(|node| node.parent);
    }
    false
}

fn press(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

#[test]
fn an_install_shows_live_progress_and_cancels() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    let (queue, _) = fakes();
    queue.put(job("j1", json!({})));
    let mut window = activity();
    window.settle_until("the job", |window| {
        window.find("data-cs", "activity-job").is_some()
    });
    let celeste = card(&window, "Celeste");
    assert_eq!(
        window.attr(celeste, "data-status").as_deref(),
        Some("running")
    );
    let text = window.text(celeste);
    assert!(text.contains("Install · Downloading"), "{text}");
    assert!(text.contains("In progress"), "{text}");
    assert!(text.contains("Downloading celeste.zip"), "{text}");
    let bar = window.get("aria-label", "Celeste progress");
    assert_eq!(window.attr(bar, "aria-valuenow").as_deref(), Some("25"));
    // The tab says how many are active.
    let tab = window
        .find_all("role", "tab")
        .into_iter()
        .find(|&tab| {
            window
                .attr(tab, "aria-label")
                .is_some_and(|label| label.starts_with("Jobs"))
        })
        .unwrap();
    assert_eq!(
        window.attr(tab, "aria-label").as_deref(),
        Some("Jobs, 1 active")
    );
    // Live: the queue's next snapshot moves the bar.
    queue.put(job(
        "j1",
        json!({"revision": 2, "progress": {"bytes": 8192, "totalBytes": 8192, "files": 0, "etaSecs": null, "message": "Downloading celeste.zip"}}),
    ));
    window.settle_until("progress", |window| {
        window
            .find("aria-label", "Celeste progress")
            .is_some_and(|bar| window.attr(bar, "aria-valuenow").as_deref() == Some("100"))
    });
    // A pad's A on Cancel job.
    let cancel = button_in(&window, card(&window, "Celeste"), "Cancel job");
    window.focus(cancel);
    window.pad(Action::Confirm);
    window.settle_until("cancelled", |window| {
        fakes().0.calls() == ["cancel j1"]
            && window
                .attr(card(window, "Celeste"), "data-status")
                .as_deref()
                == Some("cancelled")
    });
    // A cancelled install can run again or be removed.
    let celeste = card(&window, "Celeste");
    let retry = button_in(&window, celeste, "Retry");
    press(&mut window, retry);
    window.settle_until("retried", |_| {
        fakes().0.calls().last().map(String::as_str) == Some("retry j1 as before")
    });
    queue.put(job("j1", json!({"revision": 9, "status": "cancelled"})));
    window.settle_until("cancelled again", |window| {
        window
            .attr(card(window, "Celeste"), "data-status")
            .as_deref()
            == Some("cancelled")
    });
    let remove = button_in(&window, card(&window, "Celeste"), "Remove");
    press(&mut window, remove);
    window.settle_until("removed", |window| {
        window.find("data-cs", "activity-job").is_none()
            && window.find("data-part", "empty").is_some()
    });
}

#[test]
fn a_job_waiting_for_an_answer_opens_its_question_and_continues() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    let (queue, _) = fakes();
    queue.put(job(
        "j2",
        json!({
            "status": "needsUser",
            "progress": null,
            "question": {"type": "choose_installer", "value": {"artifacts": ["setup.exe", "launcher.exe"]}}
        }),
    ));
    let mut window = activity();
    window.settle_until("the job", |window| {
        window.find("data-cs", "activity-job").is_some()
    });
    let waiting = card(&window, "Celeste");
    assert!(window.text(waiting).contains("Needs your attention"));
    let review = button_in(&window, waiting, "Review choices");
    press(&mut window, review);
    window.settle_until("the question", |window| {
        window.find("data-cs", "job-question").is_some()
    });
    let dialog = window.get("data-cs", "job-question");
    assert!(
        window
            .text(dialog)
            .contains("Several installers were found in this input.")
    );
    let second = window
        .find_all("role", "radio")
        .into_iter()
        .find(|&radio| {
            inside(&window, radio, dialog) && window.text(radio).contains("launcher.exe")
        })
        .unwrap();
    window.focus(second);
    window.key(Key::Character(" ".into()));
    let go = button_in(&window, dialog, "Continue");
    press(&mut window, go);
    window.settle_until("answered", |window| {
        fakes().0.calls() == ["answer j2 1"] && window.find("data-cs", "job-question").is_none()
    });
}

#[test]
fn a_failed_install_says_why_and_recovers() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    let (queue, _) = fakes();
    queue.put(job(
        "j3",
        json!({
            "status": "failed",
            "progress": null,
            "failure": {
                "kind": "integrity",
                "stage": "download",
                "path": null,
                "transient": false,
                "security": null,
                "summary": "The download didn't match its checksum.",
                "detail": "sha256 mismatch",
                "actions": ["redownload", "cancel"],
                "occurrences": 2,
                "retryAtMs": null
            }
        }),
    ));
    let mut window = activity();
    window.settle_until("the job", |window| {
        window.find("data-part", "failure").is_some()
    });
    let failure = window.get("data-part", "failure");
    let text = window.text(failure);
    assert!(
        text.contains("The download didn't match its checksum."),
        "{text}"
    );
    assert!(
        text.contains("This needs you: choose what to do next."),
        "{text}"
    );
    // A classified failure brings its own buttons, not the plain Retry.
    let celeste = card(&window, "Celeste");
    assert!(
        window
            .find_all("data-cs", "button")
            .into_iter()
            .all(
                |button| !inside(&window, button, celeste) || window.text(button).trim() != "Retry"
            )
    );
    let details = button_in(&window, failure, "Technical details");
    press(&mut window, details);
    window.settle_until("details", |window| {
        window
            .text(window.get("data-part", "failure"))
            .contains("2 times in a row")
    });
    let again = button_in(
        &window,
        window.get("data-part", "failure"),
        "Download again",
    );
    press(&mut window, again);
    window.settle_until("recovered", |_| {
        fakes().0.calls() == ["recover j3 redownload"]
    });
}

fn refresh(running: bool, cancelled: bool, failed: &[&str], remaining: &[&str]) -> RefreshStatus {
    serde_json::from_value(json!({
        "mode": "missing",
        "running": running,
        "total": 10,
        "done": 4,
        "updated": 3,
        "unchanged": 1,
        "unmatched": 0,
        "skipped": 0,
        "failed": failed,
        "cancelled": cancelled,
        "remaining": remaining
    }))
    .unwrap()
}

#[test]
fn the_details_refresh_cancels_retries_and_continues() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    let (_, refresh_fake) = fakes();
    refresh_fake.set(Some(refresh(true, false, &[], &[])));
    let mut window = activity();
    window.settle_until("the card", |window| {
        window.find("data-cs", "games-refresh").is_some()
    });
    let card = window.get("data-cs", "games-refresh");
    assert_eq!(window.attr(card, "data-status").as_deref(), Some("running"));
    let text = window.text(card);
    assert!(
        text.contains("Refreshing game details") && text.contains("Missing details only"),
        "{text}"
    );
    assert!(text.contains("3 updated"), "{text}");
    let bar = window.get("aria-label", "4 of 10 games");
    assert_eq!(window.attr(bar, "aria-valuenow").as_deref(), Some("4"));
    let cancel = button_in(&window, card, "Cancel");
    press(&mut window, cancel);
    window.settle_until("cancel", |_| fakes().1.calls() == ["cancel"]);
    // Cancelled with failures and games left.
    refresh_fake.set(Some(refresh(false, true, &["g1", "g2"], &["g3"])));
    window.settle_until("cancelled", |window| {
        window
            .find("data-cs", "games-refresh")
            .is_some_and(|card| window.attr(card, "data-status").as_deref() == Some("cancelled"))
    });
    let card = window.get("data-cs", "games-refresh");
    let retry = button_in(&window, card, "Retry the 2 failed games");
    press(&mut window, retry);
    window.settle_until("retry", |_| {
        fakes().1.calls().last().map(String::as_str) == Some("start g1,g2 missing")
    });
    let card = window.get("data-cs", "games-refresh");
    let rest = button_in(&window, card, "Continue with the last game");
    press(&mut window, rest);
    window.settle_until("continue", |_| {
        fakes().1.calls().last().map(String::as_str) == Some("start g3 missing")
    });
}

fn request(
    id: &str,
    value: serde_json::Value,
) -> caskade_app::commands::install_requests::InstallRequest {
    let mut base = json!({
        "requestId": id,
        "kind": "url",
        "value": "https://example.invalid/celeste.zip",
        "plan": null,
        "peer": null,
        "share": null
    });
    for (key, field) in value.as_object().unwrap() {
        base[key] = field.clone();
    }
    serde_json::from_value(base).unwrap()
}

#[test]
fn a_deep_link_waits_until_confirmed_and_its_job_shows() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    let mut window = activity();
    inbox().offer(request("link-1", json!({})));
    window.settle_until("the confirmation", |window| {
        window.find("data-cs", "install-confirmation").is_some()
    });
    let dialog = window.get("data-cs", "install-confirmation");
    let text = window.text(dialog);
    assert!(text.contains("Install this game?"), "{text}");
    assert!(
        text.contains("An external link asked Caskade to install from this URL."),
        "{text}"
    );
    assert!(
        text.contains("Nothing will be downloaded until you confirm."),
        "{text}"
    );
    assert_eq!(
        window.text(window.get("data-part", "source")).trim(),
        "https://example.invalid/celeste.zip"
    );
    // Nothing started by showing it.
    assert!(caskade_ui_data::jobs::JobQueue::list(fakes().0).is_empty());
    // A pad's A on Confirm installation.
    let confirm = button_in(&window, dialog, "Confirm installation");
    window.focus(confirm);
    window.pad(Action::Confirm);
    window.settle_until("confirmed", |window| {
        inbox().calls() == ["confirm link-1 default"]
            && window.find("data-cs", "install-confirmation").is_none()
            && window
                .find_all("data-cs", "activity-job")
                .into_iter()
                .any(|card| window.text(card).contains("celeste.zip"))
    });
}

#[test]
fn closing_a_copy_from_another_device_dismisses_it() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    let mut window = activity();
    inbox().offer(request(
        "peer-1",
        json!({
            "kind": "peer",
            "value": "deck",
            "peer": {"deviceId": "d1", "deviceName": "Deck", "entryId": "e1", "gameId": null, "title": "Celeste", "sizeBytes": 2048}
        }),
    ));
    window.settle_until("the confirmation", |window| {
        window.find("data-cs", "install-confirmation").is_some()
    });
    let dialog = window.get("data-cs", "install-confirmation");
    let text = window.text(dialog);
    assert!(text.contains("Copy Celeste from Deck?"), "{text}");
    assert!(
        text.contains("Deck sends Celeste (2.0 kB) to this computer."),
        "{text}"
    );
    button_in(&window, dialog, "Copy here");
    // Escape closes it: a dismissal, never a download.
    window.key(Key::Named(NamedKey::Escape));
    window.settle_until("dismissed", |window| {
        inbox().calls() == ["dismiss peer-1"]
            && window.find("data-cs", "install-confirmation").is_none()
    });
}

#[test]
fn a_file_dropped_outside_the_library_asks_first() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    let mut window = activity();
    let _ = crate::drops::drops().send(crate::drops::DropEvent::Dropped(vec![
        std::path::PathBuf::from("/games/celeste-setup.exe"),
    ]));
    window.settle_until("the confirmation", |window| {
        window.find("data-cs", "install-confirmation").is_some()
    });
    assert_eq!(inbox().calls(), ["dropped /games/celeste-setup.exe"]);
    let dialog = window.get("data-cs", "install-confirmation");
    let text = window.text(dialog);
    assert!(
        text.contains("Caskade works out how to install this file"),
        "{text}"
    );
    assert!(
        window
            .text(window.get("data-part", "plan"))
            .contains("/games/celeste-setup.exe")
    );
    let dismiss = button_in(&window, dialog, "Dismiss");
    press(&mut window, dismiss);
    window.settle_until("dismissed", |window| {
        inbox().calls().last().map(String::as_str) == Some("dismiss request-1")
            && window.find("data-cs", "install-confirmation").is_none()
    });
}

#[test]
fn a_shared_collection_is_added_without_a_job() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    let mut window = activity();
    inbox().offer(request(
        "share-1",
        json!({
            "kind": "share",
            "value": "",
            "share": {
                "sharerId": "s1",
                "sharer": "Sam",
                "kind": "collection",
                "title": "Couch co-op",
                "size": null,
                "windows": false,
                "extras": [],
                "expiresAtMs": null,
                "reach": "direct",
                "contents": {"kind": "collection", "name": "Couch co-op", "games": []}
            }
        }),
    ));
    window.settle_until("the confirmation", |window| {
        window.find("data-cs", "install-confirmation").is_some()
    });
    let dialog = window.get("data-cs", "install-confirmation");
    assert!(window.text(dialog).contains("Receive Couch co-op?"));
    assert!(window.find("data-cs", "share-info").is_some());
    let add = button_in(&window, dialog, "Add to my collections");
    press(&mut window, add);
    window.settle_until("added", |window| {
        inbox().calls() == ["collection share-1"]
            && window.find("data-cs", "install-confirmation").is_none()
    });
    assert!(caskade_ui_data::jobs::JobQueue::list(fakes().0).is_empty());
}
