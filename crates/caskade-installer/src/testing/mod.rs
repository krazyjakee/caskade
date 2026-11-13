//! Public deterministic seams for integration tests. Fake process file writes
//! really touch the supplied temporary paths: use `TestRoot` to contain tests.
//! They intentionally model hostile writes; this module is not a sandbox.

mod fake_runtime;
mod pe;
mod probe_tree;
mod root;
mod windows_runner;

use std::{
    io,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    disk::DiskSpace,
    events::{EventSink, JobEvent},
};

pub use fake_runtime::*;
pub use pe::pe_with_icon;
pub use probe_tree::probe_tree;
pub use root::TestRoot;
pub use windows_runner::{host_for_windows_payloads, windows_runner_path};

#[derive(Debug)]
pub struct FakeDiskSpace {
    remaining: AtomicU64,
    decrease: u64,
}

impl FakeDiskSpace {
    pub fn fixed(bytes: u64) -> Self {
        Self::decreasing(bytes, 0)
    }
    /// Each query returns the current value and subtracts the given amount,
    /// saturating at zero, so disk exhaustion can be exercised deterministically.
    pub fn decreasing(initial: u64, per_query: u64) -> Self {
        Self {
            remaining: AtomicU64::new(initial),
            decrease: per_query,
        }
    }
}

impl DiskSpace for FakeDiskSpace {
    fn available_bytes(&self, _: &Path) -> io::Result<u64> {
        // Rust 1.99 renamed `fetch_update` to `try_update`, which the MSRV
        // lacks: the same compare-exchange loop, spelled out.
        let mut current = self.remaining.load(Ordering::SeqCst);
        loop {
            let next = current.saturating_sub(self.decrease);
            match self.remaining.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(previous) => return Ok(previous),
                Err(actual) => current = actual,
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct RecordingSink {
    events: Mutex<Vec<JobEvent>>,
}

impl RecordingSink {
    pub fn events(&self) -> Vec<JobEvent> {
        self.events.lock().expect("event sink poisoned").clone()
    }
    pub fn take(&self) -> Vec<JobEvent> {
        std::mem::take(&mut *self.events.lock().expect("event sink poisoned"))
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: JobEvent) {
        self.events.lock().expect("event sink poisoned").push(event);
    }
}
