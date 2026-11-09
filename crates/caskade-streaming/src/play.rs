//! Streaming a game from another device (#264):
//! [`Streams`] is the launch pipeline's [`StreamLauncher`]. Before the
//! client starts, Caskade asks the host itself (over HTTPS, as the paired
//! client) whether it still knows this device, what it plays and which
//! games it offers, wakes it when it sleeps, and stops another game once
//! Play asked. Moonlight then only streams, full screen; its errors (it
//! exits 0 either way, after a dialog) are read from stderr and shown in
//! Caskade's words. When the stream ends, the game stops on the host.

use super::{
    client::{SETTINGS_DIR, SETTINGS_FILE, StreamingClient},
    gamestream::{self, HostLink},
    hosts::{HTTP_PORT, PairedHost, StreamHosts, ask_server_info, paired_hosts},
    pairing::{Endpoint, Identity},
};
use async_trait::async_trait;
use caskade_core::{
    launch::{ClientLine, HostBusy, LaunchError, StreamLauncher, StreamStart, WindowPlacement},
    settings::{SettingsService, StreamingSettings},
};
use std::{
    collections::HashMap,
    ffi::OsString,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

/// The host's HTTPS port.
pub const HTTPS_PORT: u16 = 47984;

/// How long things may take (shorter in tests).
#[derive(Debug, Clone, Copy)]
pub struct StreamTiming {
    /// How long a woken host may take to answer.
    pub wake: Duration,
    /// How often a host is asked again while waking or quitting.
    pub poll: Duration,
    /// How long the stream may take to come up.
    pub ready: Duration,
    /// How long errors still count as not starting once the host answered.
    pub settle: Duration,
    /// How long the client gets to quit.
    pub stop_grace: Duration,
}

impl Default for StreamTiming {
    fn default() -> Self {
        Self {
            wake: Duration::from_secs(45),
            poll: Duration::from_secs(1),
            ready: Duration::from_secs(60),
            settle: Duration::from_secs(3),
            stop_grace: Duration::from_secs(5),
        }
    }
}

/// A stream being played: what `read` and `finish` need.
#[derive(Clone)]
struct Playing {
    device: String,
    link: HostLink,
    app_id: String,
}

struct Inner {
    client: StreamingClient,
    hosts: Option<StreamHosts>,
    settings: SettingsService,
    timing: StreamTiming,
    ports: (u16, u16),
    /// Where wake-up messages go instead of the network (tests).
    wake_to: Option<Vec<std::net::SocketAddr>>,
    /// Where Caskade's window is: the client's own window opens, one
    /// pixel small, in its middle, under it.
    placement: Option<Placement>,
    activation: Option<Activation>,
    playing: Mutex<HashMap<(String, String), Playing>>,
}

/// Streams games from paired devices through Caskade's client.
#[derive(Clone)]
pub struct Streams {
    inner: Arc<Inner>,
}

fn refused(text: String) -> LaunchError {
    LaunchError::Refused(text)
}

fn not_paired(device: &str) -> LaunchError {
    refused(caskade_core::tr!(
        "{device} no longer knows this device. Pair them again in Settings → Streaming.",
        device = device
    ))
}

fn not_offered(device: &str, game: &str) -> LaunchError {
    refused(caskade_core::tr!(
        "{device} no longer offers {game}.",
        device = device,
        game = game
    ))
}

fn launch_error(error: caskade_core::Error) -> LaunchError {
    refused(error.to_string())
}

impl Streams {
    pub fn new(
        client: StreamingClient,
        hosts: Option<StreamHosts>,
        settings: SettingsService,
    ) -> Self {
        Self::with(
            client,
            hosts,
            settings,
            StreamTiming::default(),
            (HTTP_PORT, HTTPS_PORT),
        )
    }

    /// Other timing and ports (`(http, https)`; tests).
    pub fn with(
        client: StreamingClient,
        hosts: Option<StreamHosts>,
        settings: SettingsService,
        timing: StreamTiming,
        ports: (u16, u16),
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                hosts,
                settings,
                timing,
                ports,
                wake_to: None,
                placement: None,
                activation: None,
                playing: Mutex::default(),
            }),
        }
    }

    /// What lets the client's window take focus from Caskade's: without it
    /// GNOME says "<the client> is ready" in a notification (#27).
    pub fn with_activation(
        self,
        activation: impl Fn() -> Vec<(String, String)> + Send + Sync + 'static,
    ) -> Self {
        let inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("with_activation before the service is shared"));
        Self {
            inner: Arc::new(Inner {
                activation: Some(Arc::new(activation)),
                ..inner
            }),
        }
    }

    /// The client's window opens behind Caskade's, at `placement` (#264).
    pub fn with_placement(
        self,
        placement: impl Fn() -> Option<WindowPlacement> + Send + Sync + 'static,
    ) -> Self {
        let inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("with_placement before the service is shared"));
        Self {
            inner: Arc::new(Inner {
                placement: Some(Arc::new(placement)),
                ..inner
            }),
        }
    }

    /// Sends wake-up messages to `targets` only (tests).
    pub fn with_wake_targets(self, targets: Vec<std::net::SocketAddr>) -> Self {
        let inner = Arc::try_unwrap(self.inner)
            .unwrap_or_else(|_| panic!("with_wake_targets before the service is shared"));
        Self {
            inner: Arc::new(Inner {
                wake_to: Some(targets),
                ..inner
            }),
        }
    }

    fn endpoint(&self, address: &str) -> caskade_core::Result<Endpoint> {
        let host = match address.parse::<IpAddr>() {
            Ok(IpAddr::V6(ip)) => format!("[{ip}]"),
            _ => address.to_owned(),
        };
        let (http, https) = self.inner.ports;
        let invalid = |error: url::ParseError| caskade_core::Error::Invalid(error.to_string());
        Ok(Endpoint {
            http: format!("http://{host}:{http}/").parse().map_err(invalid)?,
            https: format!("https://{host}:{https}/")
                .parse()
                .map_err(invalid)?,
        })
    }

    /// Where the host answers now: where it was last found, then each
    /// address it was paired or reached at. Wakes it when none answers
    /// and its MAC is known.
    async fn reach(&self, record: &PairedHost, device: &str) -> Result<String, LaunchError> {
        let mut addresses: Vec<String> = self
            .inner
            .hosts
            .as_ref()
            .and_then(|hosts| {
                hosts
                    .hosts()
                    .into_iter()
                    .find(|host| host.id.eq_ignore_ascii_case(&record.id))?
                    .address
            })
            .into_iter()
            .collect();
        for address in &record.addresses {
            if !addresses.contains(address) {
                addresses.push(address.clone());
            }
        }
        if let Some(address) = self.answering(&addresses).await {
            return Ok(address);
        }
        let Some(mac) = record.mac else {
            return Err(refused(caskade_core::tr!(
                "{device} isn't answering. Check that it's switched on and on this network.",
                device = device
            )));
        };
        let ips: Vec<IpAddr> = addresses.iter().filter_map(|a| a.parse().ok()).collect();
        let targets = self
            .inner
            .wake_to
            .clone()
            .unwrap_or_else(|| gamestream::wake_targets(&ips));
        let deadline = Instant::now() + self.inner.timing.wake;
        while Instant::now() < deadline {
            if let Err(error) = gamestream::wake(mac, &targets).await {
                tracing::debug!(%error, "wake-up not sent");
            }
            tokio::time::sleep(self.inner.timing.poll).await;
            if let Some(address) = self.answering(&addresses).await {
                return Ok(address);
            }
        }
        Err(refused(caskade_core::tr!(
            "{device} didn't wake up. Check that it's plugged in and on this network.",
            device = device
        )))
    }

    async fn answering(&self, addresses: &[String]) -> Option<String> {
        for address in addresses {
            if ask_server_info(address, self.inner.ports.0).await.is_some() {
                return Some(address.clone());
            }
        }
        None
    }

    fn device_name(&self, record: Option<&PairedHost>, host: &str) -> String {
        self.inner
            .hosts
            .as_ref()
            .and_then(|hosts| {
                hosts
                    .hosts()
                    .into_iter()
                    .find(|known| known.id.eq_ignore_ascii_case(host))
                    .map(|known| known.name)
            })
            .or_else(|| record.map(|record| record.name.clone()))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| caskade_core::tr!("The other device"))
    }

    /// The client's arguments for streaming `app` from `address`.
    async fn args(&self, address: &str, app: &str) -> Vec<OsString> {
        let settings: StreamingSettings =
            self.inner.settings.read_or_default("core.streaming").await;
        let mut args: Vec<OsString> = vec![
            "stream".into(),
            address.into(),
            app.into(),
            "--display-mode".into(),
            // Windows' exclusive full screen stays over Caskade's guide menu.
            if cfg!(windows) {
                "borderless"
            } else {
                "fullscreen"
            }
            .into(),
            // Caskade stops the game itself, without the client's
            // quitting window.
            "--no-quit-after".into(),
        ];
        let quality = settings.quality;
        if let Some(resolution) = quality.resolution.filter(|r| valid_resolution(r)) {
            args.extend(["--resolution".into(), resolution.into()]);
        }
        if let Some(fps) = quality.fps.filter(|fps| (10..=240).contains(fps)) {
            args.extend(["--fps".into(), fps.to_string().into()]);
        }
        if let Some(bitrate) = quality.bitrate_kbps.filter(|b| (500..=500_000).contains(b)) {
            args.extend(["--bitrate".into(), bitrate.to_string().into()]);
        }
        // The client shows a window of its own until the stream is up: one
        // pixel, in the middle of Caskade's, which covers it meanwhile.
        if let Some(at) = self
            .inner
            .placement
            .as_ref()
            .and_then(|placement| placement())
        {
            args.extend(["-qwindowgeometry".into(), start_window(&at).into()]);
        }
        args
    }
}

