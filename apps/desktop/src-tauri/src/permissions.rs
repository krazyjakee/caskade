//! The permission gate (#205): every command the webview invokes is checked
//! against the signed-in account before it runs (`caskade_app::permissions`
//! holds the rules), so they hold with the webview bypassed.

#[cfg(test)]
use caskade_app::permissions::access;
use caskade_core::{
    accounts::{AccountService, Permission},
    install::InstallService,
};
use tauri::{
    Manager, Runtime,
    ipc::{Invoke, InvokeBody},
};

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
    let args = match message.payload() {
        InvokeBody::Json(value) => value,
        InvokeBody::Raw(_) => &serde_json::Value::Null,
    };
    caskade_app::permissions::check(
        message.command(),
        args,
        webview.try_state::<AccountService>().as_deref(),
        webview.try_state::<InstallService>().as_deref(),
    )
}

/// Whether the signed-in account may do what needs `permission`; the
/// reason, for people, when not.
pub fn require<R: Runtime>(app: &impl Manager<R>, permission: Permission) -> Result<(), String> {
    caskade_app::permissions::require(app.try_state::<AccountService>().as_deref(), permission)
}

#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;
