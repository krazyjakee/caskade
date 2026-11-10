//! The app's async runtime: one runtime for the whole process, which the
//! shell hands to its own toolkit (Tauri's `async_runtime::set`) so that
//! everything spawns onto the same workers. Work spawned from a thread
//! outside the runtime (startup, a window's callbacks) goes through here.

use std::{future::Future, sync::OnceLock};
use tokio::{
    runtime::{Builder, Handle},
    task::JoinHandle,
};

static RUNTIME: OnceLock<Handle> = OnceLock::new();

/// Starts the runtime with at most `workers` workers (no more than the
/// machine's cores): the app's async work waits on I/O, and its heavy work
/// runs on blocking threads, which come and go as needed. Its handle, which
/// the first caller sets; later calls return that.
pub fn init(workers: usize) -> std::io::Result<Handle> {
    if let Some(handle) = RUNTIME.get() {
        return Ok(handle.clone());
    }
    let workers = std::thread::available_parallelism()
        .map_or(workers, |cores| cores.get().min(workers))
        .max(1);
    let runtime = Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()?;
    // It runs as long as the process: only handles are kept.
    let handle = Box::leak(Box::new(runtime)).handle().clone();
    Ok(RUNTIME.get_or_init(|| handle).clone())
}

/// The runtime's handle; one with the default workers if [`init`] wasn't
/// called (tests, tools).
pub fn handle() -> Handle {
    RUNTIME
        .get_or_init(|| {
            let runtime = Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("the async runtime starts");
            Box::leak(Box::new(runtime)).handle().clone()
        })
        .clone()
}

/// Runs `future` on the runtime, from any thread.
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    handle().spawn(future)
}

/// Runs blocking `work` on the runtime's blocking threads.
pub fn spawn_blocking<F, T>(work: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    handle().spawn_blocking(work)
}

/// Waits for `future` on the calling thread, which must not be one of the
/// runtime's own.
pub fn block_on<F: Future>(future: F) -> F::Output {
    handle().block_on(future)
}
