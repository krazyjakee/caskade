//! The running Caskade's end of `caskade --launch` (#256): each connection
//! is one launch through the pipeline, as Play would start it, held until
//! its session ends or the caller goes.

use super::{Ending, Reply, Request, channel, client_env, read_request};
use caskade_core::{
    accounts::AccountService,
    db::LibraryStore,
    launch::{LaunchChoice, LaunchError, LaunchHandle, LaunchService, SessionStatus},
    model::{GameEntry, InstallState, LaunchSpec, PlaySession},
    saves::versions::SaveVersions,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

/// How long Stop gives a game to quit before ending it, as the quick
/// menu's Stop does.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// How often Stop is tried again while the game's processes aren't known.
const STOP_RETRY: Duration = Duration::from_millis(250);

/// What a launch from another program goes through: the shell's services
/// once the library is open.
#[derive(Clone)]
pub struct Services {
    pub accounts: AccountService,
    pub store: LibraryStore,
    pub launches: LaunchService,
    /// A newer save from another device goes in place first, when there.
    pub saves: Option<SaveVersions>,
}

impl Services {
    /// A started app's.
    pub fn of(started: &crate::start::Services) -> Self {
        Self {
            accounts: started.accounts.clone(),
            store: started.store.clone(),
            launches: started.launch.clone(),
            saves: Some(started.save_versions.clone()),
        }
    }
}

/// Takes launches at `endpoint` for as long as the app runs.
pub fn listen(services: Services, endpoint: channel::Endpoint) {
    crate::runtime::spawn(async move {
        let mut listener = match channel::Listener::bind(&endpoint).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "launches from other programs unavailable");
                return;
            }
        };
        loop {
            match listener.accept().await {
                Ok(stream) => {
                    let services = services.clone();
                    crate::runtime::spawn(async move { handle(&services, stream).await });
                }
                Err(error) => {
                    tracing::warn!(%error, "a launch from another program wasn't taken");
                    tokio::time::sleep(STOP_RETRY).await;
                }
            }
        }
    });
}

/// One caller: its request, the launch, and the session's end.
pub async fn handle(services: &Services, stream: channel::Stream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let request = match read_request(&mut reader).await {
        Ok(request) => request,
        Err(message) => {
            send(&mut write, &Reply::Failed { message }).await;
            return;
        }
    };
    let (launches, title, handle) = match start(services, &request).await {
        Ok(started) => started,
        Err(reply) => {
            if let Reply::Refused { message } | Reply::Failed { message } = &reply {
                tracing::info!(game = %request.game_id, %message, "launch from another program not started");
            }
            send(&mut write, &reply).await;
            return;
        }
    };
    let session_id = handle.ticket.session_id.clone();
    tracing::info!(game = %request.game_id, session = %session_id, "launch from another program started");
    send(
        &mut write,
        &Reply::Started {
            session_id: session_id.clone(),
            title,
        },
    )
    .await;
    let ended = handle.wait_status();
    tokio::pin!(ended);
    tokio::select! {
        result = &mut ended => {
            let how = ending(&result, launches.take_stopped(&session_id));
            send(&mut write, &Reply::Ended { how }).await;
        }
        () = closed(&mut reader) => {
            tracing::info!(session = %session_id, "the program that launched the game went; stopping it");
            loop {
                if launches.stop(&session_id, STOP_GRACE).await.is_ok() {
                    break;
                }
                // Its processes aren't recorded yet: try again until they
                // are, or the session ends by itself.
                tokio::select! {
                    _ = &mut ended => break,
                    () = tokio::time::sleep(STOP_RETRY) => {}
                }
            }
            launches.take_stopped(&session_id);
        }
    }
}

/// Resolves once the caller has closed its end.
async fn closed<R: tokio::io::AsyncRead + Unpin>(reader: &mut BufReader<R>) {
    let mut buffer = [0_u8; 256];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            // Callers send nothing more; whatever comes is ignored.
            Ok(_) => {}
        }
    }
}

