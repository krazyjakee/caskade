//! Capabilities (#124, TECH-PLAN §7): a backend gets only what its manifest
//! declares and the user granted. Network access is an allowlist checked on
//! every `wasi:http` request (redirects are new requests, so they are
//! checked too), folders are WASI preopens, and library and launch access
//! are checked by the host functions and the hook adapter.

use crate::{
    manifest::{LibraryAccess, Manifest, Permissions, folder_mode},
    outbound,
};
use caskade_installer::fetch::grant::{entry_parts, host_matches, names_ip, public_ip};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use wasmtime_wasi_http::{Error, RequestOptions, Result as HttpResult, WasiBody, WasiHttpHooks};

/// Whether `host` matches an allowlist entry: the exact host, or any
/// subdomain of `*.domain`. Case-insensitive. An entry's port doesn't
/// matter here; the installer's downloads follow the same rule.
pub use caskade_installer::fetch::grant::host_allowed;
pub use outbound::{Resolve, system_resolve};

/// Whether an entry allows `port`: the port it names, or else 80 and 443.
fn port_allowed(entry: &str, port: u16) -> bool {
    match entry_parts(entry) {
        Some((_, Some(named))) => named == port,
        Some((_, None)) => port == 80 || port == 443,
        None => false,
    }
}

/// Whether a backend may send a request to `host` on `port` (CSK-03): an
/// entry must cover the host, and name the port unless it is 80 or 443.
pub fn request_allowed(allowlist: &[String], host: &str, port: u16) -> bool {
    allowlist
        .iter()
        .any(|entry| host_matches(entry, host) && port_allowed(entry, port))
}

/// Whether a backend's request may connect to `address`, once its host
/// has resolved (CSK-03): a public address, or a loopback, private,
/// link-local or unspecified one only when an entry names it itself
/// (`127.0.0.1:8384`, `[::1]`, `192.168.1.20`, or `localhost` for
/// loopback) and allows the port. A granted name that resolves to the LAN
/// (DNS rebinding included) is never connected to.
pub fn address_allowed(allowlist: &[String], address: SocketAddr) -> bool {
    public_ip(address.ip())
        || allowlist
            .iter()
            .any(|entry| names_ip(entry, address.ip()) && port_allowed(entry, address.port()))
}

/// A folder the backend may open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderGrant {
    pub path: PathBuf,
    pub read_only: bool,
}

/// Resolves `fs` permissions such as `$HOME/ROMs:ro` against `home`.
/// Entries that aren't absolute after expansion, or don't exist, are
/// skipped with a warning.
pub fn folder_grants(entries: &[String], home: Option<&Path>) -> Vec<FolderGrant> {
    let mut grants = Vec::new();
    for entry in entries {
        let Some((path, read_only)) = folder_mode(entry) else {
            tracing::warn!(entry, "plugin folder permission skipped: not :ro or :rw");
            continue;
        };
        let expanded = match home {
            Some(home) => {
                let home = home.to_string_lossy();
                if let Some(rest) = path.strip_prefix('~') {
                    format!("{home}{rest}")
                } else {
                    path.replace("${HOME}", &home).replace("$HOME", &home)
                }
            }
            None => path.to_owned(),
        };
        let path = PathBuf::from(expanded);
        if !path.is_absolute() || !path.is_dir() {
            tracing::warn!(
                entry,
                "plugin folder permission skipped: not an existing absolute folder"
            );
            continue;
        }
        grants.push(FolderGrant { path, read_only });
    }
    grants
}

/// Whether `granted` covers everything `wanted` asks for. A plugin whose
/// manifest asks for more than was granted waits for the user again.
pub fn covers(granted: &Permissions, wanted: &Permissions) -> bool {
    let level = |access: LibraryAccess| match access {
        LibraryAccess::None => 0,
        LibraryAccess::Read => 1,
        LibraryAccess::Write => 2,
    };
    wanted
        .network
        .iter()
        .all(|host| granted.network.contains(host))
        && wanted.fs.iter().all(|folder| granted.fs.contains(folder))
        && (!wanted.process || granted.process)
        && level(wanted.library) <= level(granted.library)
}