type Placement = Arc<dyn Fn() -> Option<WindowPlacement> + Send + Sync>;
type Activation = Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>;

/// The client's start window: one pixel in the middle of `caskade`'s.
/// X geometry counts a negative offset from the far edge, so a middle left
/// of or above the main screen gives only the size.
pub fn start_window(caskade: &WindowPlacement) -> String {
    let x = i64::from(caskade.x) + i64::from(caskade.width / 2);
    let y = i64::from(caskade.y) + i64::from(caskade.height / 2);
    if x < 0 || y < 0 {
        return "1x1".into();
    }
    format!("1x1+{x}+{y}")
}

/// `WIDTHxHEIGHT`, each 100–99999.
fn valid_resolution(text: &str) -> bool {
    text.split_once('x').is_some_and(|(width, height)| {
        [width, height].iter().all(|part| {
            part.parse::<u32>()
                .is_ok_and(|n| (100..=99_999).contains(&n))
        })
    })
}

/// The error to show for a line the client wrote, in Caskade's words.
pub fn client_failure(line: &str, device: &str, game: &str) -> Option<String> {
    let at = line
        .find("Qt Critical:")
        .or_else(|| line.find("Qt Fatal:"))?;
    let text = &line[at..];
    Some(if text.contains("Failed to find application") {
        caskade_core::tr!(
            "{device} no longer offers {game}.",
            device = device,
            game = game
        )
    } else if text.contains("has not been paired") {
        caskade_core::tr!(
            "{device} no longer knows this device. Pair them again in Settings → Streaming.",
            device = device
        )
    } else if text.contains("Failed to start the specified application") {
        caskade_core::tr!(
            "{device} couldn't start {game}.",
            device = device,
            game = game
        )
    } else if text.contains("wasn't started by this PC") {
        caskade_core::tr!(
            "{device} is playing a game another device started. Stop it there first.",
            device = device
        )
    } else {
        caskade_core::tr!("Streaming from {device} failed.", device = device)
    })
}

