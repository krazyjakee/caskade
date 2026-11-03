//! Who may call which command (#205). [`access`] gives every registered
//! command its permission group, and [`gate`] checks it against the
//! signed-in account before the command runs, so the rules hold with the
//! webview bypassed. A refusal is the core's `Error::Forbidden` text.
//!
//! Reads are never checked. Changes need someone signed in; changes to a
//! permission group's state need that group (`caskade_core::accounts::
//! Permission`). The app's own background work doesn't go through
//! commands, so whoever is signed in never blocks it. A command missing
//! from the table is for admins only, and a test fails until it is added.

use caskade_core::{
    accounts::{AccountService, Permission},
    install::{InstallService, JobKind},
};
use tauri::{
    Manager, Runtime,
    ipc::{Invoke, InvokeBody},
};

/// What calling a command needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Reads, and the window's own state (pickers, console mode): always
    /// allowed, signed in or not, because the UI needs them to render.
    Open,
    /// Changes every account shares (play, favourites, game updates):
    /// someone signed in.
    SignedIn,
    /// The accounts commands, whose rules the core checks itself; signing
    /// in needs nobody signed in.
    Accounts,
    /// A change to a Settings section's state.
    Section(&'static str),
    /// `settings_set` / `settings_reset`: the section of the `namespace`
    /// argument (`Permission::for_namespace`).
    Settings,
    /// Bringing games in, uninstalling and moving them.
    Install,
    /// Answering or retrying the job `id`: an update's or verify's needs
    /// someone signed in, any other install.
    Job,
    /// Changing the lifecycle scripts of the `scope` argument
    /// ([`script_scope`]).
    Scripts,
    /// Admins only: backups and restores (#193), which hold and replace
    /// every account's data.
    Admin,
}