/// What the user granted a plugin, as stored with its state: the
/// permissions, and whether its manifest had a UI half then. A UI half runs
/// with the app's full access, so one that appears later needs consent
/// again (CSK-05). Stored as the permissions object plus a `ui` key; grants
/// from before that have no `ui` key and granted no UI half.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Grant {
    pub permissions: Permissions,
    pub ui: bool,
}

impl Grant {
    /// What enabling `manifest` grants.
    pub fn of(manifest: &Manifest) -> Self {
        Self {
            permissions: manifest.permissions.clone(),
            ui: manifest.ui.is_some(),
        }
    }

    /// Reads a stored grant; `None` when it is unreadable.
    pub fn from_value(mut value: serde_json::Value) -> Option<Self> {
        let ui = value
            .as_object_mut()
            .and_then(|object| object.remove("ui"))
            .and_then(|ui| ui.as_bool())
            .unwrap_or(false);
        let permissions = serde_json::from_value(value).ok()?;
        Some(Self { permissions, ui })
    }

    pub fn to_value(&self) -> serde_json::Result<serde_json::Value> {
        let mut value = serde_json::to_value(&self.permissions)?;
        if let Some(object) = value.as_object_mut() {
            object.insert("ui".into(), self.ui.into());
        }
        Ok(value)
    }

    /// Whether this grant covers everything `manifest` asks for, its UI
    /// half included.
    pub fn covers(&self, manifest: &Manifest) -> bool {
        covers(&self.permissions, &manifest.permissions) && (self.ui || manifest.ui.is_none())
    }
}

/// The fields of a launch command only `process` plugins may change.
/// `emulation` carries the profile's `before`/`after_exit` commands, which run
/// as programs too.
const PROCESS_FIELDS: [&str; 5] = ["program", "args", "env", "cwd", "emulation"];

/// The fields of a discovered game that decide what runs: its launch
/// targets, runtime and uninstaller, the folder relative programs resolve
/// against, and the tracking hints whose matches "Stop game" kills.
const IMPORT_PROCESS_FIELDS: [&str; 5] =
    ["install_dir", "launch", "runtime", "uninstall", "tracking"];

/// Checks a `launch.before` rewrite: without `process`, a plugin may not
/// change what runs. The hook adapter (#125) applies this to every step.
pub fn launch_rewrite_allowed(
    process: bool,
    before: &serde_json::Value,
    after: &serde_json::Value,
) -> Result<(), String> {
    unchanged_without_process(
        process,
        &PROCESS_FIELDS,
        before,
        after,
        "the launch command's",
    )
}

/// Checks a `library.import` rewrite the same way: renaming an entry or
/// fixing its ids is fine, changing what it launches needs `process`.
pub fn import_rewrite_allowed(
    process: bool,
    before: &serde_json::Value,
    after: &serde_json::Value,
) -> Result<(), String> {
    unchanged_without_process(
        process,
        &IMPORT_PROCESS_FIELDS,
        before,
        after,
        "an imported game's",
    )
}

fn unchanged_without_process(
    process: bool,
    fields: &[&str],
    before: &serde_json::Value,
    after: &serde_json::Value,
    owner: &str,
) -> Result<(), String> {
    if process {
        return Ok(());
    }
    let null = serde_json::Value::Null;
    for field in fields {
        // An omitted optional field and `null` deserialize the same.
        if before.get(field).unwrap_or(&null) != after.get(field).unwrap_or(&null) {
            return Err(format!(
                "changing {owner} `{field}` needs the process permission"
            ));
        }
    }
    Ok(())
}

/// Library reads need `read` or `write`; changes need `write`.
pub fn library_allowed(granted: LibraryAccess, write: bool) -> bool {
    match granted {
        LibraryAccess::None => false,
        LibraryAccess::Read => !write,
        LibraryAccess::Write => true,
    }
}

