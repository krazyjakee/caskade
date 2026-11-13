//! The in-memory tables of scans, detections and emulator installs.

use crate::lock;
use std::{collections::BTreeMap, sync::Mutex};

/// A listed job: bumped on every change, finished or not.
pub(crate) trait TableJob: Clone {
    fn id(&self) -> &str;
    fn revision(&mut self) -> &mut u32;
    fn finished(&self) -> bool;
}

/// Jobs by id (new ids sort last), each with what controls it (its cancel
/// token).
pub(crate) struct JobTable<J, C> {
    jobs: Mutex<BTreeMap<String, (J, C)>>,
}

impl<J, C> Default for JobTable<J, C> {
    fn default() -> Self {
        Self {
            jobs: Mutex::default(),
        }
    }
}

impl<J: TableJob, C: Clone> JobTable<J, C> {
    /// Lists `job`. With `keep`, finished jobs beyond the last `keep` are
    /// dropped.
    pub fn insert(&self, job: J, control: C, keep: Option<usize>) {
        let mut jobs = lock(&self.jobs);
        if let Some(keep) = keep {
            let finished: Vec<String> = jobs
                .values()
                .filter(|(job, _)| job.finished())
                .map(|(job, _)| job.id().to_owned())
                .collect();
            for id in finished.iter().take(finished.len().saturating_sub(keep)) {
                jobs.remove(id);
            }
        }
        jobs.insert(job.id().to_owned(), (job, control));
    }

    /// Applies `change` and bumps the revision; the changed job, for the
    /// caller to publish.
    pub fn update(&self, id: &str, change: impl FnOnce(&mut J)) -> Option<J> {
        let mut jobs = lock(&self.jobs);
        let (job, _) = jobs.get_mut(id)?;
        change(job);
        *job.revision() += 1;
        Some(job.clone())
    }

    pub fn get(&self, id: &str) -> Option<(J, C)> {
        lock(&self.jobs).get(id).cloned()
    }

    pub fn job(&self, id: &str) -> Option<J> {
        lock(&self.jobs).get(id).map(|(job, _)| job.clone())
    }

    /// Every job, oldest first.
    pub fn jobs(&self) -> Vec<J> {
        lock(&self.jobs)
            .values()
            .map(|(job, _)| job.clone())
            .collect()
    }
}
