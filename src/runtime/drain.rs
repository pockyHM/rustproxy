use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::Notify;

#[derive(Clone, Default)]
pub struct DrainController {
    inner: Arc<DrainInner>,
}

#[derive(Default)]
struct DrainInner {
    draining: AtomicBool,
    active: AtomicU64,
    notify_empty: Notify,
}

pub struct DrainLease {
    inner: Arc<DrainInner>,
}

impl DrainController {
    pub fn try_acquire(&self) -> Option<DrainLease> {
        if self.inner.draining.load(Ordering::Acquire) {
            return None;
        }

        self.inner.active.fetch_add(1, Ordering::AcqRel);
        if self.inner.draining.load(Ordering::Acquire) {
            self.release_one();
            return None;
        }

        Some(DrainLease {
            inner: self.inner.clone(),
        })
    }

    pub fn start_draining(&self) {
        self.inner.draining.store(true, Ordering::Release);
        if self.active() == 0 {
            self.inner.notify_empty.notify_waiters();
        }
    }

    pub async fn wait_empty(&self, timeout: Duration) -> bool {
        if self.active() == 0 {
            return true;
        }

        tokio::time::timeout(timeout, async {
            loop {
                self.inner.notify_empty.notified().await;
                if self.active() == 0 {
                    break;
                }
            }
        })
        .await
        .is_ok()
    }

    pub fn active(&self) -> u64 {
        self.inner.active.load(Ordering::Acquire)
    }

    pub fn is_draining(&self) -> bool {
        self.inner.draining.load(Ordering::Acquire)
    }

    fn release_one(&self) {
        if self.inner.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.inner.notify_empty.notify_waiters();
        }
    }
}

impl Drop for DrainLease {
    fn drop(&mut self) {
        if self.inner.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.inner.notify_empty.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DrainController;
    use std::time::Duration;

    #[tokio::test]
    async fn drain_waits_until_active_leases_drop() {
        let drain = DrainController::default();
        let lease = drain.try_acquire().expect("first lease allowed");
        drain.start_draining();
        assert!(drain.try_acquire().is_none());

        let waiter = tokio::spawn({
            let drain = drain.clone();
            async move { drain.wait_empty(Duration::from_secs(1)).await }
        });
        drop(lease);
        assert!(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn drain_rejects_new_leases_after_shutdown() {
        let drain = DrainController::default();

        assert!(drain.try_acquire().is_some());
        drain.start_draining();

        assert!(drain.is_draining());
        assert!(drain.try_acquire().is_none());
    }

    #[tokio::test]
    async fn drain_wait_empty_times_out_while_lease_is_active() {
        let drain = DrainController::default();
        let _lease = drain.try_acquire().expect("lease allowed");
        drain.start_draining();

        assert!(!drain.wait_empty(Duration::from_millis(10)).await);
        assert_eq!(drain.active(), 1);
    }
}