/// Whether the client's line says the stream is starting: the host
/// answered its launch or resume.
pub fn client_streaming(line: &str) -> bool {
    line.contains("Launch response:") || line.contains("Resume response:")
}

#[async_trait]
impl StreamLauncher for Streams {
    async fn prepare(
        &self,
        host: &str,
        app: &str,
        stop_host: bool,
    ) -> Result<StreamStart, LaunchError> {
        // One client at a time: it writes its state when it quits. Pairing
        // or a refresh of a host's games may have it for a moment.
        let hold = tokio::time::timeout(
            Duration::from_secs(30),
            self.inner.client.busy.clone().lock_owned(),
        )
        .await
        .map_err(|_| {
            refused(caskade_core::tr!(
                "Streaming is busy with another device. Try again in a moment."
            ))
        })?;
        let client = self
            .inner
            .client
            .ensure(&CancellationToken::new())
            .await
            .map_err(launch_error)?;
        let settings = std::fs::read_to_string(client.state.join(SETTINGS_DIR).join(SETTINGS_FILE))
            .unwrap_or_default();
        let record = paired_hosts(&settings)
            .into_iter()
            .find(|record| record.id.eq_ignore_ascii_case(host));
        let device = self.device_name(record.as_ref(), host);
        let (Some(record), Some(identity)) = (record, Identity::from_settings(&settings)) else {
            return Err(not_paired(&device));
        };
        let Some(cert) = record.server_cert_pem.clone() else {
            return Err(not_paired(&device));
        };
        let address = self.reach(&record, &device).await?;
        let link = HostLink::new(
            self.endpoint(&address).map_err(launch_error)?,
            &identity,
            &cert,
        )
        .map_err(launch_error)?;
        let unreachable = |error: caskade_core::Error| {
            tracing::warn!(%error, "the streaming device stopped answering");
            refused(caskade_core::tr!(
                "{device} stopped answering. Try again in a moment.",
                device = device
            ))
        };
        let state = link.state().await.map_err(unreachable)?;
        if !state.paired {
            return Err(not_paired(&device));
        }
        let apps = link.apps().await.map_err(unreachable)?;
        let wanted = apps
            .iter()
            .find(|offered| offered.name == app)
            .or_else(|| {
                apps.iter()
                    .find(|offered| offered.name.eq_ignore_ascii_case(app))
            })
            .cloned()
            .ok_or_else(|| not_offered(&device, app))?;
        if let Some(playing) = state.playing.filter(|playing| *playing != wanted.id) {
            if !stop_host {
                return Err(LaunchError::HostBusy(Box::new(HostBusy {
                    entry_id: String::new(),
                    device,
                    playing: apps
                        .iter()
                        .find(|offered| offered.id == playing)
                        .map(|offered| offered.name.clone()),
                })));
            }
            link.quit().await.map_err(|_| {
                refused(caskade_core::tr!(
                    "{device} didn't stop its game. Try again in a moment.",
                    device = device
                ))
            })?;
            let deadline = Instant::now() + self.inner.timing.wake;
            while link
                .state()
                .await
                .is_ok_and(|state| state.playing.is_some())
            {
                if Instant::now() >= deadline {
                    return Err(refused(caskade_core::tr!(
                        "{device} didn't stop its game. Try again in a moment.",
                        device = device
                    )));
                }
                tokio::time::sleep(self.inner.timing.poll).await;
            }
        }
        let args = self.args(&address, &wanted.name).await;
        let mut command = self
            .inner
            .client
            .command(&client, &args)
            .map_err(launch_error)?;
        if let Some(activation) = &self.inner.activation {
            command.envs(activation());
        }
        caskade_core::lock(&self.inner.playing).insert(
            (host.to_owned(), app.to_owned()),
            Playing {
                device,
                link,
                app_id: wanted.id,
            },
        );
        Ok(StreamStart {
            command,
            hold: Some(Box::new(hold)),
        })
    }

