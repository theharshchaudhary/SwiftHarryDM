//! Cooperative cancellation shared between the manager and an engine run.
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StopReason {
    Pause = 0,
    Shutdown = 1,
    Cancel = 2,
}

#[derive(Clone, Default)]
pub struct CancelToken {
    token: CancellationToken,
    reason: Arc<Mutex<Option<StopReason>>>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    /// Request a stop. A stronger reason (cancel > shutdown > pause) upgrades a weaker one.
    pub fn set(&self, reason: StopReason) {
        let mut r = self.reason.lock().unwrap();
        if r.is_some_and(|cur| reason <= cur) {
            return;
        }
        *r = Some(reason);
        drop(r);
        self.token.cancel();
    }
    pub fn reason(&self) -> Option<StopReason> {
        *self.reason.lock().unwrap()
    }
    pub fn is_set(&self) -> bool {
        self.token.is_cancelled()
    }
    pub async fn cancelled(&self) {
        self.token.cancelled().await
    }
    /// Sleep for `d` unless cancelled first. Returns true if cancelled.
    pub async fn sleep(&self, d: std::time::Duration) -> bool {
        tokio::select! {
            _ = self.token.cancelled() => true,
            _ = tokio::time::sleep(d) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upgrade_only() {
        let t = CancelToken::new();
        t.set(StopReason::Pause);
        t.set(StopReason::Cancel);
        t.set(StopReason::Shutdown);
        assert_eq!(t.reason(), Some(StopReason::Cancel));
        assert!(t.is_set());
    }
}
