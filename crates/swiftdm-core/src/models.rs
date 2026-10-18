//! Job domain model. A Job is owned and mutated only by the manager actor; everyone else
//! sees immutable snapshots.
use serde::{Deserialize, Serialize};

use crate::errors::ErrorInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobState {
    Queued,
    Starting,
    Downloading,
    Postprocessing,
    Finalizing,
    Completed,
    Pausing,
    Paused,
    RetryWait,
    Cancelling,
    Cancelled,
    Failed,
}

impl JobState {
    pub const ALL: [JobState; 12] = [
        JobState::Queued,
        JobState::Starting,
        JobState::Downloading,
        JobState::Postprocessing,
        JobState::Finalizing,
        JobState::Completed,
        JobState::Pausing,
        JobState::Paused,
        JobState::RetryWait,
        JobState::Cancelling,
        JobState::Cancelled,
        JobState::Failed,
    ];
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Queued => "QUEUED",
            JobState::Starting => "STARTING",
            JobState::Downloading => "DOWNLOADING",
            JobState::Postprocessing => "POSTPROCESSING",
            JobState::Finalizing => "FINALIZING",
            JobState::Completed => "COMPLETED",
            JobState::Pausing => "PAUSING",
            JobState::Paused => "PAUSED",
            JobState::RetryWait => "RETRY_WAIT",
            JobState::Cancelling => "CANCELLING",
            JobState::Cancelled => "CANCELLED",
            JobState::Failed => "FAILED",
        }
    }
    pub fn parse(s: &str) -> Option<JobState> {
        JobState::ALL.iter().copied().find(|x| x.as_str() == s)
    }
    /// A worker may exist for jobs in these states.
    pub fn running(&self) -> bool {
        matches!(
            self,
            JobState::Starting
                | JobState::Downloading
                | JobState::Postprocessing
                | JobState::Finalizing
                | JobState::Pausing
                | JobState::Cancelling
        )
    }
    pub fn active(&self) -> bool {
        matches!(
            self,
            JobState::Starting | JobState::Downloading | JobState::Postprocessing | JobState::Finalizing
        )
    }
    pub fn terminal(&self) -> bool {
        matches!(self, JobState::Completed | JobState::Cancelled)
    }
}