    fn read(&self, host: &str, app: &str, line: &str) -> ClientLine {
        if client_streaming(line) {
            return ClientLine::Streaming;
        }
        let device = caskade_core::lock(&self.inner.playing)
            .get(&(host.to_owned(), app.to_owned()))
            .map(|playing| playing.device.clone())
            .unwrap_or_else(|| caskade_core::tr!("The other device"));
        match client_failure(line, &device, app) {
            Some(text) => {
                tracing::warn!(line, "the streaming client failed");
                ClientLine::Failed(text)
            }
            None => ClientLine::Other,
        }
    }

    async fn finish(&self, host: &str, app: &str) {
        let Some(playing) =
            caskade_core::lock(&self.inner.playing).remove(&(host.to_owned(), app.to_owned()))
        else {
            return;
        };
        // Only this stream's game, and only while it runs.
        let state = tokio::time::timeout(Duration::from_secs(10), playing.link.state()).await;
        if let Ok(Ok(state)) = state
            && state.playing.as_deref() == Some(playing.app_id.as_str())
            && let Err(error) = playing.link.quit().await
        {
            tracing::warn!(%error, "the game didn't stop on the streaming device");
        }
    }

    fn ready_within(&self) -> Duration {
        self.inner.timing.ready
    }

    fn settle(&self) -> Duration {
        self.inner.timing.settle
    }

    fn stop_grace(&self) -> Duration {
        self.inner.timing.stop_grace
    }
}

#[cfg(all(test, unix))]
mod tests;
