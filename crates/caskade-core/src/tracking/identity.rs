//! Telling a process from a later one that reuses its PID, for as long as
//! either can be running, whatever the system clock does.
//!
//! Stop and session recovery act on these identities, and the wrong process
//! must never be touched, so a match is exact and anything unknown is no match.
//! What an identity holds must not change while its process lives:
//!
//! - Linux: the start in clock ticks since boot (`/proc/<pid>/stat`), which
//!   the kernel keeps on a clock nothing can set, and the boot it counts from
//!   (`/proc/sys/kernel/random/boot_id`), as PIDs and ticks both start over. A
//!   reused PID has a later tick, to 10 ms where sysinfo's seconds gave 1 s.
//!   sysinfo's own start time won't do: it adds `/proc/stat`'s `btime` to the
//!   ticks, and `btime` is the wall clock now minus the time since boot, so
//!   it moves when the clock steps. A step then looks exactly like a reused
//!   PID, and `tests/clock_step.rs` shows it moving on a real clock.
//! - Windows and macOS: sysinfo's start time, which is the OS's own record
//!   made once at creation (`GetProcessTimes`, `p_start`) and never revised. It
//!   comes from the wall clock, so two starts can compare out of order across
//!   a step, but one process's value stays the same: `tests/clock_step.rs`
//!   checks that on Windows. On macOS it rests on XNU's source (`p_start`, set
//!   in `fork` and only read after), as no CI job runs the core there.

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System};

/// A PID alone is unsafe across app restarts: the OS may reuse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// When it started: see the module's notes. Only ever compared with
    /// another identity's, never with a time.
    pub started_at: u64,
    /// The boot `started_at` counts from, or 0 when it is sysinfo's start
    /// time. Saved records from before this field have 0 too.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub boot: u64,
}

fn is_zero(boot: &u64) -> bool {
    *boot == 0
}

impl ProcessIdentity {
    /// `process`, running as `pid`, as it is identified today.
    pub fn of(pid: u32, process: &Process) -> Self {
        #[cfg(target_os = "linux")]
        if let Some((started_at, boot)) = linux::started(pid) {
            return Self {
                pid,
                started_at,
                boot,
            };
        }
        Self {
            pid,
            started_at: wall_start(process),
            boot: 0,
        }
    }

    /// Whatever is running as `pid` now.
    pub fn lookup(pid: u32) -> Option<Self> {
        reading(pid, |process| Self::of(pid, process))
    }

    /// A saved identity in today's terms. One saved on Linux by a version
    /// that kept sysinfo's seconds is still checked against them, exactly:
    /// it is only upgraded if that is the process it recorded.
    pub fn current(self) -> Self {
        if self.boot != 0 {
            return self;
        }
        reading(self.pid, |process| {
            (wall_start(process) == self.started_at).then(|| Self::of(self.pid, process))
        })
        .flatten()
        .unwrap_or(self)
    }
}

fn reading<T>(pid: u32, read: impl FnOnce(&Process) -> T) -> Option<T> {
    let id = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[id]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system.process(id).map(read)
}