/// Outbound HTTP for one plugin: requests to hosts and ports outside the
/// grant are refused before anything leaves the machine, and the rest
/// connect only to addresses the grant allows (`address_allowed`).
pub struct NetworkHooks {
    plugin: String,
    allowlist: Arc<[String]>,
    resolve: Resolve,
}

impl NetworkHooks {
    pub fn new(plugin: impl Into<String>, allowlist: &[String]) -> Self {
        Self::with_resolver(plugin, allowlist, system_resolve)
    }

    /// With another resolver, so tests can answer a name with any address.
    pub fn with_resolver(
        plugin: impl Into<String>,
        allowlist: &[String],
        resolve: Resolve,
    ) -> Self {
        Self {
            plugin: plugin.into(),
            allowlist: allowlist.into(),
            resolve,
        }
    }
}

impl WasiHttpHooks for NetworkHooks {
    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        fut: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Box<
        dyn Future<
                Output = HttpResult<(
                    http::Response<WasiBody>,
                    Box<dyn Future<Output = Result<(), Error>> + Send>,
                )>,
            > + Send,
    > {
        _ = fut;
        let uri = request.uri();
        let host = uri.host().unwrap_or_default().to_owned();
        let default_port = if uri.scheme_str() == Some("https") {
            443
        } else {
            80
        };
        let port = uri.port_u16().unwrap_or(default_port);
        if !request_allowed(&self.allowlist, &host, port) {
            // Only the host is logged; paths and queries may carry secrets.
            tracing::warn!(plugin = %self.plugin, host, port, "plugin HTTP request refused: host or port not granted");
            return Box::new(async { Err(Error::HttpRequestDenied) });
        }
        Box::new(outbound::send(
            self.plugin.clone(),
            self.allowlist.clone(),
            self.resolve,
            request,
            options,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_match_exactly_or_as_subdomains() {
        let allow = vec!["howlongtobeat.com".to_owned(), "*.example.org".to_owned()];
        assert!(host_allowed(&allow, "howlongtobeat.com"));
        assert!(host_allowed(&allow, "HowLongToBeat.com."));
        assert!(!host_allowed(&allow, "api.howlongtobeat.com"));
        assert!(host_allowed(&allow, "cdn.example.org"));
        assert!(!host_allowed(&allow, "example.org"));
        assert!(!host_allowed(&allow, "evilexample.org"));
        assert!(!host_allowed(&[], "howlongtobeat.com"));
    }

    fn grant(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|entry| (*entry).to_owned()).collect()
    }

    #[test]
    fn requests_use_ports_80_and_443_unless_the_grant_names_one() {
        let allow = grant(&["api.example.com", "127.0.0.1:8384", "[::1]:8080"]);
        assert!(request_allowed(&allow, "api.example.com", 443));
        assert!(request_allowed(&allow, "api.example.com", 80));
        assert!(!request_allowed(&allow, "api.example.com", 22));
        assert!(request_allowed(&allow, "127.0.0.1", 8384));
        assert!(!request_allowed(&allow, "127.0.0.1", 443));
        assert!(request_allowed(&allow, "[::1]", 8080));
        assert!(!request_allowed(&allow, "[::1]", 8081));
        assert!(!request_allowed(&allow, "other.example.com", 443));
        // A port-qualified entry still grants its host to downloads.
        assert!(host_allowed(&allow, "127.0.0.1"));
    }

    #[test]
    fn addresses_must_be_public_or_named_with_their_port() {
        let at = |ip: &str, port: u16| SocketAddr::new(ip.parse().unwrap(), port);
        let name_only = grant(&["rebind.example"]);
        for local in [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.0.1",
            "192.168.1.20",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "::",
            "fe80::1",
            "fd00::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.20",
        ] {
            assert!(!address_allowed(&name_only, at(local, 443)), "{local}");
        }
        assert!(address_allowed(&name_only, at("93.184.216.34", 443)));
        assert!(address_allowed(&name_only, at("2606:2800:220:1::1", 443)));

        let named = grant(&["127.0.0.1:8384", "192.168.1.20", "localhost:3000"]);
        assert!(address_allowed(&named, at("127.0.0.1", 8384)));
        assert!(!address_allowed(&named, at("127.0.0.1", 22)));
        assert!(address_allowed(&named, at("192.168.1.20", 80)));
        assert!(!address_allowed(&named, at("192.168.1.20", 8080)));
        assert!(address_allowed(&named, at("::1", 3000)));
        assert!(!address_allowed(&named, at("10.0.0.5", 80)));
    }

    /// Answers every name with loopback, as a rebinding DNS server would.
    fn rebind(_host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
    }

    /// A local service's listener and its port.
    fn local_service() -> (std::net::TcpListener, u16) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    }

    fn answer(listener: std::net::TcpListener) {
        use std::io::{Read, Write};
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            }
        });
    }

    async fn get(hooks: &mut NetworkHooks, url: &str) -> HttpResult<u16> {
        use http_body_util::BodyExt;
        let body: WasiBody = http_body_util::Empty::<hyper::body::Bytes>::new()
            .map_err(|never| match never {})
            .boxed_unsync();
        let request = http::Request::get(url).body(body).unwrap();
        let sent = hooks.send_request(request, None, Box::new(async { Ok::<(), Error>(()) }));
        let (response, _io) = Box::into_pin(sent).await?;
        Ok(response.status().as_u16())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_granted_name_that_resolves_to_loopback_is_never_connected_to() {
        let (listener, port) = local_service();
        listener.set_nonblocking(true).unwrap();
        let mut hooks = NetworkHooks::with_resolver(
            "dev.caskade.caps",
            &grant(&[format!("rebind.example:{port}").as_str()]),
            rebind,
        );
        let refused = get(&mut hooks, &format!("http://rebind.example:{port}/")).await;
        assert!(
            matches!(refused, Err(Error::DestinationIpProhibited)),
            "{refused:?}"
        );
        // Nothing reached the local service.
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );

        // A grant that names the address itself, with its port, reaches it.
        listener.set_nonblocking(false).unwrap();
        answer(listener);
        let literal = format!("127.0.0.1:{port}");
        let mut hooks = NetworkHooks::new("dev.caskade.caps", &grant(&[literal.as_str()]));
        assert_eq!(
            get(&mut hooks, &format!("http://{literal}/"))
                .await
                .unwrap(),
            200
        );
        let mut named = NetworkHooks::with_resolver(
            "dev.caskade.caps",
            &grant(&[format!("rebind.example:{port}").as_str(), literal.as_str()]),
            rebind,
        );
        assert_eq!(
            get(&mut named, &format!("http://rebind.example:{port}/"))
                .await
                .unwrap(),
            200
        );
        // Without its port the literal grant allows 80 and 443 only.
        let mut hooks = NetworkHooks::new("dev.caskade.caps", &grant(&["127.0.0.1"]));
        let refused = get(&mut hooks, &format!("http://{literal}/")).await;
        assert!(
            matches!(refused, Err(Error::HttpRequestDenied)),
            "{refused:?}"
        );
    }

    #[test]
    fn grants_cover_what_was_asked() {
        let granted = Permissions {
            network: vec!["a.com".into()],
            fs: vec!["$HOME/ROMs:ro".into()],
            process: false,
            library: LibraryAccess::Write,
        };
        assert!(covers(&granted, &granted));
        assert!(covers(&granted, &Permissions::default()));
        let mut more = granted.clone();
        more.network.push("b.com".into());
        assert!(!covers(&granted, &more));
        let mut process = granted.clone();
        process.process = true;
        assert!(!covers(&granted, &process));
        let read = Permissions {
            library: LibraryAccess::Read,
            ..Permissions::default()
        };
        assert!(covers(&granted, &read));
        assert!(!covers(&read, &granted));
    }

    #[test]
    fn launch_rewrites_need_the_process_permission() {
        let before =
            serde_json::json!({"program": "game", "args": ["-a"], "env": {}, "title": "x"});
        let mut wrapped = before.clone();
        wrapped["program"] = "gamemoderun".into();
        wrapped["args"] = serde_json::json!(["game", "-a"]);
        assert!(launch_rewrite_allowed(true, &before, &wrapped).is_ok());
        let error = launch_rewrite_allowed(false, &before, &wrapped).unwrap_err();
        assert!(error.contains("`program`"), "{error}");
        let mut hooked = before.clone();
        hooked["emulation"] = serde_json::json!({"before": ["/bin/sh", "-c", "x"]});
        let error = launch_rewrite_allowed(false, &before, &hooked).unwrap_err();
        assert!(error.contains("`emulation`"), "{error}");
        let mut retitled = before.clone();
        retitled["title"] = "y".into();
        assert!(launch_rewrite_allowed(false, &before, &retitled).is_ok());
    }

    #[test]
    fn import_rewrites_that_change_what_runs_need_the_process_permission() {
        let before = serde_json::json!({
            "title": "Game",
            "ids": [],
            "install_dir": "/games/game",
            "launch": [{"type": "uri", "value": "steam://rungameid/1"}],
            "runtime": {"type": "native"},
            "uninstall": null,
        });
        let mut renamed = before.clone();
        renamed["title"] = "Game: Definitive Edition".into();
        renamed.as_object_mut().unwrap().remove("uninstall");
        assert!(import_rewrite_allowed(false, &before, &renamed).is_ok());
        for (field, value) in [
            (
                "launch",
                serde_json::json!([{"type": "uri", "value": "ms-msdt:x"}]),
            ),
            ("install_dir", serde_json::json!("/tmp")),
            ("runtime", serde_json::json!({"type": "wine", "value": {}})),
            ("uninstall", serde_json::json!({"command": "rm"})),
            ("tracking", serde_json::json!({"exe_names": ["sshd"]})),
        ] {
            let mut changed = before.clone();
            changed[field] = value;
            let error = import_rewrite_allowed(false, &before, &changed).unwrap_err();
            assert!(error.contains(&format!("`{field}`")), "{error}");
            assert!(import_rewrite_allowed(true, &before, &changed).is_ok());
        }
    }

    #[test]
    fn folder_permissions_expand_home_and_skip_missing_folders() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("ROMs")).unwrap();
        let grants = folder_grants(
            &[
                "$HOME/ROMs:ro".into(),
                "~/ROMs".into(),
                "$HOME/Missing".into(),
                "relative".into(),
            ],
            Some(temp.path()),
        );
        assert_eq!(
            grants,
            [
                FolderGrant {
                    path: temp.path().join("ROMs"),
                    read_only: true
                },
                FolderGrant {
                    path: temp.path().join("ROMs"),
                    read_only: false
                },
            ]
        );
        // #272: an absolute folder as the OS writes it (a drive letter on
        // Windows) reads and writes unless it ends in :ro.
        let roms = temp.path().join("ROMs");
        let grants = folder_grants(
            &[
                roms.display().to_string(),
                format!("{}:ro", roms.display()),
                format!("{}:rx", roms.display()),
            ],
            None,
        );
        assert_eq!(
            grants,
            [
                FolderGrant {
                    path: roms.clone(),
                    read_only: false
                },
                FolderGrant {
                    path: roms,
                    read_only: true
                },
            ]
        );
        assert_eq!(folder_mode(r"C:\Games"), Some((r"C:\Games", false)));
        assert_eq!(folder_mode(r"C:\Games:ro"), Some((r"C:\Games", true)));
        assert_eq!(folder_mode(r"C:\Games:rx"), None);
        assert!(library_allowed(LibraryAccess::Read, false));
        assert!(!library_allowed(LibraryAccess::Read, true));
        assert!(!library_allowed(LibraryAccess::None, false));
    }
}
