//! Calls a command through Tauri's mock IPC, as the webview would. Shared by
//! the unit tests and the integration tests (`tests/main.rs` includes it).

use serde_json::Value;
use tauri::{
    test::{INVOKE_KEY, MockRuntime, get_ipc_response},
    webview::InvokeRequest,
};

/// Asserts that a command's `error` is a report of `code` whose summary
/// names none of `internal` (ids, namespaces, type names).
#[allow(dead_code)]
pub fn assert_report(error: &Value, code: &str, internal: &[&str]) {
    assert_eq!(error["code"], code, "{error}");
    let summary = error["summary"].as_str().expect("a report's summary");
    for name in internal {
        assert!(!summary.contains(name), "{summary} names {name}");
    }
}

/// The command's JSON reply, or its error.
pub fn invoke(
    window: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, Value> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
    )
    .map(|reply| reply.deserialize().unwrap())
}
