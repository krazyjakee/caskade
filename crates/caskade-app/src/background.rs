//! Background tasks that outlive a panic: [`supervise`] starts them again
//! after a pause, and [`log_panics`] sends every panic to the log.

/// Sends panics to the log as well as stderr: a panic in a background task
/// otherwise ends it with nothing in the app's log. Chains to the hook that
/// was there before (the default one prints to stderr).
pub fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let message = panic_message(info.payload());
        let location = info.location().map(ToString::to_string);
        tracing::error!(
            thread = thread.name().unwrap_or("unnamed"),
            location = location.as_deref().unwrap_or("unknown"),
            "panic: {message}"
        );
        previous(info);
    }));
}

/// The text a panic carries, when it is a string.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)")
}

/// The pause before a panicked task starts again; it doubles on each panic
/// in a row, up to [`RESTART_MAX`], so a task that panics at once doesn't
/// spin.
pub const RESTART_DELAY: std::time::Duration = std::time::Duration::from_secs(10);
pub const RESTART_MAX: std::time::Duration = std::time::Duration::from_secs(600);

/// Keeps the task `start` spawns running: when it panics, it's logged and
/// started again after a pause. A task that ends on its own (its channel
/// closed at shutdown) or is cancelled is left ended.
pub fn supervise(
    name: &'static str,
    start: impl Fn() -> tokio::task::JoinHandle<()> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    supervise_with(name, RESTART_DELAY, start)
}

pub(crate) fn supervise_with(
    name: &'static str,
    delay: std::time::Duration,
    start: impl Fn() -> tokio::task::JoinHandle<()> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    crate::runtime::spawn(async move {
        let mut pause = delay;
        loop {
            let started = std::time::Instant::now();
            match start().await {
                Ok(()) => {
                    tracing::debug!(task = name, "background task ended");
                    return;
                }
                Err(error) if error.is_panic() => {
                    let panic = error.into_panic();
                    tracing::error!(
                        task = name,
                        panic = panic_message(&*panic),
                        restart_in = ?pause,
                        "background task panicked"
                    );
                }
                Err(_) => return,
            }
            // A task that ran a good while before panicking starts over
            // from the short pause.
            if started.elapsed() > RESTART_MAX {
                pause = delay;
            }
            tokio::time::sleep(pause).await;
            pause = (pause * 2).min(RESTART_MAX.max(delay));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn a_panicked_task_starts_again_and_one_that_ends_stays_ended() {
        crate::runtime::block_on(async {
            let starts = Arc::new(AtomicUsize::new(0));
            let supervisor = supervise_with("test", std::time::Duration::from_millis(5), {
                let starts = starts.clone();
                move || {
                    let run = starts.fetch_add(1, Ordering::SeqCst);
                    tokio::spawn(async move {
                        // Panics twice, then ends.
                        assert!(run >= 2, "panic number {run}");
                    })
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), supervisor)
                .await
                .expect("the supervisor stops once the task ends")
                .unwrap();
            assert_eq!(starts.load(Ordering::SeqCst), 3);
        });
    }
}