fn ending(
    result: &caskade_core::launch::Result<(PlaySession, SessionStatus)>,
    stopped: bool,
) -> Ending {
    match result {
        _ if stopped => Ending::Stopped,
        Ok((_, SessionStatus::Exited)) => Ending::Exited,
        _ => Ending::Crashed,
    }
}

async fn send<W: tokio::io::AsyncWrite + Unpin>(write: &mut W, reply: &Reply) {
    let Ok(mut line) = serde_json::to_vec(reply) else {
        return;
    };
    line.push(b'\n');
    // A caller that went hears nothing; its session goes on as Play's do.
    let _ = write.write_all(&line).await;
    let _ = write.flush().await;
}

fn refused(message: String) -> Reply {
    Reply::Refused { message }
}

/// Starts the request's launch as Play would: someone signed in, the
/// release Play would pick unless the caller named one, the play gate.
async fn start(
    services: &Services,
    request: &Request,
) -> Result<(LaunchService, String, LaunchHandle), Reply> {
    crate::permissions::signed_in(Some(&services.accounts)).map_err(refused)?;
    let (store, launches) = (&services.store, &services.launches);
    let failed = |error: caskade_core::Error| Reply::Failed {
        message: error.to_string(),
    };
    let game = store
        .game(&request.game_id)
        .await
        .map_err(failed)?
        .ok_or_else(|| {
            refused(caskade_core::tr!(
                "no game {id} in the library",
                id = request.game_id.clone()
            ))
        })?;
    let entries = store.entries_for_game(&game.id).await.map_err(failed)?;
    let entry = match &request.entry_id {
        Some(id) => entries
            .iter()
            .find(|entry| &entry.id == id)
            .ok_or_else(|| {
                refused(caskade_core::tr!(
                    "{game} has no release {id}",
                    game = game.title.clone(),
                    id = id.clone()
                ))
            })?,
        None => playable(&entries, game.default_entry.as_deref()).ok_or_else(|| {
            refused(caskade_core::tr!(
                "{game} has no release to play on this device",
                game = game.title.clone()
            ))
        })?,
    };
    let choice = LaunchChoice {
        action: request.action_id.clone(),
        ..Default::default()
    };
    let env = client_env(request.env.clone());
    // A newer save from another device goes in place first, as Play does
    // (#239). Nobody may be there to answer which save to use, so both
    // devices' saves stay as they are until Play asks; a failed check never
    // stops the launch.
    if let Some(versions) = &services.saves
        && let Err(error) = versions.before_play(&entry.id).await
    {
        tracing::warn!(%error, "the game's saves weren't checked before the launch");
    }
    match launches.launch_with_env(&entry.id, &choice, &env).await {
        Ok(handle) => Ok((launches.clone(), game.title, handle)),
        // Nobody may be at the device to type an admin PIN: refuse plainly.
        Err(LaunchError::AgeRestricted(restriction)) => Err(refused(restriction.to_string())),
        Err(LaunchError::NeedsChoice(_)) => Err(refused(caskade_core::tr!(
            "{game} asks what to start; choose in Caskade, or name an action with --action",
            game = game.title
        ))),
        Err(error @ (LaunchError::AlreadyRunning | LaunchError::Cancelled)) => {
            Err(refused(error.to_string()))
        }
        Err(LaunchError::Refused(message)) => Err(refused(message)),
        Err(error) => Err(Reply::Failed {
            message: error.to_string(),
        }),
    }
}

/// The release Play starts: the game's default when it can launch, else the
/// first that can (an installed release Caskade itself starts).
fn playable<'a>(entries: &'a [GameEntry], preferred: Option<&str>) -> Option<&'a GameEntry> {
    let launchable = |entry: &&GameEntry| {
        entry.state == InstallState::Installed
            && entry
                .launch
                .as_ref()
                .is_some_and(|launch| !matches!(launch, LaunchSpec::Plugin { .. }))
    };
    entries
        .iter()
        .filter(launchable)
        .find(|entry| Some(entry.id.as_str()) == preferred)
        .or_else(|| entries.iter().find(launchable))
}