/// When sysinfo says `process` started, in seconds since the Unix epoch.
fn wall_start(process: &Process) -> u64 {
    let started = process.start_time();
    #[cfg(test)]
    let started = started.saturating_add_signed(STEP.get());
    started
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::OnceLock;

    /// A process's start in ticks since boot, and that boot. `None` when
    /// either can't be read, and the caller falls back on sysinfo's seconds.
    pub(super) fn started(pid: u32) -> Option<(u64, u64)> {
        static BOOT: OnceLock<Option<u64>> = OnceLock::new();
        let boot = (*BOOT.get_or_init(|| {
            let id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
            boot_key(&id)
        }))?;
        let stat = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
        Some((start_ticks(&stat)?, boot))
    }

    /// `boot_id` is a random UUID; 64 of its bits tell boots apart. Never 0,
    /// which means no boot.
    pub(super) fn boot_key(id: &str) -> Option<u64> {
        let hex: String = id.trim().chars().filter(|c| *c != '-').take(16).collect();
        u64::from_str_radix(&hex, 16).ok().filter(|key| *key != 0)
    }

    /// Field 22 of `stat`. The name in field 2 may hold spaces and brackets,
    /// so fields are counted from the last `)`, where field 3 starts.
    pub(super) fn start_ticks(stat: &[u8]) -> Option<u64> {
        let name_end = stat.iter().rposition(|byte| *byte == b')')?;
        std::str::from_utf8(&stat[name_end + 1..])
            .ok()?
            .split_ascii_whitespace()
            .nth(22 - 3)?
            .parse()
            .ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_start_is_read_past_any_process_name() {
            let fields = |name: &str| {
                format!("7 ({name}) S 1 7 7 0 -1 4194304 1 0 0 0 0 0 0 0 20 0 1 0 987654 1 2\n")
            };
            assert_eq!(start_ticks(fields("game").as_bytes()), Some(987_654));
            assert_eq!(start_ticks(fields("a) S 1 2 (b").as_bytes()), Some(987_654));
            assert_eq!(start_ticks(b"7 (game) S 1"), None);
            assert_eq!(start_ticks(b"no name here"), None);
        }

        #[test]
        fn a_boot_is_never_the_key_for_no_boot() {
            assert_eq!(
                boot_key("0123abcd-4567-89ab-cdef-0123456789ab\n"),
                Some(0x0123_abcd_4567_89ab)
            );
            assert_eq!(boot_key("00000000-0000-0000-cdef-0123456789ab"), None);
            assert_eq!(boot_key("not a boot id"), None);
        }
    }
}

#[cfg(test)]
thread_local! {
    static STEP: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

/// Stands in for a stepped system clock on this thread: what sysinfo reports
/// for a running process moves as it does on Linux, and tests leave the real
/// clock alone (`tests/clock_step.rs` steps it on throwaway CI runners).
#[cfg(test)]
pub(crate) struct ClockStep;

#[cfg(test)]
impl ClockStep {
    pub(crate) fn by(seconds: i64) -> Self {
        STEP.set(seconds);
        Self
    }
}

#[cfg(test)]
impl Drop for ClockStep {
    fn drop(&mut self) {
        STEP.set(0);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn running() -> Running {
        Running(
            std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap(),
        )
    }

    /// sysinfo's seconds for `pid`, as a version before boots saved them.
    fn saved_before_boots(pid: u32) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            started_at: reading(pid, wall_start).unwrap(),
            boot: 0,
        }
    }

    #[test]
    fn a_process_keeps_its_identity_and_a_later_one_never_shares_it() {
        let game = running();
        let pid = game.0.id();
        let now = ProcessIdentity::lookup(pid).unwrap();
        assert_ne!(now.boot, 0);
        assert_eq!(ProcessIdentity::lookup(pid), Some(now));
        {
            let _stepped = ClockStep::by(7200);
            assert_eq!(ProcessIdentity::lookup(pid), Some(now));
        }
        for other in [
            ProcessIdentity {
                started_at: now.started_at + 1,
                ..now
            },
            ProcessIdentity {
                started_at: now.started_at - 1,
                ..now
            },
            ProcessIdentity {
                boot: now.boot ^ 1,
                ..now
            },
        ] {
            assert_ne!(ProcessIdentity::lookup(pid), Some(other));
        }
    }

    #[test]
    fn a_saved_identity_upgrades_only_if_it_is_that_process() {
        let game = running();
        let pid = game.0.id();
        let now = ProcessIdentity::lookup(pid).unwrap();

        let saved = saved_before_boots(pid);
        assert_eq!(saved.current(), now);
        // A file from before this field says the same thing.
        let file = format!(r#"{{"pid":{pid},"started_at":{}}}"#, saved.started_at);
        let read: ProcessIdentity = serde_json::from_str(&file).unwrap();
        assert_eq!(read, saved);

        let reused = ProcessIdentity {
            started_at: saved.started_at + 1,
            ..saved
        };
        assert_eq!(reused.current(), reused);
        assert_ne!(ProcessIdentity::lookup(pid), Some(reused.current()));
        // One saved under another boot isn't this process, and isn't a guess.
        let earlier_boot = ProcessIdentity {
            boot: now.boot ^ 1,
            ..now
        };
        assert_eq!(earlier_boot.current(), earlier_boot);
    }

    #[test]
    fn saving_needs_no_new_field_where_there_is_no_boot() {
        let id = ProcessIdentity {
            pid: 7,
            started_at: 9,
            boot: 0,
        };
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            r#"{"pid":7,"started_at":9}"#
        );
    }
}
