//! Shared ingress for file drops, DOM paste/link drops and deep links.
//! Merely requesting never resolves a URL, downloads bytes or executes input.
pub use app::*;
use caskade_app::commands::install_requests as app;
// The webview's events, which the app sends as `caskade_app`'s.
pub use crate::events::{InstallRefused, InstallRequested};
use caskade_core::install::{InstallService, JobSnapshot};
use tauri::Runtime;

#[tauri::command]
#[specta::specta]
pub async fn request_install_input<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, InstallRequests>,
    value: String,
) -> Result<InstallRequest, String> {
    // A pasted share link (#246) asks its sharer first.
    if caskade_share::is_link(&value) {
        return super::shares::queue_share_link(&app, value).await;
    }
    app::request_install_input(&crate::events::sink(app.clone()), &state, value).await
}

#[tauri::command]
#[specta::specta]
pub fn pending_install_requests(state: tauri::State<'_, InstallRequests>) -> Vec<InstallRequest> {
    app::pending_install_requests(&state)
}

#[tauri::command]
#[specta::specta]
pub fn dismiss_install_request(
    state: tauri::State<'_, InstallRequests>,
    request_id: String,
) -> Result<(), String> {
    app::dismiss_install_request(&state, request_id)
}

#[tauri::command]
#[specta::specta]
pub async fn confirm_install_request(
    state: tauri::State<'_, InstallRequests>,
    service: tauri::State<'_, InstallService>,
    request_id: String,
    root_id: Option<String>,
) -> Result<JobSnapshot, String> {
    app::confirm_install_request(&state, &service, request_id, root_id).await
}

/// A file or folder dropped on the library, planned first: a shortcut
/// stands for its target and a game's executable for its folder
/// (`caskade_core::install::dropped`). It then waits for confirmation like
/// any other request. Drops elsewhere use `request_install_input`.
#[tauri::command]
#[specta::specta]
pub async fn request_dropped_install<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, InstallRequests>,
    path: String,
) -> Result<InstallRequest, String> {
    app::request_dropped_install(&crate::events::sink(app.clone()), &state, path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_ipc::invoke;
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};
    use tauri::{
        Listener,
        test::{mock_builder, mock_context, noop_assets},
    };
    use tauri_specta::Event;
    #[test]
    fn drops_and_pastes_only_queue_and_failed_confirmation_remains_pending() {
        let store =
            caskade_app::runtime::block_on(caskade_core::db::LibraryStore::in_memory()).unwrap();
        let service = InstallService::new(
            store,
            caskade_core::bus::Bus::default(),
            caskade_core::install::InstallOptions::default(),
        );
        let queue = InstallRequests::default();
        let specta = tauri_specta::Builder::<tauri::test::MockRuntime>::new()
            .dangerously_cast_bigints_to_number()
            .commands(tauri_specta::collect_commands![
                request_install_input::<tauri::test::MockRuntime>,
                request_dropped_install::<tauri::test::MockRuntime>,
                pending_install_requests,
                confirm_install_request,
                dismiss_install_request
            ])
            .events(tauri_specta::collect_events![InstallRequested]);
        let app = mock_builder()
            .manage(service.clone())
            .manage(queue.clone())
            .invoke_handler(specta.invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        specta.mount_events(&app);
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
        let events = seen.clone();
        app.listen(InstallRequested::NAME, move |event| {
            events
                .lock()
                .unwrap()
                .push(serde_json::from_str(event.payload()).unwrap())
        });
        let request = invoke(
            &window,
            "request_install_input",
            json!({"value":"https://example.invalid/game.zip"}),
        )
        .unwrap();
        assert_eq!(request["kind"], "url");
        assert!(service.jobs().is_empty());
        let id = request["requestId"].as_str().unwrap();
        assert!(
            invoke(
                &window,
                "confirm_install_request",
                json!({"requestId":id,"rootId":"missing"})
            )
            .is_err()
        );
        assert_eq!(queue.pending().len(), 1);
        assert!(service.jobs().is_empty());
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("Café game.zip");
        std::fs::write(&file, b"not executed").unwrap();
        let dropped = invoke(
            &window,
            "request_dropped_install",
            json!({"path": file.to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(dropped["plan"]["kind"], "file");
        assert_eq!(dropped["value"], dropped["plan"]["input"]);
        assert_eq!(queue.pending().len(), 2);
        assert!(service.jobs().is_empty());
        assert_eq!(seen.lock().unwrap().len(), 2);
        invoke(&window, "dismiss_install_request", json!({"requestId":id})).unwrap();
        assert_eq!(
            invoke(&window, "pending_install_requests", json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            invoke(
                &window,
                "confirm_install_request",
                json!({"requestId":id,"rootId":null})
            )
            .is_err()
        );
    }
}
