//! A live limit: lowering it lets active downloads finish while queued
//! requests wait; increasing it wakes queued jobs immediately.
use super::*;
pub(super) struct DownloadQueue {
    state: Mutex<(usize, usize)>,
    changed: tokio::sync::Notify,
}
pub(super) struct Permit {
    queue: Arc<DownloadQueue>,
}
impl DownloadQueue {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((limit.max(1), 0)),
            changed: Default::default(),
        })
    }
    pub fn set_limit(&self, limit: usize) {
        lock(&self.state).0 = limit.clamp(1, 8);
        self.changed.notify_waiters();
    }
    pub async fn acquire(self: &Arc<Self>, cancel: &CancellationToken) -> Result<Permit> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if cancel.is_cancelled() {
                return Err(Error::Invalid(crate::tr!("cancelled")));
            }
            {
                let mut state = lock(&self.state);
                if state.1 < state.0 {
                    state.1 += 1;
                    return Ok(Permit {
                        queue: self.clone(),
                    });
                }
            }
            tokio::select! {_=cancel.cancelled()=>return Err(Error::Invalid(crate::tr!("cancelled"))),_=notified=>()}
        }
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        lock(&self.queue.state).1 -= 1;
        self.queue.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn changing_limits_drains_active_downloads_and_wakes_waiters() {
        let queue = DownloadQueue::new(2);
        let cancel = CancellationToken::new();
        let one = queue.acquire(&cancel).await.unwrap();
        let two = queue.acquire(&cancel).await.unwrap();
        queue.set_limit(1);
        let pending = {
            let queue = queue.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { queue.acquire(&cancel).await })
        };
        tokio::task::yield_now().await;
        drop(one);
        tokio::task::yield_now().await;
        assert!(!pending.is_finished());
        drop(two);
        let three = tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let pending = {
            let queue = queue.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { queue.acquire(&cancel).await })
        };
        tokio::task::yield_now().await;
        assert!(!pending.is_finished());
        queue.set_limit(2);
        let four = tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let pending = {
            let queue = queue.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { queue.acquire(&cancel).await })
        };
        cancel.cancel();
        assert!(pending.await.unwrap().is_err());
        drop((three, four));
        assert_eq!(lock(&queue.state).1, 0);
    }
}
