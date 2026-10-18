//! Events published by the manager. Subscribers that fall behind get `Lagged` from the
//! broadcast channel and must re-read a full snapshot.
use serde::Serialize;
use tokio::sync::broadcast;

use crate::models::{JobSnapshot, ProgressSnapshot};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    JobChanged { snapshot: JobSnapshot },
    JobProgress { progress: ProgressSnapshot },
    JobRemoved { job_id: String },
    JobLog { job_id: String, level: String, message: String },
    QueueChanged { queue_id: String, running: bool },
    /// A queue finished; the app performs the configured action (shutdown, sleep, exit...).
    QueueFinished { queue_id: String, action: String },
    /// The download quota was reached; downloads stopped and resume automatically later.
    QuotaExceeded { used: u64, limit: u64, resume_in_secs: f64 },
    QuotaReleased,
}

#[derive(Clone)]
pub struct EventHub {
    tx: broadcast::Sender<Event>,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(8192);
        EventHub { tx }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
    pub fn publish(&self, e: Event) {
        let _ = self.tx.send(e);
    }
}