pub const MEDIA_PRESETS: [&str; 11] = ["best", "2160", "1440", "1080", "720", "480", "360", "mp4", "mkv", "mp3", "m4a"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaOptions {
    pub preset: String,
    #[serde(default)]
    pub max_height: Option<u32>,
    /// Exact yt-dlp format id chosen by the user in the format picker (overrides preset selection).
    #[serde(default)]
    pub format_id: Option<String>,
}

impl MediaOptions {
    pub fn preset(p: &str) -> Self {
        let p = if MEDIA_PRESETS.contains(&p) { p } else { "best" };
        MediaOptions { preset: p.into(), max_height: None, format_id: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSpec {
    pub url: String,
    pub dest_dir: String,
    #[serde(default = "auto")]
    pub kind: String, // auto | http | media
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub media: Option<MediaOptions>,
    #[serde(default = "ui")]
    pub source: String, // ui | extension | clipboard | batch
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "best")]
    pub format_label: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub queue_id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub speed_limit: Option<u64>,
    /// Cookies forwarded by the browser extension. Never serialized with the spec: stored
    /// DPAPI-encrypted in its own column and never logged.
    #[serde(skip)]
    pub cookies: Option<String>,
}

fn auto() -> String {
    "auto".into()
}
fn ui() -> String {
    "ui".into()
}
fn best() -> String {
    "best".into()
}

impl JobSpec {
    pub fn new(url: impl Into<String>, dest_dir: impl Into<String>) -> Self {
        JobSpec {
            url: url.into(),
            dest_dir: dest_dir.into(),
            kind: auto(),
            filename: None,
            media: None,
            source: ui(),
            referer: None,
            priority: 0,
            format_label: best(),
            category: None,
            queue_id: None,
            description: None,
            speed_limit: None,
            cookies: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub idx: u32,
    pub start: u64,
    /// Inclusive; -1 = unknown length (single stream).
    pub end: i64,
    /// Bytes durably written from `start`.
    pub committed: u64,
}

impl Segment {
    pub fn new(idx: u32, start: u64, end: i64) -> Self {
        Segment { idx, start, end, committed: 0 }
    }
    pub fn length(&self) -> Option<u64> {
        (self.end >= 0).then(|| self.end as u64 - self.start + 1)
    }
    pub fn done(&self) -> bool {
        self.length().is_some_and(|l| self.committed >= l)
    }
    pub fn next_offset(&self) -> u64 {
        self.start + self.committed
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub spec: JobSpec,
    pub state: JobState,
    pub reason: Option<String>,
    pub version: u64,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub final_url: Option<String>,
    pub reserved_name: Option<String>,
    pub temp_path: Option<String>,
    pub artifact_path: Option<String>,
    pub final_path: Option<String>,
    pub total_bytes: Option<u64>,
    pub committed_bytes: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub accept_ranges: Option<bool>,
    pub size_verified: Option<bool>,
    pub media: Option<serde_json::Value>,
    pub error: Option<ErrorInfo>,
    pub attempts: u32,
    pub created_at: f64,
    pub updated_at: f64,
    pub completed_at: Option<f64>,
    pub last_try: Option<f64>,
}

impl Job {
    pub fn new(id: String, spec: JobSpec) -> Self {
        let now = crate::now();
        Job {
            id,
            spec,
            state: JobState::Queued,
            reason: None,
            version: 0,
            kind: None,
            title: None,
            final_url: None,
            reserved_name: None,
            temp_path: None,
            artifact_path: None,
            final_path: None,
            total_bytes: None,
            committed_bytes: 0,
            etag: None,
            last_modified: None,
            content_type: None,
            accept_ranges: None,
            size_verified: None,
            media: None,
            error: None,
            attempts: 0,
            created_at: now,
            updated_at: now,
            completed_at: None,
            last_try: None,
        }
    }
    pub fn dest_dir(&self) -> &str {
        &self.spec.dest_dir
    }
}

/// Fields a worker may change (sent to the manager, which persists them).
#[derive(Debug, Clone, Default)]
pub struct JobPatch {
    pub kind: Option<String>,
    pub title: Option<String>,
    pub final_url: Option<String>,
    pub reserved_name: Option<String>,
    pub temp_path: Option<String>,
    pub total_bytes: Option<Option<u64>>,
    pub committed_bytes: Option<u64>,
    pub etag: Option<Option<String>>,
    pub last_modified: Option<Option<String>>,
    pub content_type: Option<Option<String>>,
    pub accept_ranges: Option<bool>,
    pub media: Option<serde_json::Value>,
}

impl JobPatch {
    pub fn apply(&self, job: &mut Job) {
        if let Some(v) = &self.kind {
            job.kind = Some(v.clone());
        }
        if let Some(v) = &self.title {
            job.title = Some(v.clone());
        }
        if let Some(v) = &self.final_url {
            job.final_url = Some(v.clone());
        }
        if let Some(v) = &self.reserved_name {
            job.reserved_name = Some(v.clone());
        }
        if let Some(v) = &self.temp_path {
            job.temp_path = Some(v.clone());
        }
        if let Some(v) = self.total_bytes {
            job.total_bytes = v;
        }
        if let Some(v) = self.committed_bytes {
            job.committed_bytes = v;
        }
        if let Some(v) = &self.etag {
            job.etag = v.clone();
        }
        if let Some(v) = &self.last_modified {
            job.last_modified = v.clone();
        }
        if let Some(v) = &self.content_type {
            job.content_type = v.clone();
        }
        if let Some(v) = self.accept_ranges {
            job.accept_ranges = Some(v);
        }
        if let Some(v) = &self.media {
            job.media = Some(v.clone());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SegmentProgress {
    pub idx: u32,
    pub start: u64,
    pub end: i64,
    pub downloaded: u64,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressSnapshot {
    pub job_id: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Bytes/s measured from real bytes; None = unknown.
    pub speed: Option<f64>,
    pub eta: Option<f64>,
    pub phase: String,
    pub segments: Vec<SegmentProgress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSnapshot {
    pub id: String,
    pub url: String,
    pub title: String,
    pub file_name: Option<String>,
    pub state: JobState,
    pub reason: Option<String>,
    pub kind: Option<String>,
    pub dest_dir: String,
    pub final_path: Option<String>,
    pub total: Option<u64>,
    pub downloaded: u64,
    pub error: Option<ErrorInfo>,
    pub format_label: String,
    pub source: String,
    pub category: Option<String>,
    pub queue_id: Option<String>,
    pub description: Option<String>,
    /// Where the download was started from (IDM: "parent web page" / referer).
    pub referer: Option<String>,
    /// URL after redirects.
    pub final_url: Option<String>,
    pub resumable: Option<bool>,
    /// Per-download speed limit in bytes/s.
    pub speed_limit: Option<u64>,
    pub priority: i32,
    pub created_at: f64,
    pub completed_at: Option<f64>,
    pub last_try: Option<f64>,
    pub attempts: u32,
    pub version: u64,
}

impl JobSnapshot {
    pub fn of(job: &Job, downloaded: Option<u64>) -> Self {
        let file_name = job
            .final_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name().map(|s| s.to_string_lossy().into_owned()))
            .or_else(|| job.reserved_name.clone())
            .or_else(|| job.spec.filename.clone());
        let title = job
            .title
            .clone()
            .or_else(|| file_name.clone())
            .unwrap_or_else(|| job.spec.url.clone());
        JobSnapshot {
            id: job.id.clone(),
            url: job.spec.url.clone(),
            title,
            file_name,
            state: job.state,
            reason: job.reason.clone(),
            kind: job.kind.clone(),
            dest_dir: job.spec.dest_dir.clone(),
            final_path: job.final_path.clone(),
            total: job.total_bytes,
            downloaded: downloaded.unwrap_or(job.committed_bytes),
            error: job.error.clone(),
            format_label: job.spec.format_label.clone(),
            source: job.spec.source.clone(),
            category: job.spec.category.clone(),
            queue_id: job.spec.queue_id.clone(),
            description: job.spec.description.clone(),
            referer: job.spec.referer.clone(),
            final_url: job.final_url.clone(),
            resumable: job.accept_ranges,
            speed_limit: job.spec.speed_limit,
            priority: job.spec.priority,
            created_at: job.created_at,
            completed_at: job.completed_at,
            last_try: job.last_try,
            attempts: job.attempts,
            version: job.version,
        }
    }
}
