//! Thin settings IPC contract; validation/persistence/events live in core.
//! Forwarded to `caskade_app::commands::settings`.
use crate::error::CommandError;
use caskade_app::commands::settings as forwarded;
use caskade_core::settings::{SettingsSection, SettingsService, SettingsSnapshot};
use tauri::Manager;

/// Every settings section. A plugin's (`plugins.<id>`) comes with its
/// titles and descriptions from the plugin's own catalog (#219).
#[tauri::command]
#[specta::specta]
pub fn settings_sections<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, SettingsService>,
) -> Result<Vec<SettingsSection>, CommandError> {
    forwarded::settings_sections(
        &state,
        app.try_state::<caskade_plugin_host::PluginHost>()
            .as_deref(),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn settings_get(
    state: tauri::State<'_, SettingsService>,
    namespace: String,
) -> Result<SettingsSnapshot, CommandError> {
    forwarded::settings_get(&state, &namespace).await
}

#[tauri::command]
#[specta::specta]
pub async fn settings_set(
    state: tauri::State<'_, SettingsService>,
    namespace: String,
    value: serde_json::Value,
    expected_revision: Option<u32>,
) -> Result<SettingsSnapshot, CommandError> {
    forwarded::settings_set(&state, &namespace, value, expected_revision).await
}

#[tauri::command]
#[specta::specta]
pub async fn settings_reset(
    state: tauri::State<'_, SettingsService>,
    namespace: String,
    expected_revision: Option<u32>,
) -> Result<SettingsSnapshot, CommandError> {
    forwarded::settings_reset(&state, &namespace, expected_revision).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_ipc::{assert_report, invoke};
    use serde_json::json;
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn commands_validate_persist_reset_and_reject_stale_revision_through_ipc() {
        let store =
            tauri::async_runtime::block_on(caskade_core::db::LibraryStore::in_memory()).unwrap();
        let service = SettingsService::new(store.clone());
        service.register(caskade_core::settings::SectionDefinition::new("test.preferences", 1,
            json!({"type":"object","properties":{"enabled":{"type":"boolean"}},"required":["enabled"],"additionalProperties":false}),
            json!({"enabled":true})).unwrap()).unwrap();
        let app = mock_builder()
            .manage(service)
            .invoke_handler(tauri::generate_handler![
                settings_sections,
                settings_get,
                settings_set,
                settings_reset
            ])
            .build(mock_context(noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        assert_eq!(
            invoke(&webview, "settings_sections", json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let before = invoke(
            &webview,
            "settings_get",
            json!({"namespace":"test.preferences"}),
        )
        .unwrap();
        assert_eq!(before["revision"], 0);
        let changed = invoke(
            &webview,
            "settings_set",
            json!({"namespace":"test.preferences","value":{"enabled":false},"expectedRevision":0}),
        )
        .unwrap();
        assert_eq!(changed["value"]["enabled"], false);
        assert_eq!(changed["revision"], 1);
        let stale = invoke(
            &webview,
            "settings_set",
            json!({"namespace":"test.preferences","value":{"enabled":true},"expectedRevision":0}),
        )
        .unwrap_err();
        assert_report(&stale, "conflict", &["test.preferences"]);
        let wrong = invoke(&webview, "settings_set", json!({"namespace":"test.preferences","value":{"enabled":"wrong"},"expectedRevision":1})).unwrap_err();
        assert_report(&wrong, "invalid", &["test.preferences", "/enabled"]);
        assert!(wrong["detail"].as_str().unwrap().contains("/enabled"));
        let absent =
            invoke(&webview, "settings_get", json!({"namespace":"core.saves"})).unwrap_err();
        assert_report(&absent, "unavailable", &["core.saves", "section"]);
        assert_eq!(
            invoke(
                &webview,
                "settings_reset",
                json!({"namespace":"test.preferences","expectedRevision":1})
            )
            .unwrap()["value"]["enabled"],
            true
        );
        tauri::async_runtime::block_on(store.close());
    }
}