/// Every registered command's [`Access`]; `None` for one not classified
/// yet, which [`gate`] leaves to admins.
pub fn access(command: &str) -> Option<Access> {
    use Access::*;
    Some(match command {
        // Reads.
        "background_status"
        | "discovery_status"
        | "discovery_launchers"
        | "list_games"
        | "get_game"
        | "game_page_images"
        | "library_stats"
        | "library_platforms"
        | "title_letters"
        | "list_sessions"
        | "hidden_games"
        | "list_collections"
        | "get_collection"
        | "collection_games"
        | "game_collections"
        | "pending_install_requests"
        | "jobs_list"
        | "job_get"
        | "running_games"
        | "storage_list"
        | "settings_sections"
        | "settings_get"
        | "emulation_catalog"
        | "emulation_overview"
        | "emulators_list"
        | "profile_preview"
        | "args_split"
        | "emulation_platforms"
        | "emulation_detect_get"
        | "emulation_detect_latest"
        | "emulation_game"
        | "emulator_setup_installable"
        | "emulator_setup_missing"
        | "emulator_setup_jobs"
        | "plugins_list"
        | "plugins_ui"
        | "plugin_messages"
        | "rom_scanners_list"
        | "rom_scan_get"
        | "rom_scan_jobs"
        | "rom_exclusions"
        | "igdb_games"
        | "igdb_refresh"
        | "igdb_search"
        | "igdb_catalog_search"
        | "igdb_catalog_recent"
        | "igdb_catalog_upcoming"
        | "igdb_catalog_filters"
        | "igdb_catalog_games"
        | "igdb_owned"
        | "game_actions"
        | "lifecycle_scripts"
        | "lifecycle_scripts_effective"
        | "game_achievements"
        | "game_achievement_summary"
        | "game_achievement_links"
        | "game_achievement_icon"
        | "game_age_ratings"
        | "game_age_check"
        | "game_igdb"
        | "game_edit_state"
        | "game_saves_state"
        // A game's save versions (#241) and the save manifest's state.
        | "game_save_history"
        | "saves_manifest_status"
        | "saves_manifest_update"
        // Which games run on which paired device (#240).
        | "devices_playing"
        // Add-ons on the user's other devices (#245), offered to install.
        | "addons_from_devices"
        // Emulators the user's other devices prefer (#275), offered.
        | "emulators_from_devices"
        // The add-on catalogue (#181): reading it, and fetching it again.
        | "addons_catalogue"
        | "addons_catalogue_refresh"
        | "addons_media"
        | "addons_records"
        | "game_artwork"
        | "library_game_ids"
        | "manual_game_target"
        | "manual_game_duplicates"
        | "games_refresh_status"
        | "game_updates"
        | "updates_available"
        | "romm_overview"
        | "romm_cache_status"
        | "romm_game_status"
        | "themes_list"
        | "theme_stylesheet"
        | "theme_resolve"
        | "appearance_safe_mode"
        | "themes_reload"
        | "input_devices"
        | "console_initial_mode"
        | "locale_current"
        | "browse_dir"
        | "app_update_status"
        | "torrent_links_status" => Open,
        // The notification feed and its read state (#198): one feed every
        // account shares, and marking it read changes nothing else.
        "notifications_list"
        | "notifications_unread"
        | "notification_members"
        | "notification_mark_read"
        | "notification_mark_unread"
        | "notifications_mark_all_read" => Open,
        // The window's own state and pickers that only answer a path.
        "hello"
        | "choose_install_files"
        | "choose_install_folder"
        | "storage_choose_directory"
        | "set_console_mode"
        | "guide_return_to_game"
        | "plugin_view_opened"
        // Settings → Statistics only reads the machine (#156).
        | "system_stats_watch"
        | "system_stats_unwatch" => Open,
        // Shared by every account. Collections check their owner in the core.
        "set_game_hidden"
        | "set_game_favorite"
        | "set_game_default_entry"
        | "reveal_game_entry"
        | "save_collection"
        | "set_collection_pinned"
        | "delete_collection"
        | "add_to_collection"
        | "remove_from_collection"
        | "dismiss_install_request"
        | "job_cancel"
        | "verify_game"
        | "launch_game"
        | "launch_game_with"
        // Saves before a launch and a conflict's choice (#239): they write
        // the player's own save files, like playing does.
        | "game_saves_before_play"
        | "game_saves_choose"
        // Restoring an older save (#241) and showing where saves live.
        | "game_saves_restore"
        | "game_saves_reveal"
        | "game_tool_run"
        // A trailer starts the video window, a process of its own.
        | "video_play"
        | "launch_cancel"
        | "lifecycle_script_test_cancel"
        | "game_achievements_refresh"
        | "game_achievements_cancel"
        | "game_achievement_link_set"
        | "game_achievement_link_clear"
        | "game_age_approve"
        | "guide_stop_game"
        | "plugin_request"
        | "game_igdb_set"
        | "game_igdb_rematch"
        | "game_igdb_refresh"
        | "game_separate_entry"
        // Game edits (#171): the library's details, like the IGDB match.
        | "game_edit"
        | "game_saves_edit"
        | "game_artwork_set"
        | "games_bulk_edit"
        | "games_refresh_start"
        | "games_refresh_cancel"
        | "romm_cancel_download"
        | "app_update_check"
        | "app_update_restart" => SignedIn,
        // Updates of installed games run for everyone, so Play still works.
        "game_update_check"
        | "game_update_before_play"
        | "game_update_skip"
        | "game_update_prereleases"
        | "game_update_install" => SignedIn,
        "accounts_list"
        | "account_create"
        | "account_update"
        | "account_delete"
        | "account_sign_in"
        | "account_sign_out"
        | "account_set_pin"
        | "account_verify_admin_pin" => Accounts,
        "settings_set" | "settings_reset" => Settings,
        "app_update_install" => Section("general"),
        // Whether the OS opens magnet links and torrent files with Caskade.
        "torrent_links_set" | "torrent_links_choose" => Section("installer"),
        "discovery_scan" | "discovery_cancel" | "discovery_review" => Section("library-discovery"),
        // Emulation, RomM and Get emulators.
        "emulator_save"
        | "emulator_delete"
        | "platform_settings_set"
        | "emulation_detect_start"
        | "emulation_detect_cancel"
        | "emulation_detect_import"
        | "emulation_game_set"
        | "emulator_setup_install"
        | "emulator_setup_install_for"
        | "emulator_offer_keep"
        | "emulator_setup_cancel"
        | "emulator_setup_check"
        | "emulator_setup_skip"
        | "emulator_setup_update"
        | "emulator_setup_fetch_bios"
        | "rom_scanner_save"
        | "rom_scanner_delete"
        | "rom_folder_setup"
        | "rom_exclusions_set"
        | "romm_settings_set"
        | "romm_server_save"
        | "romm_server_remove"
        | "romm_server_test"
        | "romm_cache_clear" => Section("emulators"),
        "theme_load_unpacked" => Section("appearance"),
        "storage_add" | "storage_edit" | "storage_default" | "storage_remove" => Section("storage"),
        "plugin_enable"
        | "plugin_disable"
        | "plugin_install_preview"
        | "plugin_install"
        | "plugin_uninstall"
        | "plugin_load_unpacked" => Section("plugins"),
        // Catalogue add-ons (#182): plugins and themes, installed and
        // removed as Settings → Plugins does.
        "addons_install"
        | "addons_install_cancel"
        | "addons_uninstall"
        | "addons_skip_update"
        | "addons_set_update_preference" => Section("plugins"),
        // Install requests (paste, drops; deep links check in
        // `deep_link::open`), installs, ROM scans and RomM imports and
        // downloads, games added by hand, uninstalls, removals and moves.
        "install_start"
        | "fetch_start"
        | "request_install_input"
        | "request_dropped_install"
        | "confirm_install_request"
        | "uninstall_game"
        | "remove_game"
        | "move_game"
        | "rom_scan_start"
        | "rom_scan_cancel"
        | "rom_scan_submit"
        | "rom_scan_all"
        | "romm_sync"
        | "romm_set_pinned"
        | "romm_evict"
        | "add_web_game"
        | "save_web_game_offline"
        | "remove_web_game"
        | "remove_web_offline"
        // What a game runs: adding games by hand, in effect (#177).
        | "game_action_save"
        | "game_action_delete"
        | "game_actions_order"
        // Installing an owned game through its launcher (#170).
        | "owned_install"
        // Copying a game from another of the user's devices (#233).
        | "game_copy_here"
        // Sending one there: it's confirmed on that device (#235).
        | "game_send_to"
        | "game_action_settings_save"
        // Adding a game by hand: in effect what an install adds (#174).
        | "manual_game_add" => Install,
        "job_answer" | "job_retry" | "job_recover" => Job,
        "backup_status" | "backup_plan" => Open,
        // Game streaming (#257): its state is a read; hosting installs a
        // program and changes the system with an administrator prompt.
        "streaming_host_status"
        | "streaming_hosts"
        | "streaming_pair_requests"
        | "streaming_paired_devices" => Open,
        // Pairing this device with another, or forgetting one, is anyone's.
        "streaming_pair_with" | "streaming_forget_host" => SignedIn,
        // Who may stream from this device is an admin's choice.
        "streaming_accept_pairing" | "streaming_decline_pairing" | "streaming_unpair" => Admin,
        // Remembering a device to stream from changes what everyone sees.
        "streaming_add_host" => SignedIn,
        "streaming_set_hosting" | "streaming_use_existing" => Admin,
        // Its upkeep (#266): what's installed is anyone's to see.
        "streaming_programs" => Open,
        "streaming_repair"
        | "streaming_update"
        | "streaming_skip"
        | "streaming_unpair_all" => Admin,
        // Paired devices (#226): reading them needs an account; every change
        // is an admin's (pairing hands another device the library).
        "devices_status"
        | "game_other_devices"
        // Playing a game on another device (#234): that device's own
        // account and limits decide there.
        | "game_play_on"
        | "remote_play_finished"
        | "devices_transfers"
        | "devices_opened_link"
        | "devices_dismiss_opened_link"
        | "devices_sync_now" => SignedIn,
        "devices_rename"
        | "devices_set_kind"
        | "devices_pair_nearby"
        | "devices_confirm_pairing"
        | "devices_cancel_pairing"
        | "devices_pairing_link"
        | "devices_accept_pairing_link"
        | "devices_accept_opened_link"
        | "devices_forget"
        | "devices_create_identity"
        | "devices_retry_identity" => Admin,
        // Store accounts (#164): reads, a refresh for whoever is signed
        // in, and the account changes for admins.
        "store_providers"
        | "store_accounts_list"
        | "store_account_removal_preview"
        | "game_ownership"
        | "game_playtime"
        | "game_owned_releases"
        | "store_auth_flows" => Open,
        "store_account_refresh" | "store_account_cancel_refresh" | "owned_install_cancel" => {
            SignedIn
        }
        "store_account_disconnect"
        | "store_account_remove"
        | "store_connect_begin"
        | "store_connect_complete"
        | "store_connect_cancel" => Admin,
        "backup_start"
        | "backup_cancel"
        | "backup_inspect"
        | "backup_restore"
        | "backup_restore_discard"
        | "backup_restart"
        | "backup_reveal"
        // Backups kept on other devices (#243): listing and fetching one
        // to restore, as restoring is.
        | "backups_on_devices"
        | "backup_fetch_from_device" => Admin,
        // A test runs the script: whoever may change the scope.
        "lifecycle_script_save"
        | "lifecycle_script_delete"
        | "lifecycle_scripts_order"
        | "lifecycle_script_test" => Scripts,
        _ => return None,
    })
}

