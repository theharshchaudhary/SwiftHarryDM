//! Engine interface shared by the native HTTP engine and the media (yt-dlp) engine.
use std::sync::Arc;

use crate::cancel::{CancelToken, StopReason};
use crate::errors::{ErrorInfo, SdmError};
use crate::limiter::Limiter;
use crate::models::{Job, JobPatch, ProgressSnapshot};
use crate::settings::SettingsStore;

#[derive(Debug, Clone, Default)]
pub struct ProbeResult {
    pub patch: JobPatch,
    /// Reserve this name before downloading.
    pub desired_name: Option<String>,
    /// e.g. "media" when an HTTP URL turned out to be a web page.
    pub reroute: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Completed { artifact: String, size: u64, verified: bool },
    Stopped(StopReason),
    Failed(ErrorInfo),
}

/// Error type inside engines: either a stop request or a real failure.
#[derive(Debug)]
pub enum EngineError {
    Stop(StopReason),
    Fail(SdmError),
}

impl From<SdmError> for EngineError {
    fn from(e: SdmError) -> Self {
        EngineError::Fail(e)
    }
}

pub type EResult<T> = Result<T, EngineError>;

pub trait Reporter: Send + Sync {
    fn progress(&self, p: ProgressSnapshot);
    fn update(&self, job_id: &str, patch: JobPatch);
    fn phase(&self, job_id: &str, phase: &str);
    fn log(&self, job_id: &str, level: &str, message: &str);
}

#[derive(Clone)]
pub struct RunCtx {
    pub cancel: CancelToken,
    pub reporter: Arc<dyn Reporter>,
    pub settings: SettingsStore,
    pub limiter: Arc<Limiter>,
}

impl RunCtx {
    pub fn check(&self) -> EResult<()> {
        if self.cancel.is_set() {
            Err(EngineError::Stop(self.cancel.reason().unwrap_or(StopReason::Pause)))
        } else {
            Ok(())
        }
    }
    pub fn stop_err(&self) -> EngineError {
        EngineError::Stop(self.cancel.reason().unwrap_or(StopReason::Pause))
    }
}

#[async_trait::async_trait]
pub trait Engine: Send + Sync {
    fn name(&self) -> &'static str;
    async fn probe(&self, job: &Job, ctx: &RunCtx) -> EResult<ProbeResult>;
    async fn run(&self, job: &mut Job, ctx: &RunCtx) -> EResult<Outcome>;
    /// Delete partial data (cancel). Must be idempotent.
    fn discard(&self, job: &Job);
    /// Remove leftovers after a successful finalize. Must be idempotent.
    fn cleanup(&self, job: &Job);
}
