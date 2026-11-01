use std::path::{Path, PathBuf};

use super::{CancellationToken, ExecError};

/// Run maintenance under the same root exclusion as installs and uninstalls.
/// The blocking worker retains its lease after its async caller is dropped;
/// dropping that caller cancels the child token supplied to the operation.
/// An operation must check the token between bounded units of work.
pub async fn with_root_exclusive<T, E, F>(
    root: &Path,
    cancellation: CancellationToken,
    operation: F,
) -> Result<T, E>
where
    T: Send + 'static,
    E: From<ExecError> + Send + 'static,
    F: FnOnce(PathBuf, CancellationToken) -> Result<T, E> + Send + 'static,
{
    let root = std::fs::canonicalize(root)
        .map_err(ExecError::Io)
        .map_err(E::from)?;
    let token = cancellation.child_token();
    let _cancel_on_drop = token.clone().drop_guard();
    if token.is_cancelled() {
        return Err(E::from(ExecError::Cancelled));
    }
    let guard = tokio::select! {
        _ = token.cancelled() => return Err(E::from(ExecError::Cancelled)),
        guard = super::engine::root_lock(&root).lock_owned() => guard,
    };
    if token.is_cancelled() {
        return Err(E::from(ExecError::Cancelled));
    }
    caskade_jobs::blocking("Checking game files", move || {
        let _guard = guard;
        operation(root, token)
    })
    .await
    .map_err(|error| {
        E::from(ExecError::Worker {
            message: error.to_string(),
        })
    })?
}

/// [`with_root_exclusive`] over several roots (e.g. a move's source and
/// target). Locks are taken in canonical, sorted, de-duplicated order, so two
/// opposite moves between the same roots cannot deadlock.
pub async fn with_roots_exclusive<T, E, F>(
    roots: &[&Path],
    cancellation: CancellationToken,
    operation: F,
) -> Result<T, E>
where
    T: Send + 'static,
    E: From<ExecError> + Send + 'static,
    F: FnOnce(Vec<PathBuf>, CancellationToken) -> Result<T, E> + Send + 'static,
{
    let mut canonical = roots
        .iter()
        .map(|root| std::fs::canonicalize(root).map_err(ExecError::Io))
        .collect::<Result<Vec<_>, _>>()
        .map_err(E::from)?;
    canonical.sort();
    canonical.dedup();
    let token = cancellation.child_token();
    let _cancel_on_drop = token.clone().drop_guard();
    let mut guards = Vec::with_capacity(canonical.len());
    for root in &canonical {
        if token.is_cancelled() {
            return Err(E::from(ExecError::Cancelled));
        }
        guards.push(tokio::select! {
            _ = token.cancelled() => return Err(E::from(ExecError::Cancelled)),
            guard = super::engine::root_lock(root).lock_owned() => guard,
        });
    }
    if token.is_cancelled() {
        return Err(E::from(ExecError::Cancelled));
    }
    caskade_jobs::blocking("Moving game files", move || {
        let _guards = guards;
        operation(canonical, token)
    })
    .await
    .map_err(|error| {
        E::from(ExecError::Worker {
            message: error.to_string(),
        })
    })?
}