/// Wraps the app's invoke handler: a command the signed-in account may not
/// call is answered with the reason and never runs.
pub fn gate<R, F>(handler: F) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static
where
    R: Runtime,
    F: Fn(Invoke<R>) -> bool + Send + Sync + 'static,
{
    move |invoke: Invoke<R>| match check(&invoke) {
        Ok(()) => handler(invoke),
        Err(error) => {
            // A structured refusal, like every command's error (#160).
            invoke.resolver.reject(crate::error::CommandError::new(
                crate::error::ErrorCode::Forbidden,
                error,
            ));
            true
        }
    }
}

fn check<R: Runtime>(invoke: &Invoke<R>) -> Result<(), String> {
    let message = &invoke.message;
    let webview = message.webview_ref();
    let permission = match access(message.command()) {
        Some(Access::Open | Access::Accounts) => return Ok(()),
        Some(Access::SignedIn) => return signed_in(webview),
        Some(Access::Section(section)) => Permission::Settings(section),
        Some(Access::Settings) => {
            Permission::for_namespace(argument(message.payload(), "namespace").unwrap_or(""))
        }
        Some(Access::Install) => Permission::Install,
        Some(Access::Admin) => Permission::Admin,
        Some(Access::Scripts) => script_scope(message.payload()),
        Some(Access::Job) => {
            let kind = argument(message.payload(), "id")
                .and_then(|id| Some(webview.try_state::<InstallService>()?.job(id)?.kind));
            match kind {
                Some(JobKind::Update | JobKind::Verify) => return signed_in(webview),
                _ => Permission::Install,
            }
        }
        None => Permission::Admin,
    };
    require(webview, permission)
}

