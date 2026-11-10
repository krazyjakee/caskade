//! The local channel between `caskade --launch` and the running Caskade:
//! a Unix socket in a folder only the user can open, or on Windows a named
//! pipe that refuses remote clients. Its name comes from Caskade's data
//! folders, so a dev build and an installed one keep apart.

use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

/// Either end of a connection.
pub trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}
pub type Stream = Box<dyn Duplex>;

/// Where the running Caskade takes launches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    #[cfg(unix)]
    path: std::path::PathBuf,
    #[cfg(windows)]
    name: String,
}

impl Endpoint {
    pub fn for_paths(paths: &caskade_core::AppPaths) -> Self {
        #[cfg(unix)]
        {
            Self::at(paths.cache.join("run").join("launch.sock"))
        }
        #[cfg(windows)]
        {
            Self::at(paths.cache.clone())
        }
    }

    /// A socket at `path`, whose folder only the user may open, or on
    /// Windows a pipe named after `path`.
    pub fn at(path: std::path::PathBuf) -> Self {
        #[cfg(unix)]
        {
            Self { path }
        }
        #[cfg(windows)]
        {
            let hash = fnv1a(path.to_string_lossy().as_bytes());
            Self {
                name: format!(r"\\.\pipe\caskade-launch-{hash:016x}"),
            }
        }
    }
}

/// A stable hash, the same in every build.
#[cfg(windows)]
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub async fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    #[cfg(unix)]
    {
        Ok(Box::new(
            tokio::net::UnixStream::connect(&endpoint.path).await?,
        ))
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        const ERROR_PIPE_BUSY: i32 = 231;
        for _ in 0..50 {
            match ClientOptions::new().open(&endpoint.name) {
                Ok(client) => return Ok(Box::new(client)),
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "Caskade is busy"))
    }
}

/// Takes connections at an endpoint.
pub struct Listener {
    #[cfg(unix)]
    socket: tokio::net::UnixListener,
    #[cfg(unix)]
    path: std::path::PathBuf,
    #[cfg(windows)]
    name: String,
    #[cfg(windows)]
    next: tokio::net::windows::named_pipe::NamedPipeServer,
}

impl Listener {
    /// Listens at `endpoint`; `AddrInUse` when another Caskade already does.
    pub async fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            let path = endpoint.path.clone();
            if let Some(folder) = path.parent() {
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(folder)?;
                std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
            }
            if tokio::net::UnixStream::connect(&path).await.is_ok() {
                return Err(io::ErrorKind::AddrInUse.into());
            }
            // What a Caskade that ended without tidying up left behind.
            match std::fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
            let socket = tokio::net::UnixListener::bind(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            Ok(Self { socket, path })
        }
        #[cfg(windows)]
        {
            let next = pipe(&endpoint.name, true)?;
            Ok(Self {
                name: endpoint.name.clone(),
                next,
            })
        }
    }

    pub async fn accept(&mut self) -> io::Result<Stream> {
        #[cfg(unix)]
        {
            let (stream, _) = self.socket.accept().await?;
            Ok(Box::new(stream))
        }
        #[cfg(windows)]
        {
            self.next.connect().await?;
            let next = pipe(&self.name, false)?;
            Ok(Box::new(std::mem::replace(&mut self.next, next)))
        }
    }
}

#[cfg(windows)]
fn pipe(name: &str, first: bool) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    tokio::net::windows::named_pipe::ServerOptions::new()
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .create(name)
}

#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
