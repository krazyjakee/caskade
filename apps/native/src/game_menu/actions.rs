//! What a game's menu, its quick look and its card do (#99), through the
//! app layer's commands as the webview's hooks call them: start and stop a
//! game, favourite and hide it, show its files, copy its path, uninstall
//! and remove it. Each runs on the app's runtime; its app event then
//! refreshes what shows it. Without the app's services (fixtures mode) the
//! ones that need them say they aren't available.

use caskade_app::commands::launch::LaunchOutcome;
use caskade_core::error::ErrorReport;
use caskade_core::install::JobSnapshot;
use caskade_core::launch::LaunchChoice;

type Services = &'static caskade_app::start::Services;

fn unavailable() -> ErrorReport {
    ErrorReport::from(caskade_ui_i18n::t!(
        "That isn't available in this copy of Caskade."
    ))
}

fn services() -> Result<Services, ErrorReport> {
    crate::shell::services().ok_or_else(unavailable)
}

fn failed(error: String) -> ErrorReport {
    ErrorReport::from(error)
}

/// Starts entry `entry_id`, with `choice` when a launch asked for one: the
/// launch may ask (an emulator's choices, an age limit, a busy host).
pub async fn launch(
    entry_id: String,
    choice: Option<LaunchChoice>,
) -> Result<LaunchOutcome, ErrorReport> {
    let services = services()?;
    crate::data::change(async move {
        caskade_app::commands::launch::launch_game_with(&services.launch, &entry_id, choice).await
    })
    .await
}

/// Runs entry `entry_id`'s tool `action_id` once: no play session. An age
/// limit holding it back is a failure here until the age gate (#106) and
/// #108's `ToolRequest` ask for an admin's PIN.
pub async fn run_tool(entry_id: String, action_id: String) -> Result<(), ErrorReport> {
    use caskade_app::commands::actions::ToolOutcome;
    let services = services()?;
    let outcome = crate::data::change(async move {
        caskade_app::commands::actions::game_tool_run(&services.launch, &entry_id, &action_id).await
    })
    .await?;
    match outcome {
        ToolOutcome::Started => Ok(()),
        ToolOutcome::AgeRestricted { .. } => {
            Err(ErrorReport::from(caskade_ui_i18n::t!("Admin PIN needed")))
        }
    }
}

/// Stops the running session `session_id`, keeping its playtime so far.
pub async fn stop(session_id: String) -> Result<(), ErrorReport> {
    let services = services()?;
    crate::data::change(async move {
        caskade_app::commands::guide::guide_stop_game(session_id, &services.launch)
            .await
            .map(drop)
            .map_err(failed)
    })
    .await
}

/// Opens the folder that holds entry `entry_id`'s files.
pub async fn reveal(entry_id: String) -> Result<(), ErrorReport> {
    let services = services()?;
    crate::data::change(async move {
        caskade_app::commands::library::reveal_game_entry(&services.store, &entry_id).await
    })
    .await
}

/// Puts entry `entry_id`'s path on the clipboard.
pub async fn copy_path(entry_id: String) -> Result<(), ErrorReport> {
    let services = services()?;
    crate::data::change(async move {
        let path =
            caskade_app::commands::library::game_entry_path(&services.store, &entry_id).await?;
        caskade_app::commands::clipboard::clipboard_write_text(path).map_err(failed)
    })
    .await
}

/// Uninstalls entry `entry_id`: a job in Activity.
pub async fn uninstall(entry_id: String) -> Result<JobSnapshot, ErrorReport> {
    let services = services()?;
    let install = services.install.as_ref().ok_or_else(unavailable)?;
    crate::data::change(async move {
        caskade_app::commands::install::uninstall_game(install, &services.launch, entry_id)
            .await
            .map_err(failed)
    })
    .await
}

/// Removes game `game_id` from the library for good, uninstalling its files
/// first: the uninstall's job, when there is one.
pub async fn remove(game_id: String) -> Result<Option<JobSnapshot>, ErrorReport> {
    let services = services()?;
    let install = services.install.as_ref().ok_or_else(unavailable)?;
    crate::data::change(async move {
        caskade_app::commands::install::remove_game(install, &services.launch, game_id).await
    })
    .await
}
