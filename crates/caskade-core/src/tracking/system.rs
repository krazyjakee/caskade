use super::{ProcessIdentity, ProcessInfo, ProcessSnapshot};
use std::path::PathBuf;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

/// The system's processes over a reused sysinfo cache. Its reads block, so
/// [`ProcessMonitor`](super::ProcessMonitor) runs them off Tokio's worker
/// threads.
#[derive(Default)]
pub struct SystemProcesses {
    system: System,
}

/// Executable, working directory, arguments and environment: what the
/// matcher reads. All of them can change during a process's life (`exec`
/// replaces the first three and the environment, `chdir` the directory), so
/// a process that may join a session must be re-read on every poll.
fn metadata(update: UpdateKind) -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .without_tasks()
        .with_exe(update)
        .with_cwd(update)
        .with_cmd(update)
        .with_environ(update)
}

impl SystemProcesses {
    /// Every process with fresh metadata.
    pub fn snapshot(&mut self) -> ProcessSnapshot {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            metadata(UpdateKind::Always),
        );
        self.collect()
    }

    /// A tracking poll. Only processes started at or after `since` (Unix
    /// seconds) can join a session; the rest are in its baseline.
    ///
    /// Lists every process from its `stat` alone (identity, parent, status),
    /// then re-reads the metadata of processes started since the session's
    /// baseline. Older processes are in the baseline, which the matcher skips,
    /// so the hundreds of unrelated system processes cost one read each.
    pub fn snapshot_since(&mut self, since: u64) -> ProcessSnapshot {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        let fresh: Vec<Pid> = self
            .system
            .processes()
            .iter()
            .filter(|(_, process)| process.start_time() >= since)
            .map(|(pid, _)| *pid)
            .collect();
        if !fresh.is_empty() {
            self.system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&fresh),
                false,
                metadata(UpdateKind::Always),
            );
        }
        self.collect()
    }

    fn collect(&self) -> ProcessSnapshot {
        ProcessSnapshot {
            processes: self
                .system
                .processes()
                .iter()
                .filter_map(|(pid, process)| {
                    if matches!(
                        process.status(),
                        ProcessStatus::Zombie | ProcessStatus::Dead
                    ) {
                        return None;
                    }
                    let steam_app_id = process
                        .environ()
                        .iter()
                        .filter_map(|v| v.to_str())
                        .filter_map(|s| {
                            s.strip_prefix("SteamAppId=")
                                .or_else(|| s.strip_prefix("SteamGameId="))
                        })
                        .find_map(|s| s.parse::<u32>().ok().filter(|id| *id > 0));
                    Some(ProcessInfo {
                        identity: ProcessIdentity {
                            pid: pid.as_u32(),
                            started_at: process.start_time(),
                        },
                        parent: process.parent().map(|pid| pid.as_u32()),
                        name: process.name().to_str().map(str::to_owned),
                        executable: process.exe().map(PathBuf::from),
                        cwd: process.cwd().map(PathBuf::from),
                        arguments: process.cmd().iter().take(256).map(PathBuf::from).collect(),
                        steam_app_id,
                    })
                })
                .collect(),
        }
    }
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
