//! Public deterministic seams for integration tests. Fake process file writes
//! really touch the supplied temporary paths: use `TestRoot` to contain tests.
//! They intentionally model hostile writes; this module is not a sandbox.

mod fake_runtime;
mod pe;
mod root;

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
pub use root::TestRoot;

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
        Ok(self
            .remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                Some(current.saturating_sub(self.decrease))
            })
            .expect("update always succeeds"))
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