/// Whether the signed-in account may do what needs `permission`; the
/// reason, for people, when not.
pub fn require<R: Runtime>(app: &impl Manager<R>, permission: Permission) -> Result<(), String> {
    accounts(app)?
        .require(permission)
        .map_err(|error| error.to_string())
}

/// Whether someone is signed in; the reason, for people, when not.
pub(crate) fn signed_in<R: Runtime>(app: &impl Manager<R>) -> Result<(), String> {
    accounts(app)?
        .signed_in()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn accounts<R: Runtime>(app: &impl Manager<R>) -> Result<tauri::State<'_, AccountService>, String> {
    app.try_state::<AccountService>()
        .ok_or_else(|| caskade_core::tr!("accounts are unavailable"))
}

/// What changing a lifecycle script scope needs (#179): a game's, what
/// adding games needs (it changes what the game runs); an emulator
/// profile's, the Emulators section; the global scripts, which run with
/// every game, an admin.
pub fn script_scope(body: &InvokeBody) -> Permission {
    let kind = match body {
        InvokeBody::Json(value) => value
            .get("scope")
            .and_then(|scope| scope.get("type"))
            .and_then(|kind| kind.as_str()),
        InvokeBody::Raw(_) => None,
    };
    match kind {
        Some("game") => Permission::Install,
        Some("profile") => Permission::Settings("emulators"),
        _ => Permission::Admin,
    }
}

/// A string argument of the call, by its (camelCase) name.
fn argument<'a>(body: &'a InvokeBody, name: &str) -> Option<&'a str> {
    match body {
        InvokeBody::Json(value) => value.get(name)?.as_str(),
        InvokeBody::Raw(_) => None,
    }
}

#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;
