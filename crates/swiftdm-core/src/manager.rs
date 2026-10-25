//! JobManager: the single owner of job state (an actor on the tokio runtime).
//!
//! UI calls, API calls, worker results, retry timers and schedule ticks are all messages,
//! so transitions are serialized; every one is persisted before its event is published.
//! Workers never mutate shared state: they send patches and an Outcome.
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Reverse;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::cancel::{CancelToken, StopReason};
use crate::engine::{Engine, EngineError, Outcome, Reporter, RunCtx};
use crate::errors::{from_io, ErrorCode, ErrorInfo, SdmError};
use crate::events::{Event, EventHub};
use crate::fs::collisions::{finalize_move, move_path, Reservations};
use crate::fs::names::sanitize_component;
use crate::limiter::Limiter;
use crate::models::{Job, JobPatch, JobSnapshot, JobSpec, JobState, ProgressSnapshot};
use crate::scheduler::{self, Queue, Tick, MAIN_QUEUE};
use crate::settings::SettingsStore;
use crate::states;
use crate::store::{Store, StoreError};

use JobState as S;

pub const QUEUED_REASON: &str = "queued";

pub fn new_job_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn validate_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.len() > 8192 {
        return Err("The URL is too long.".into());
    }
    match url::Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty()) => {
            Ok(url.to_string())
        }
        _ => Err("Enter a full http:// or https:// address.".into()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    /// Start as soon as a slot is free.
    Now,
    /// Add to a queue without starting ("Download later").
    Later,
}

type InspectFn = Box<dyn FnOnce(&Actor) -> Box<dyn std::any::Any + Send> + Send>;

enum Cmd {
    Submit(Job, StartMode),
    Pause(String),
    Resume(String),
    Cancel(String),
    Retry(String),
    Remove(String, bool),
    PauseAll,
    ResumeAll,
    StartQueue(String),
    StopQueue(String),
    SaveQueue(Queue),
    DeleteQueue(String),
    MoveToQueue(String, Option<String>),
    SetDescription(String, Option<String>),
    SetSpeedLimit(String, Option<u64>),
    SetPriority(String, i32),
    /// New folder and/or file name; resume afterwards if it was downloading.
    Relocate(String, String, Option<String>),
    ApplyRefresh(String, String, crate::http::probe::HttpProbe, oneshot::Sender<Result<(), String>>),
    Update(String, JobPatch),
    Phase(String, String),
    Done(String, Outcome),
    ScheduleTick(u32, usize),
    Sync(oneshot::Sender<()>),
    Inspect(InspectFn, oneshot::Sender<Box<dyn std::any::Any + Send>>),
    Shutdown(Duration, oneshot::Sender<()>),
    Halt(oneshot::Sender<Vec<(tokio::task::JoinHandle<()>, CancelToken)>>),
}

struct Shared {
    snaps: RwLock<HashMap<String, JobSnapshot>>,
    progress: RwLock<HashMap<String, ProgressSnapshot>>,
    queues: RwLock<Vec<Queue>>,
    quota: RwLock<QuotaStatus>,
}

/// Current use of the download quota, for the status bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct QuotaStatus {
    pub used: u64,
    /// 0 = no quota.
    pub limit: u64,
    pub blocked: bool,
}

#[derive(Clone)]
pub struct Manager {
    tx: mpsc::UnboundedSender<Cmd>,
    shared: Arc<Shared>,
    hub: EventHub,
    engines: Arc<HashMap<String, Arc<dyn Engine>>>,
    settings: SettingsStore,
    handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

struct ChannelReporter {
    tx: mpsc::UnboundedSender<Cmd>,
    shared: Arc<Shared>,
    hub: EventHub,
}

impl Reporter for ChannelReporter {
    fn progress(&self, p: ProgressSnapshot) {
        self.shared.progress.write().unwrap().insert(p.job_id.clone(), p.clone());
        self.hub.publish(Event::JobProgress { progress: p });
    }
    fn update(&self, job_id: &str, patch: JobPatch) {
        let _ = self.tx.send(Cmd::Update(job_id.into(), patch));
    }
    fn phase(&self, job_id: &str, phase: &str) {
        let _ = self.tx.send(Cmd::Phase(job_id.into(), phase.into()));
    }
    fn log(&self, job_id: &str, level: &str, message: &str) {
        log_event(&self.hub, job_id, level, message);
    }
}

fn log_event(hub: &EventHub, job_id: &str, level: &str, message: &str) {
    let short = &job_id[..job_id.len().min(8)];
    match level {
        "error" => tracing::error!("job={short} {message}"),
        "warning" => tracing::warn!("job={short} {message}"),
        _ => tracing::info!("job={short} {message}"),
    }
    hub.publish(Event::JobLog { job_id: job_id.into(), level: level.into(), message: message.into() });
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UrlInfo {
    pub file_name: String,
    pub size: Option<u64>,
    pub content_type: Option<String>,
    pub resumable: bool,
    pub final_url: String,
    /// The address opens a web page; the media engine may still find a video on it.
    pub is_web_page: bool,
    pub category: Option<String>,
    /// Folder the file would be saved to (its category folder).
    pub folder: String,
}

pub struct ManagerConfig {
    pub retry_base: Duration,
    pub autostart: bool,
    /// Run the internal scheduler ticker (disabled in tests that drive ticks manually).
    pub scheduler_ticker: bool,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        ManagerConfig { retry_base: Duration::from_secs(5), autostart: true, scheduler_ticker: true }
    }
}

impl Manager {
    pub fn start(
        store: Arc<Store>,
        engines: HashMap<String, Arc<dyn Engine>>,
        settings: SettingsStore,
        config: ManagerConfig,
    ) -> Result<Manager, StoreError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            snaps: RwLock::new(HashMap::new()),
            progress: RwLock::new(HashMap::new()),
            queues: RwLock::new(vec![]),
            quota: RwLock::new(QuotaStatus::default()),
        });
        let hub = EventHub::new();
        let engines = Arc::new(engines);
        let limiter = Arc::new(Limiter::new(settings.get().speed_limit));
        let reporter = Arc::new(ChannelReporter { tx: tx.clone(), shared: shared.clone(), hub: hub.clone() });
        let mut actor = Actor {
            store,
            engines: engines.clone(),
            settings: settings.clone(),
            hub: hub.clone(),
            shared: shared.clone(),
            res: Reservations::new(),
            jobs: HashMap::new(),
            workers: HashMap::new(),
            via_queue: HashMap::new(),
            requeue_on_stop: HashSet::new(),
            pending_remove: HashMap::new(),
            queues: vec![],
            timers: BinaryHeap::new(),
            seq: 0,
            stopping: None,
            limiter,
            reporter,
            tx: tx.clone(),
            config,
            last_tick: None,
            quota: crate::quota::QuotaWindow::new(crate::now(), 0),
            quota_blocked: false,
            quota_paused: HashSet::new(),
            job_limiters: HashMap::new(),
            pending_relocate: HashMap::new(),
        };
        actor.recover()?;
        let mgr = Manager {
            tx,
            shared,
            hub,
            engines,
            settings,
            handle: Arc::new(std::sync::Mutex::new(None)),
        };
        if actor.config.autostart {
            let ticker = actor.config.scheduler_ticker;
            let h = tokio::spawn(actor.run(rx));
            *mgr.handle.lock().unwrap() = Some(h);
            if ticker {
                let t = mgr.tx.clone();
                tokio::spawn(async move {
                    let mut iv = tokio::time::interval(Duration::from_secs(20));
                    loop {
                        iv.tick().await;
                        let (m, d) = scheduler::local_now();
                        if t.send(Cmd::ScheduleTick(m, d)).is_err() {
                            break;
                        }
                    }
                });
            }
        }
        Ok(mgr)
    }

    // ------------------------------------------------------------------ public API
    pub fn submit(&self, mut spec: JobSpec, mode: StartMode) -> Result<String, String> {
        spec.url = validate_url(&spec.url)?;
        if spec.dest_dir.trim().is_empty() {
            return Err("No download folder selected.".into());
        }
        spec.dest_dir = spec.dest_dir.replace('/', "\\");
        if mode == StartMode::Later && spec.queue_id.is_none() {
            spec.queue_id = Some(MAIN_QUEUE.into());
        }
        let job = Job::new(new_job_id(), spec);
        let id = job.id.clone();
        let _ = self.tx.send(Cmd::Submit(job, mode));
        Ok(id)
    }
    pub fn pause(&self, id: &str) {
        let _ = self.tx.send(Cmd::Pause(id.into()));
    }
    pub fn resume(&self, id: &str) {
        let _ = self.tx.send(Cmd::Resume(id.into()));
    }
    pub fn cancel(&self, id: &str) {
        let _ = self.tx.send(Cmd::Cancel(id.into()));
    }
    pub fn retry(&self, id: &str) {
        let _ = self.tx.send(Cmd::Retry(id.into()));
    }
    pub fn remove(&self, id: &str, delete_file: bool) {
        let _ = self.tx.send(Cmd::Remove(id.into(), delete_file));
    }
    pub fn pause_all(&self) {
        let _ = self.tx.send(Cmd::PauseAll);
    }
    pub fn resume_all(&self) {
        let _ = self.tx.send(Cmd::ResumeAll);
    }
    pub fn start_queue(&self, q: &str) {
        let _ = self.tx.send(Cmd::StartQueue(q.into()));
    }
    pub fn stop_queue(&self, q: &str) {
        let _ = self.tx.send(Cmd::StopQueue(q.into()));
    }
    pub fn save_queue(&self, q: Queue) {
        let _ = self.tx.send(Cmd::SaveQueue(q));
    }
    pub fn delete_queue(&self, q: &str) {
        let _ = self.tx.send(Cmd::DeleteQueue(q.into()));
    }
    pub fn move_to_queue(&self, id: &str, q: Option<String>) {
        let _ = self.tx.send(Cmd::MoveToQueue(id.into(), q));
    }
    pub fn set_description(&self, id: &str, d: Option<String>) {
        let _ = self.tx.send(Cmd::SetDescription(id.into(), d));
    }
    /// Per-download speed limit in bytes/s (None or 0 = no limit); applies immediately.
    pub fn set_speed_limit(&self, id: &str, limit: Option<u64>) {
        let _ = self.tx.send(Cmd::SetSpeedLimit(id.into(), limit.filter(|l| *l > 0)));
    }
    /// Higher priority starts first within its queue (used to reorder queue files).
    /// Save a download to another folder and/or under another name, also while it downloads
    /// (it is paused, its data moved, and resumed) or after it finished (the file is moved).
    /// IDM starts downloading while "Download File Info" is open; this applies the choices
    /// made there afterwards.
    pub fn relocate(&self, id: &str, dir: &str, name: Option<String>) {
        let _ = self.tx.send(Cmd::Relocate(id.into(), dir.into(), name));
    }
    pub fn set_priority(&self, id: &str, priority: i32) {
        let _ = self.tx.send(Cmd::SetPriority(id.into(), priority));
    }

    /// What the "Download File Info" dialog shows before a download is added.
    pub async fn inspect_url(&self, url: &str, cookies: Option<String>) -> Result<UrlInfo, String> {
        let url = validate_url(url)?;
        let s = self.settings.get();
        let p = crate::http::probe::probe_url(&url, &s, &CancelToken::new(), cookies).await.map_err(|e| e.0.message)?;
        let name = crate::fs::names::choose_filename(None, p.content_disposition.as_deref(), Some(&p.final_url), p.content_type.as_deref());
        let (folder, category) = s.folder_for(&name, Some(&p.final_url));
        Ok(UrlInfo {
            file_name: name,
            size: p.total,
            content_type: p.content_type.clone(),
            resumable: p.ranges,
            final_url: p.final_url.clone(),
            is_web_page: p.is_html(),
            category,
            folder: folder.to_string_lossy().into_owned(),
        })
    }

    /// IDM's "Zip preview": the files inside a ZIP archive on a server, read with range
    /// requests (no download).
    pub async fn zip_preview(&self, url: &str, total: u64, cookies: Option<String>) -> Result<Vec<crate::http::zip_preview::ZipEntry>, String> {
        let url = validate_url(url)?;
        crate::http::zip_preview::preview(&url, &self.settings.get(), cookies, total).await
    }

    pub fn schedule_tick(&self, minute: u32, weekday: usize) {
        let _ = self.tx.send(Cmd::ScheduleTick(minute, weekday));
    }

    /// IDM "Refresh download address": point an unfinished download at a new link, keeping the
    /// partial data only if the new link serves the same file (same size).
    pub async fn refresh_url(&self, id: &str, new_url: &str) -> Result<(), String> {
        let new_url = validate_url(new_url)?;
        let snap = self.get(id).ok_or("Unknown download.")?;
        if snap.kind.as_deref() == Some("media") {
            return Err("Media downloads fetch fresh links automatically; refreshing is not needed.".into());
        }
        let probe = crate::http::probe::probe_url(&new_url, &self.settings.get(), &CancelToken::new(), None)
            .await
            .map_err(|e| e.0.message)?;
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(Cmd::ApplyRefresh(id.into(), new_url, probe, tx));
        rx.await.map_err(|_| "The download manager is shutting down.".to_string())?
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.hub.subscribe()
    }
    pub fn hub(&self) -> &EventHub {
        &self.hub
    }
    pub fn engine(&self, name: &str) -> Option<Arc<dyn Engine>> {
        self.engines.get(name).cloned()
    }
    pub fn settings(&self) -> &SettingsStore {
        &self.settings
    }
    pub fn snapshot(&self) -> Vec<JobSnapshot> {
        let mut v: Vec<_> = self.shared.snaps.read().unwrap().values().cloned().collect();
        v.sort_by(|a, b| a.created_at.total_cmp(&b.created_at).then(a.id.cmp(&b.id)));
        v
    }
    pub fn get(&self, id: &str) -> Option<JobSnapshot> {
        self.shared.snaps.read().unwrap().get(id).cloned()
    }
    pub fn progress(&self, id: &str) -> Option<ProgressSnapshot> {
        self.shared.progress.read().unwrap().get(id).cloned()
    }
    pub fn queues(&self) -> Vec<Queue> {
        self.shared.queues.read().unwrap().clone()
    }
    pub fn quota_status(&self) -> QuotaStatus {
        *self.shared.quota.read().unwrap()
    }

    /// `submit`, following IDM's duplicate-link choices when the address is already in the
    /// list: "number" (and "ask", when there is no one to ask) adds it under a numbered name,
    /// "overwrite" replaces the existing download and its file, "resume" uses the existing
    /// download (resumed if unfinished). Returns the download's id and whether it is the
    /// existing one.
    pub fn submit_or_reuse(&self, mut spec: JobSpec, mode: StartMode, policy: &str) -> Result<(String, bool), String> {
        let dups = self.find_duplicates(&spec.url);
        match (dups.first(), policy) {
            (Some(d), "resume") => {
                if d.state != JobState::Completed && mode == StartMode::Now {
                    self.resume(&d.id);
                    self.retry(&d.id);
                }
                Ok((d.id.clone(), true))
            }
            (Some(d), "overwrite") => {
                if spec.filename.is_none() {
                    spec.filename = d.final_path.as_deref().and_then(|p| Path::new(p).file_name()).map(|n| n.to_string_lossy().into_owned());
                }
                for d in &dups {
                    self.remove(&d.id, true);
                }
                self.submit(spec, mode).map(|id| (id, false))
            }
            _ => self.submit(spec, mode).map(|id| (id, false)),
        }
    }

    /// Downloads with the same address (as typed or after redirects) that were not cancelled,
    /// so the UI can offer the duplicate-link choices.
    pub fn find_duplicates(&self, url: &str) -> Vec<JobSnapshot> {
        let url = url.trim();
        self.snapshot()
            .into_iter()
            .filter(|s| s.state != JobState::Cancelled && (s.url == url || s.final_url.as_deref() == Some(url)))
            .collect()
    }

    pub fn running_count(&self) -> usize {
        self.shared.snaps.read().unwrap().values().filter(|s| s.state.running()).count()
    }

    pub async fn sync(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Cmd::Sync(tx)).is_ok() {
            let _ = rx.await;
        }
    }

    pub async fn wait_for(&self, id: &str, wanted: &[JobState], timeout: Duration) -> Option<JobSnapshot> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(s) = self.get(id) {
                if wanted.contains(&s.state) {
                    return Some(s);
                }
            }
            if Instant::now() >= deadline {
                return self.get(id);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Run `f` on the actor (tests/diagnostics).
    pub async fn inspect<T: Send + 'static>(&self, f: impl FnOnce(&Actor) -> T + Send + 'static) -> T {
        let (tx, rx) = oneshot::channel();
        let f: InspectFn = Box::new(move |a| Box::new(f(a)));
        let _ = self.tx.send(Cmd::Inspect(f, tx));
        *rx.await.expect("manager stopped").downcast::<T>().unwrap()
    }

    /// Pause everything, persist, stop the actor.
    pub async fn shutdown(&self, timeout: Duration) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Cmd::Shutdown(timeout, tx)).is_ok() {
            let _ = rx.await;
        }
        let h = self.handle.lock().unwrap().take();
        if let Some(h) = h {
            let _ = h.await;
        }
    }

    /// Simulate a crash: stop immediately without persisting anything further.
    pub async fn halt_for_tests(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Cmd::Halt(tx)).is_ok() {
            if let Ok(workers) = rx.await {
                for (h, t) in workers {
                    t.set(StopReason::Shutdown);
                    let _ = h.await;
                }
            }
        }
        let h = self.handle.lock().unwrap().take();
        if let Some(h) = h {
            let _ = h.await;
        }
    }
}

pub struct Actor {
    store: Arc<Store>,
    engines: Arc<HashMap<String, Arc<dyn Engine>>>,
    settings: SettingsStore,
    hub: EventHub,
    shared: Arc<Shared>,
    res: Reservations,
    pub jobs: HashMap<String, Job>,
    pub workers: HashMap<String, (tokio::task::JoinHandle<()>, CancelToken)>,
    pub via_queue: HashMap<String, String>,
    requeue_on_stop: HashSet<String>,
    pending_remove: HashMap<String, bool>,
    pub queues: Vec<Queue>,
    timers: BinaryHeap<Reverse<(Instant, u64, String)>>,
    seq: u64,
    stopping: Option<(Instant, oneshot::Sender<()>)>,
    limiter: Arc<Limiter>,
    reporter: Arc<ChannelReporter>,
    tx: mpsc::UnboundedSender<Cmd>,
    config: ManagerConfig,
    last_tick: Option<u32>,
    quota: crate::quota::QuotaWindow,
    quota_blocked: bool,
    /// Downloads stopped by the quota (not queue members), resumed when it frees up.
    quota_paused: HashSet<String>,
    job_limiters: HashMap<String, Arc<Limiter>>,
    /// Relocations waiting for a running download to stop: (folder, name, resume afterwards).
    pending_relocate: HashMap<String, (String, Option<String>, bool)>,
}

impl Actor {
    pub fn store(&self) -> &Store {
        &self.store
    }
    pub fn max_concurrent(&self) -> usize {
        self.settings.get().max_concurrent as usize
    }

    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Cmd>) {
        loop {
            let next_timer = self.timers.peek().map(|Reverse((t, _, _))| *t);
            let deadline = next_timer.unwrap_or_else(|| Instant::now() + Duration::from_millis(250));
            let deadline = deadline.min(Instant::now() + Duration::from_millis(250));
            let cmd = tokio::select! {
                c = rx.recv() => c,
                _ = tokio::time::sleep_until(deadline) => None,
            };
            if let Some(cmd) = cmd {
                match cmd {
                    Cmd::Halt(reply) => {
                        let workers = self.workers.drain().map(|(_, w)| w).collect();
                        let _ = reply.send(workers);
                        return;
                    }
                    other => self.handle(other),
                }
            } else if rx.is_closed() && self.workers.is_empty() {
                return;
            }
            self.fire_timers();
            if let Some((deadline, _)) = &self.stopping {
                if self.workers.is_empty() || Instant::now() >= *deadline {
                    self.finish_shutdown();
                    return;
                }
            } else {
                self.limiter.set_rate(self.settings.get().speed_limit);
                self.check_quota();
                self.schedule();
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Submit(job, mode) => self.on_submit(job, mode),
            Cmd::Pause(id) => self.on_pause(&id),
            Cmd::Resume(id) => self.on_resume(&id),
            Cmd::Cancel(id) => self.on_cancel(&id),
            Cmd::Retry(id) => self.on_retry(&id),
            Cmd::Remove(id, del) => self.on_remove(&id, del),
            Cmd::PauseAll => {
                let ids: Vec<String> = self.jobs.keys().cloned().collect();
                for id in ids {
                    self.on_pause(&id);
                }
            }
            Cmd::ResumeAll => {
                let ids: Vec<String> = self
                    .jobs
                    .values()
                    .filter(|j| j.state == S::Paused && j.reason.as_deref() != Some(QUEUED_REASON))
                    .map(|j| j.id.clone())
                    .collect();
                for id in ids {
                    self.on_resume(&id);
                }
            }
            Cmd::StartQueue(q) => self.set_queue_running(&q, true),
            Cmd::StopQueue(q) => self.set_queue_running(&q, false),
            Cmd::SaveQueue(q) => self.on_save_queue(q),
            Cmd::DeleteQueue(q) => self.on_delete_queue(&q),
            Cmd::MoveToQueue(id, q) => self.on_move_to_queue(&id, q),
            Cmd::SetDescription(id, d) => {
                if let Some(mut j) = self.jobs.remove(&id) {
                    j.spec.description = d;
                    self.save(&mut j);
                    self.jobs.insert(id, j);
                }
            }
            Cmd::SetSpeedLimit(id, limit) => {
                if let Some(l) = self.job_limiters.get(&id) {
                    l.set_rate(limit.unwrap_or(0));
                }
                if let Some(mut j) = self.jobs.remove(&id) {
                    j.spec.speed_limit = limit;
                    self.save(&mut j);
                    self.jobs.insert(id, j);
                }
            }
            Cmd::Relocate(id, dir, name) => self.on_relocate(&id, dir, name),
            Cmd::SetPriority(id, p) => {
                if let Some(mut j) = self.jobs.remove(&id) {
                    j.spec.priority = p;
                    self.save(&mut j);
                    self.jobs.insert(id, j);
                }
            }
            Cmd::ApplyRefresh(id, url, probe, reply) => {
                let _ = reply.send(self.on_refresh(&id, url, probe));
            }
            Cmd::Update(id, patch) => {
                if let Some(mut j) = self.jobs.remove(&id) {
                    if !j.state.terminal() {
                        patch.apply(&mut j);
                        self.save(&mut j);
                    }
                    self.jobs.insert(id, j);
                }
            }
            Cmd::Phase(id, phase) => self.on_phase(&id, &phase),
            Cmd::Done(id, outcome) => self.on_done(&id, outcome),
            Cmd::ScheduleTick(m, d) => self.on_schedule_tick(m, d),
            Cmd::Sync(reply) => {
                let _ = reply.send(());
            }
            Cmd::Inspect(f, reply) => {
                let _ = reply.send(f(self));
            }
            Cmd::Shutdown(timeout, reply) => {
                self.stopping = Some((Instant::now() + timeout, reply));
                for (_, t) in self.workers.values() {
                    t.set(StopReason::Shutdown);
                }
            }
            Cmd::Halt(_) => unreachable!(),
        }
    }

    // ------------------------------------------------------------------ helpers
    fn set_state(&mut self, job: &mut Job, new: JobState, reason: Option<&str>, error: Option<ErrorInfo>, keep_error: bool) {
        if let Err(e) = states::check(job.state, new) {
            tracing::error!("BUG: {e} for job={}", &job.id[..8]);
            return;
        }
        job.state = new;
        job.reason = reason.map(Into::into);
        if error.is_some() {
            job.error = error;
        } else if !keep_error {
            job.error = None;
        }
        self.save(job);
    }

    fn save(&mut self, job: &mut Job) {
        job.version += 1;
        job.updated_at = crate::now();
        if let Err(e) = self.store.save_job(job) {
            tracing::error!("failed to persist job={}: {e}", &job.id[..8]);
        }
        self.publish(job);
    }

    fn publish(&self, job: &Job) {
        let downloaded = if job.state.running() {
            self.shared.progress.read().unwrap().get(&job.id).map(|p| p.downloaded)
        } else {
            None
        };
        let snap = JobSnapshot::of(job, downloaded);
        self.shared.snaps.write().unwrap().insert(job.id.clone(), snap.clone());
        self.hub.publish(Event::JobChanged { snapshot: snap });
    }

    fn log(&self, id: &str, level: &str, msg: &str) {
        log_event(&self.hub, id, level, msg);
    }

    fn overwrite(&self) -> bool {
        self.settings.get().collision_policy == "overwrite"
    }

    fn with_job(&mut self, id: &str, f: impl FnOnce(&mut Self, &mut Job)) {
        if let Some(mut j) = self.jobs.remove(id) {
            f(self, &mut j);
            if !j.id.is_empty() {
                self.jobs.insert(j.id.clone(), j);
            }
        }
    }

    fn publish_queues(&self) {
        *self.shared.queues.write().unwrap() = self.queues.clone();
    }

    fn persist_queues(&self) {
        for (i, q) in self.queues.iter().enumerate() {
            if let Err(e) = self.store.save_queue(q, i as i64) {
                tracing::error!("failed to persist queue {}: {e}", q.id);
            }
        }
        self.publish_queues();
    }

    // ------------------------------------------------------------------ recovery
    fn recover(&mut self) -> Result<(), StoreError> {
        self.queues = self.store.load_queues()?;
        if !self.queues.iter().any(|q| q.id == MAIN_QUEUE) {
            self.queues.insert(0, Queue::main());
        }
        // Queues stop on restart, like IDM; schedules start them again.
        for q in self.queues.iter_mut() {
            q.running = false;
        }
        self.persist_queues();
        let auto = self.settings.get().auto_resume;
        for mut job in self.store.load_jobs()? {
            if let (Some(name), false) = (job.reserved_name.clone(), job.state.terminal()) {
                if !self.res.adopt(&job.id, job.dest_dir(), &name) {
                    job.reserved_name = None;
                }
            }
            match job.state {
                S::Starting | S::Downloading | S::Postprocessing | S::Pausing | S::RetryWait => {
                    if job.state == S::Pausing {
                        self.set_state(&mut job, S::Paused, Some("paused"), None, true);
                    } else if auto {
                        self.set_state(&mut job, S::Queued, None, None, true);
                    } else {
                        self.set_state(&mut job, S::Paused, Some("interrupted"), None, true);
                    }
                }
                S::Finalizing => self.refinalize(&mut job),
                S::Cancelling => {
                    self.discard(&mut job);
                    self.set_state(&mut job, S::Cancelled, None, None, false);
                }
                S::Paused if job.reason.as_deref() == Some("interrupted") && auto => {
                    self.set_state(&mut job, S::Queued, None, None, true)
                }
                _ => self.publish(&job),
            }
            self.jobs.insert(job.id.clone(), job);
        }
        Ok(())
    }

    fn refinalize(&mut self, job: &mut Job) {
        if job.artifact_path.is_none() {
            if let Some(fp) = job.final_path.clone() {
                if Path::new(&fp).is_file() {
                    self.set_state(job, S::Completed, None, None, false);
                    return;
                }
            }
        }
        if let Some(a) = job.artifact_path.clone() {
            if let Ok(meta) = std::fs::metadata(crate::fs::long_path(&a)) {
                let size = meta.len();
                if job.kind.as_deref() == Some("http") && job.size_verified == Some(true) && job.total_bytes.is_some_and(|t| t != size) {
                    let e = SdmError::new(ErrorCode::Postprocess, "The downloaded file has the wrong size after a restart.");
                    self.set_state(job, S::Failed, None, Some(e.0), false);
                    return;
                }
                self.finalize(job, a, size, job.size_verified.unwrap_or(false));
                return;
            }
        }
        // The app may have stopped right after moving the file into place and before recording
        // it. The reserved name then holds a file of exactly the verified size, written after
        // the download was added: that is this download's file.
        if let (Some(name), Some(total), Some(true)) = (job.reserved_name.clone(), job.total_bytes, job.size_verified) {
            let candidate = Path::new(job.dest_dir()).join(&name);
            let meta = std::fs::metadata(crate::fs::long_path(&candidate));
            let fresh = meta.as_ref().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).is_some_and(|t| t.as_secs_f64() + 2.0 >= job.created_at);
            if meta.as_ref().is_ok_and(|m| m.is_file() && m.len() == total) && fresh {
                job.final_path = Some(candidate.to_string_lossy().into_owned());
                job.artifact_path = None;
                job.temp_path = None;
                job.committed_bytes = total;
                job.completed_at = Some(crate::now());
                self.log(&job.id, "info", "Finished moving the file into place after a restart.");
                self.set_state(job, S::Completed, None, None, false);
                self.res.release(&job.id);
                return;
            }
        }
        let e = SdmError::new(ErrorCode::Postprocess, "The app closed while finishing this download and the file was not found.");
        self.set_state(job, S::Failed, None, Some(e.0), false);
    }

    // ------------------------------------------------------------------ commands
    fn on_submit(&mut self, mut job: Job, mode: StartMode) {
        match mode {
            StartMode::Now => {
                job.state = S::Queued;
                job.reason = None;
            }
            StartMode::Later => {
                job.state = S::Paused;
                job.reason = Some(QUEUED_REASON.into());
            }
        }
        if let Some(name) = job.spec.filename.clone() {
            match self.res.reserve(&job.id, job.dest_dir(), &sanitize_component(&name), self.overwrite()) {
                Ok(n) => job.reserved_name = Some(n),
                Err(e) => tracing::warn!("reservation failed: {e}"),
            }
        }
        if let Err(e) = self.store.save_job(&job) {
            tracing::error!("failed to persist new job: {e}");
        }
        self.publish(&job);
        self.log(&job.id, "info", &format!("Added {}", job.spec.url));
        self.jobs.insert(job.id.clone(), job);
    }

    fn on_pause(&mut self, id: &str) {
        self.with_job(id, |me, j| match j.state {
            S::Queued | S::RetryWait => me.set_state(j, S::Paused, Some("paused"), None, true),
            S::Starting | S::Downloading | S::Postprocessing => {
                me.set_state(j, S::Pausing, Some("paused"), None, false);
                if let Some((_, t)) = me.workers.get(id) {
                    t.set(StopReason::Pause);
                }
            }
            _ => {}
        });
    }

    fn on_resume(&mut self, id: &str) {
        self.with_job(id, |me, j| {
            if j.state == S::Paused {
                j.attempts = 0;
                me.via_queue.remove(&j.id);
                me.set_state(j, S::Queued, None, None, false);
            }
        });
    }

    fn on_relocate(&mut self, id: &str, dir: String, name: Option<String>) {
        let Some(state) = self.jobs.get(id).map(|j| j.state) else { return };
        if state.running() {
            // Applied when the worker has stopped (see on_done). A download that was moving
            // data continues afterwards; one already being paused stays paused.
            let resume = matches!(state, S::Starting | S::Downloading | S::Postprocessing);
            self.pending_relocate.insert(id.into(), (dir, name, resume));
            if resume {
                self.on_pause(id);
            }
            return;
        }
        self.with_job(id, |me, j| {
            if let Err(e) = me.relocate_now(j, &dir, name) {
                me.log(&j.id, "error", &format!("Could not move the download to {dir}: {e}"));
            }
        });
    }

    fn relocate_now(&mut self, j: &mut Job, dir: &str, name: Option<String>) -> std::io::Result<()> {
        let dir = dir.trim().replace('/', "\\");
        std::fs::create_dir_all(crate::fs::long_path(&dir))?;
        let overwrite = self.overwrite();
        if j.state == S::Completed {
            let Some(fp) = j.final_path.clone() else { return Ok(()) };
            let current = Path::new(&fp).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let desired = sanitize_component(&name.unwrap_or(current));
            if Path::new(&fp) == Path::new(&dir).join(&desired) {
                return Ok(());
            }
            let n = self.res.reserve(&j.id, &dir, &desired, overwrite)?;
            let dst = Path::new(&dir).join(&n);
            move_path(Path::new(&fp), &dst)?;
            self.res.release(&j.id);
            j.final_path = Some(dst.to_string_lossy().into_owned());
            j.reserved_name = Some(n.clone());
            j.spec.filename = Some(n);
            j.spec.dest_dir = dir;
            self.save(j);
            return Ok(());
        }
        let desired = name.or_else(|| j.spec.filename.clone()).or_else(|| j.reserved_name.clone());
        self.res.release(&j.id);
        j.spec.dest_dir = dir.clone();
        j.reserved_name = None;
        if let Some(n) = desired {
            let n = sanitize_component(&n);
            j.spec.filename = Some(n.clone());
            j.reserved_name = self.res.reserve(&j.id, &dir, &n, overwrite).ok();
        }
        // Data already downloaded moves along (the HTTP part file or the video's work folder).
        if let Some(t) = j.temp_path.clone() {
            if crate::fs::long_path(&t).exists() {
                let file = Path::new(&t).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let new_name = if file.ends_with(".sdmpart") {
                    format!(".{}.{}.sdmpart", j.reserved_name.as_deref().unwrap_or("download"), &j.id[..8])
                } else {
                    file
                };
                let dst = Path::new(&dir).join(new_name);
                if dst != Path::new(&t) {
                    move_path(Path::new(&t), &dst)?;
                }
                j.temp_path = Some(dst.to_string_lossy().into_owned());
            } else {
                j.temp_path = None;
            }
        }
        self.save(j);
        Ok(())
    }

    fn on_retry(&mut self, id: &str) {
        self.with_job(id, |me, j| {
            if j.state == S::Failed {
                j.attempts = 0;
                me.set_state(j, S::Queued, None, None, false);
            }
        });
    }

    fn on_cancel(&mut self, id: &str) {
        self.with_job(id, |me, j| match j.state {
            S::Queued | S::Paused | S::RetryWait | S::Failed => {
                me.discard(j);
                me.set_state(j, S::Cancelled, None, None, false);
                me.log(&j.id, "info", "Cancelled");
            }
            S::Starting | S::Downloading | S::Postprocessing | S::Pausing => {
                me.set_state(j, S::Cancelling, None, None, false);
                if let Some((_, t)) = me.workers.get(id) {
                    t.set(StopReason::Cancel);
                }
            }
            _ => {}
        });
        self.maybe_remove(id);
    }

    fn on_remove(&mut self, id: &str, delete_file: bool) {
        let Some(state) = self.jobs.get(id).map(|j| j.state) else { return };
        if state.running() {
            self.pending_remove.insert(id.into(), delete_file);
            if state != S::Finalizing {
                self.on_cancel(id);
            }
            return;
        }
        let mut j = self.jobs.remove(id).unwrap();
        if j.state == S::Completed && delete_file {
            if let Some(p) = &j.final_path {
                if let Err(e) = std::fs::remove_file(crate::fs::long_path(p)) {
                    self.log(id, "warning", &format!("Could not delete file: {e}"));
                }
            }
        }
        if !j.state.terminal() {
            self.discard(&mut j);
        }
        self.res.release(id);
        let _ = self.store.delete_job(id);
        self.shared.snaps.write().unwrap().remove(id);
        self.shared.progress.write().unwrap().remove(id);
        self.via_queue.remove(id);
        self.hub.publish(Event::JobRemoved { job_id: id.into() });
    }

    fn maybe_remove(&mut self, id: &str) {
        let not_running = self.jobs.get(id).is_some_and(|j| !j.state.running());
        if not_running {
            if let Some(del) = self.pending_remove.remove(id) {
                self.on_remove(id, del);
            }
        }
    }

    fn on_refresh(&mut self, id: &str, url: String, probe: crate::http::probe::HttpProbe) -> Result<(), String> {
        let job = self.jobs.get(id).ok_or("Unknown download.")?;
        if job.state.running() {
            return Err("Pause the download before changing its address.".into());
        }
        if job.state == S::Completed || job.state == S::Cancelled {
            return Err("This download is already finished.".into());
        }
        if probe.is_html() {
            return Err("The new link opens a web page, not the file.".into());
        }
        if let (Some(old), Some(new)) = (job.total_bytes, probe.total) {
            if old != new {
                return Err(format!(
                    "The new link points to a different file ({new} bytes instead of {old}). Keep the old link or start a new download."
                ));
            }
        } else if job.committed_bytes > 0 {
            return Err("The file size could not be confirmed, so the partial data cannot be kept safely.".into());
        }
        let mut j = self.jobs.remove(id).unwrap();
        j.spec.url = url.clone();
        j.final_url = Some(probe.final_url.clone());
        // Validators come from the new server; the overlap check on resume verifies content.
        j.etag = probe.etag.clone();
        j.last_modified = probe.last_modified.clone();
        j.accept_ranges = Some(probe.ranges);
        self.save(&mut j);
        self.log(id, "info", &format!("Download address refreshed: {}", crate::logging::redact_url(&url)));
        self.jobs.insert(id.into(), j);
        Ok(())
    }

    // ------------------------------------------------------------------ queues
    fn set_queue_running(&mut self, qid: &str, running: bool) {
        let Some(q) = self.queues.iter_mut().find(|q| q.id == qid) else { return };
        if q.running == running {
            return;
        }
        q.running = running;
        self.persist_queues();
        self.hub.publish(Event::QueueChanged { queue_id: qid.into(), running });
        if !running {
            // Stop the queue's downloads; they wait in the queue for the next start.
            let ids: Vec<String> = self.via_queue.iter().filter(|(_, q)| *q == qid).map(|(id, _)| id.clone()).collect();
            for id in ids {
                let st = self.jobs.get(&id).map(|j| j.state);
                match st {
                    Some(S::Starting | S::Downloading | S::Postprocessing) => {
                        // Keep the queue marker until the worker has really stopped, so the
                        // winding-down download still counts against this queue's limit.
                        self.requeue_on_stop.insert(id.clone());
                        self.on_pause(&id);
                        continue;
                    }
                    Some(S::Pausing | S::Cancelling) => continue,
                    Some(S::Queued | S::RetryWait) => self.with_job(&id, |me, j| {
                        me.set_state(j, S::Paused, Some(QUEUED_REASON), None, true);
                    }),
                    _ => {}
                }
                self.via_queue.remove(&id);
            }
        }
    }

    fn on_save_queue(&mut self, q: Queue) {
        if let Some(existing) = self.queues.iter_mut().find(|x| x.id == q.id) {
            let running = existing.running;
            *existing = q;
            existing.running = running;
        } else {
            self.queues.push(Queue { running: false, ..q });
        }
        self.persist_queues();
    }

    fn on_delete_queue(&mut self, qid: &str) {
        if qid == MAIN_QUEUE {
            return;
        }
        self.set_queue_running(qid, false);
        self.queues.retain(|q| q.id != qid);
        let _ = self.store.delete_queue(qid);
        let ids: Vec<String> =
            self.jobs.values().filter(|j| j.spec.queue_id.as_deref() == Some(qid)).map(|j| j.id.clone()).collect();
        for id in ids {
            self.on_move_to_queue(&id, Some(MAIN_QUEUE.into()));
        }
        self.publish_queues();
    }

    fn on_move_to_queue(&mut self, id: &str, q: Option<String>) {
        self.with_job(id, |me, j| {
            j.spec.queue_id = q.clone();
            if q.is_some() && j.state == S::Paused && j.reason.as_deref() != Some(QUEUED_REASON) {
                j.state = S::Paused;
                j.reason = Some(QUEUED_REASON.into());
            }
            me.save(j);
        });
    }

    fn queue_has_work(&self, qid: &str) -> bool {
        self.jobs.values().any(|j| {
            (j.spec.queue_id.as_deref() == Some(qid) && j.state == S::Paused && j.reason.as_deref() == Some(QUEUED_REASON))
                || (self.via_queue.get(&j.id).map(String::as_str) == Some(qid)
                    && (j.state.running() || matches!(j.state, S::Queued | S::RetryWait)))
        })
    }

    fn on_schedule_tick(&mut self, minute: u32, weekday: usize) {
        let prev = self.last_tick.replace(minute).unwrap_or(minute);
        let decisions: Vec<(String, Tick)> = self
            .queues
            .iter()
            .filter_map(|q| q.schedule.as_ref().map(|s| (q.id.clone(), scheduler::evaluate(s, prev, minute, weekday))))
            .collect();
        for (qid, tick) in decisions {
            match tick {
                Tick::Start => {
                    self.log(&qid, "info", "Scheduled start of queue");
                    self.set_queue_running(&qid, true)
                }
                Tick::Stop => {
                    self.log(&qid, "info", "Scheduled stop of queue");
                    self.set_queue_running(&qid, false)
                }
                Tick::Nothing => {}
            }
        }
    }

    // ------------------------------------------------------------------ worker messages
    fn on_phase(&mut self, id: &str, phase: &str) {
        self.with_job(id, |me, j| {
            if phase == "downloading" && j.state == S::Starting {
                me.set_state(j, S::Downloading, None, None, false);
            } else if phase == "postprocessing" && j.state == S::Downloading {
                me.set_state(j, S::Postprocessing, None, None, false);
            }
        });
    }

    fn on_done(&mut self, id: &str, outcome: Outcome) {
        self.workers.remove(id);
        self.job_limiters.remove(id);
        let p = self.shared.progress.read().unwrap().get(id).cloned();
        let requeue = self.requeue_on_stop.remove(id);
        self.with_job(id, |me, j| {
            if let Some(p) = &p {
                j.committed_bytes = p.downloaded;
            }
            match outcome {
                Outcome::Completed { artifact, size, verified } => {
                    if j.state == S::Cancelling {
                        me.discard(j);
                        me.set_state(j, S::Cancelled, None, None, false);
                    } else {
                        j.artifact_path = Some(artifact.clone());
                        j.size_verified = Some(verified);
                        me.set_state(j, S::Finalizing, None, None, false);
                        me.finalize(j, artifact, size, verified);
                    }
                }
                Outcome::Stopped(reason) => {
                    if j.state == S::Cancelling || reason == StopReason::Cancel {
                        me.discard(j);
                        if j.state != S::Cancelling {
                            me.set_state(j, S::Cancelling, None, None, false);
                        }
                        me.set_state(j, S::Cancelled, None, None, false);
                        me.log(&j.id, "info", "Cancelled");
                    } else if reason == StopReason::Shutdown && me.stopping.is_some() {
                        me.set_state(j, S::Paused, Some("interrupted"), None, true);
                    } else if requeue {
                        me.set_state(j, S::Paused, Some(QUEUED_REASON), None, true);
                    } else {
                        me.set_state(j, S::Paused, Some("paused"), None, true);
                        me.log(&j.id, "info", "Paused");
                    }
                }
                Outcome::Failed(err) => me.on_failed(j, err),
            }
            if !matches!(j.state, S::Queued | S::RetryWait) {
                me.via_queue.remove(&j.id);
            }
        });
        if let Some((dir, name, resume)) = self.pending_relocate.remove(id) {
            let resume = resume && self.jobs.get(id).is_some_and(|j| j.state == S::Paused);
            self.with_job(id, |me, j| {
                if let Err(e) = me.relocate_now(j, &dir, name) {
                    me.log(&j.id, "error", &format!("Could not move the download to {dir}: {e}"));
                }
            });
            if resume {
                self.on_resume(id);
            }
        }
        self.maybe_remove(id);
    }

    fn on_failed(&mut self, j: &mut Job, err: ErrorInfo) {
        if j.state == S::Cancelling {
            self.discard(j);
            self.set_state(j, S::Cancelled, None, None, false);
            return;
        }
        self.log(&j.id, "error", &err.message);
        if err.code == ErrorCode::DiskFull {
            self.set_state(j, S::Paused, Some("disk_full"), Some(err), false);
            return;
        }
        if j.state == S::Pausing {
            self.set_state(j, S::Paused, Some("paused"), Some(err), false);
            return;
        }
        j.attempts += 1;
        let max_attempts = self.settings.get().retries.max(1);
        if err.retryable && j.attempts < max_attempts && self.stopping.is_none() {
            let delay = err
                .retry_after
                .map(Duration::from_secs_f64)
                .unwrap_or_else(|| (self.config.retry_base * 3u32.pow(j.attempts - 1)).min(Duration::from_secs(120)));
            let reason = format!("retry in {}s", delay.as_secs());
            self.set_state(j, S::RetryWait, Some(&reason), Some(err), false);
            self.seq += 1;
            self.timers.push(Reverse((Instant::now() + delay, self.seq, j.id.clone())));
        } else {
            self.set_state(j, S::Failed, None, Some(err), false);
        }
    }

    fn fire_timers(&mut self) {
        let now = Instant::now();
        while let Some(Reverse((t, _, _))) = self.timers.peek() {
            if *t > now {
                break;
            }
            let Reverse((_, _, id)) = self.timers.pop().unwrap();
            self.with_job(&id, |me, j| {
                if j.state == S::RetryWait {
                    me.set_state(j, S::Queued, None, None, true);
                }
            });
        }
    }

    // ------------------------------------------------------------------ quota
    fn check_quota(&mut self) {
        let s = self.settings.get();
        let now = crate::now();
        self.quota.record(now, self.limiter.received());
        let window = s.quota_hours.max(1) as f64 * 3600.0;
        let limit = s.quota_mb.saturating_mul(1024 * 1024);
        let used = self.quota.used(now, window);
        let status = QuotaStatus { used, limit, blocked: self.quota_blocked };
        if *self.shared.quota.read().unwrap() != status {
            *self.shared.quota.write().unwrap() = status;
        }
        if limit > 0 && !self.quota_blocked && used >= limit {
            self.quota_blocked = true;
            let ids: Vec<(String, JobState)> = self
                .jobs
                .values()
                .filter(|j| j.state.active() || matches!(j.state, S::Queued | S::RetryWait))
                .map(|j| (j.id.clone(), j.state))
                .collect();
            for (id, st) in ids {
                if self.via_queue.contains_key(&id) {
                    // Queue members go back to their queue, like when the queue is stopped.
                    if st.active() {
                        self.requeue_on_stop.insert(id.clone());
                        self.on_pause(&id);
                    } else {
                        self.with_job(&id, |me, j| me.set_state(j, S::Paused, Some(QUEUED_REASON), None, true));
                        self.via_queue.remove(&id);
                    }
                } else {
                    self.quota_paused.insert(id.clone());
                    self.on_pause(&id);
                }
            }
            let resume_in_secs = self.quota.seconds_until_below(now, window, limit.saturating_sub(1));
            tracing::warn!("download quota reached: {used} of {limit} bytes");
            self.hub.publish(Event::QuotaExceeded { used, limit, resume_in_secs });
        } else if self.quota_blocked && (limit == 0 || used < limit) {
            self.quota_blocked = false;
            let ids: Vec<String> = self.quota_paused.drain().collect();
            for id in ids {
                let resumable = self.jobs.get(&id).is_some_and(|j| j.state == S::Paused && j.reason.as_deref() == Some("paused"));
                if resumable {
                    self.on_resume(&id);
                }
            }
            self.hub.publish(Event::QuotaReleased);
        }
    }

    // ------------------------------------------------------------------ scheduling
    fn schedule(&mut self) {
        if self.quota_blocked {
            return;
        }
        // 1. Running queues pull their waiting jobs.
        let queues: Vec<(String, usize)> =
            self.queues.iter().filter(|q| q.running).map(|q| (q.id.clone(), q.max_concurrent.max(1) as usize)).collect();
        for (qid, limit) in &queues {
            let active = self
                .via_queue
                .iter()
                .filter(|(id, q)| *q == qid && self.jobs.get(*id).is_some_and(|j| j.state.running() || j.state == S::Queued))
                .count();
            if active >= *limit {
                continue;
            }
            let mut waiting: Vec<&Job> = self
                .jobs
                .values()
                .filter(|j| {
                    j.spec.queue_id.as_deref() == Some(qid.as_str())
                        && j.state == S::Paused
                        && j.reason.as_deref() == Some(QUEUED_REASON)
                })
                .collect();
            waiting.sort_by(|a, b| b.spec.priority.cmp(&a.spec.priority).then(a.created_at.total_cmp(&b.created_at)));
            let pick: Vec<String> = waiting.iter().take(limit - active).map(|j| j.id.clone()).collect();
            for id in pick {
                self.via_queue.insert(id.clone(), qid.clone());
                self.with_job(&id, |me, j| me.set_state(j, S::Queued, None, None, false));
            }
        }
        // 2. Queue completion.
        let finished: Vec<(String, String)> = self
            .queues
            .iter()
            .filter(|q| q.running && !self.queue_has_work(&q.id))
            .map(|q| (q.id.clone(), q.after.as_str().to_string()))
            .collect();
        for (qid, action) in finished {
            self.set_queue_running(&qid, false);
            self.log(&qid, "info", "Queue finished");
            self.hub.publish(Event::QueueFinished { queue_id: qid, action });
        }
        // 3. Start QUEUED jobs within their pool's limit.
        let global_limit = self.max_concurrent();
        let mut global_active =
            self.workers.keys().filter(|id| !self.via_queue.contains_key(*id)).count();
        let mut queued: Vec<&Job> = self.jobs.values().filter(|j| j.state == S::Queued).collect();
        queued.sort_by(|a, b| b.spec.priority.cmp(&a.spec.priority).then(a.created_at.total_cmp(&b.created_at)).then(a.id.cmp(&b.id)));
        let candidates: Vec<(String, Option<String>)> =
            queued.iter().map(|j| (j.id.clone(), self.via_queue.get(&j.id).cloned())).collect();
        for (id, via) in candidates {
            match via {
                Some(qid) => {
                    let limit = self.queues.iter().find(|q| q.id == qid).map(|q| q.max_concurrent.max(1) as usize).unwrap_or(1);
                    let active =
                        self.workers.keys().filter(|w| self.via_queue.get(*w) == Some(&qid)).count();
                    if active < limit {
                        self.start(&id);
                    }
                }
                None => {
                    if global_active < global_limit {
                        global_active += 1;
                        self.start(&id);
                    }
                }
            }
        }
    }

    fn start(&mut self, id: &str) {
        let token = CancelToken::new();
        let mut copy = None;
        self.with_job(id, |me, j| {
            j.last_try = Some(crate::now());
            me.set_state(j, S::Starting, None, None, false);
            copy = Some(j.clone());
        });
        let Some(job) = copy else { return };
        let job_limiter = Arc::new(Limiter::new(job.spec.speed_limit.unwrap_or(0)));
        self.job_limiters.insert(job.id.clone(), job_limiter.clone());
        let ctx = RunCtx {
            cancel: token.clone(),
            reporter: self.reporter.clone(),
            settings: self.settings.clone(),
            limiter: self.limiter.clone(),
            job_limiter,
        };
        let engines = self.engines.clone();
        let tx = self.tx.clone();
        let res = self.res_handle();
        let h = tokio::spawn(async move {
            let id = job.id.clone();
            let outcome = run_worker(job, engines, ctx, res).await;
            let _ = tx.send(Cmd::Done(id, outcome));
        });
        self.workers.insert(id.into(), (h, token));
    }

    fn res_handle(&self) -> ResHandle {
        ResHandle { tx: self.tx.clone() }
    }

    // ------------------------------------------------------------------ finalize / discard
    fn finalize(&mut self, j: &mut Job, artifact: String, size: u64, _verified: bool) {
        let engine = j.kind.as_deref().and_then(|k| self.engines.get(k).cloned());
        let result = (|| -> Result<std::path::PathBuf, SdmError> {
            let meta = std::fs::metadata(crate::fs::long_path(&artifact))
                .map_err(|_| SdmError::new(ErrorCode::Postprocess, "The downloaded file is missing."))?;
            if meta.len() != size {
                return Err(SdmError::new(
                    ErrorCode::Postprocess,
                    format!("The downloaded file has {} bytes but {size} were expected.", meta.len()),
                ));
            }
            let desired = j.reserved_name.clone().unwrap_or_else(|| {
                sanitize_component(&Path::new(&artifact).file_name().unwrap_or_default().to_string_lossy())
            });
            let final_path = finalize_move(Path::new(&artifact), j.dest_dir(), &desired, &j.id, &self.res, self.overwrite())
                .map_err(|e| from_io(&e, j.dest_dir()))?;
            let got = std::fs::metadata(crate::fs::long_path(&final_path)).map(|m| m.len()).unwrap_or(0);
            if got != size {
                return Err(SdmError::new(ErrorCode::Postprocess, "The file changed while it was being moved into place."));
            }
            if final_path.file_name().map(|n| n.to_string_lossy().into_owned()).as_deref() != Some(desired.as_str()) {
                self.log(&j.id, "info", &format!("A file named {desired} already existed; saved as {}", final_path.file_name().unwrap().to_string_lossy()));
            }
            Ok(final_path)
        })();
        match result {
            Ok(final_path) => {
                j.final_path = Some(final_path.to_string_lossy().into_owned());
                j.reserved_name = final_path.file_name().map(|n| n.to_string_lossy().into_owned());
                j.artifact_path = None;
                j.temp_path = None;
                j.committed_bytes = size;
                if j.total_bytes.is_none() {
                    j.total_bytes = Some(size);
                }
                j.completed_at = Some(crate::now());
                if let Some(e) = &engine {
                    e.cleanup(j);
                }
                self.set_state(j, S::Completed, None, None, false);
                self.res.release(&j.id);
                self.log(&j.id, "info", &format!("Completed: {}", final_path.display()));
                self.maybe_scan(j);
            }
            Err(e) => self.set_state(j, S::Failed, None, Some(e.0), false),
        }
    }

    fn maybe_scan(&self, j: &Job) {
        let s = self.settings.get();
        let (Some(path), true) = (j.final_path.clone(), s.av_scan_enabled) else { return };
        let template = s.av_scan_command.clone();
        let hub = self.hub.clone();
        let id = j.id.clone();
        tokio::spawn(async move {
            let msg = match run_scan(&template, &path).await {
                Ok(0) => ("info", "Antivirus scan: no threats reported.".to_string()),
                Ok(code) => ("warning", format!("Antivirus scan reported a problem (exit code {code}). Check your antivirus.")),
                Err(e) => ("warning", format!("Antivirus scan could not run: {e}")),
            };
            log_event(&hub, &id, msg.0, &msg.1);
        });
    }

    fn discard(&mut self, j: &mut Job) {
        if let Some(e) = j.kind.as_deref().and_then(|k| self.engines.get(k)) {
            e.discard(j);
        }
        j.temp_path = None;
        j.artifact_path = None;
        self.res.release(&j.id);
    }

    fn finish_shutdown(&mut self) {
        let ids: Vec<String> = self.workers.keys().cloned().collect();
        for id in ids {
            self.with_job(&id, |me, j| {
                if j.state.running() && j.state != S::Finalizing && j.state != S::Cancelling {
                    me.set_state(j, S::Paused, Some("interrupted"), None, true);
                }
            });
        }
        self.workers.clear();
        if let Some((_, reply)) = self.stopping.take() {
            let _ = reply.send(());
        }
    }

    pub fn reserved_names(&self) -> Vec<(String, String)> {
        self.jobs
            .values()
            .filter(|j| !j.state.terminal())
            .filter_map(|j| j.reserved_name.as_ref().map(|n| (j.dest_dir().to_lowercase(), n.to_lowercase())))
            .collect()
    }
}

/// Split a command template like `"C:\x\scan.exe" -File "{file}"` into program and args.
fn split_command(template: &str, file: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in template.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            ' ' if !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|a| a.replace("{file}", file)).collect()
}

async fn run_scan(template: &str, file: &str) -> std::io::Result<i32> {
    let parts = split_command(template, file);
    let Some((prog, args)) = parts.split_first() else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty scan command"));
    };
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(args).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(Duration::from_secs(600), cmd.output())
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "scan timed out"))??;
    Ok(out.status.code().unwrap_or(-1))
}

/// Name reservation performed by workers goes through the actor-owned registry: workers
/// ask via a blocking round trip so a name is never handed to two jobs.
#[derive(Clone)]
struct ResHandle {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl ResHandle {
    async fn reserve(&self, job_id: String, dest: String, desired: String) -> Result<String, String> {
        let f: InspectFn = Box::new(move |a: &Actor| {
            let r = a.res.reserve(&job_id, &dest, &desired, a.overwrite()).map_err(|e| e.to_string());
            Box::new(r)
        });
        let (rtx, rrx) = oneshot::channel();
        self.tx.send(Cmd::Inspect(f, rtx)).map_err(|_| "manager stopped".to_string())?;
        let b = rrx.await.map_err(|_| "manager stopped".to_string())?;
        *b.downcast::<Result<String, String>>().unwrap()
    }
}

async fn run_worker(
    mut job: Job,
    engines: Arc<HashMap<String, Arc<dyn Engine>>>,
    ctx: RunCtx,
    res: ResHandle,
) -> Outcome {
    let result: Result<Outcome, EngineError> = async {
        let mut kind = job
            .kind
            .clone()
            .unwrap_or_else(|| crate::router::route(&job.spec).to_string());
        let mut engine = engines
            .get(&kind)
            .cloned()
            .ok_or_else(|| SdmError::new(ErrorCode::Unsupported, format!("No engine available for this download ({kind}).")))?;
        if job.kind.as_deref() != Some(kind.as_str()) {
            job.kind = Some(kind.clone());
            ctx.reporter.update(&job.id, JobPatch { kind: Some(kind.clone()), ..Default::default() });
        }
        let mut probe = engine.probe(&job, &ctx).await?;
        if let Some(r) = probe.reroute.clone() {
            if job.spec.kind == "auto" && r != kind && engines.contains_key(&r) {
                ctx.reporter.log(&job.id, "info", "The link is a web page; trying media extraction.");
                kind = r;
                job.kind = Some(kind.clone());
                ctx.reporter.update(&job.id, JobPatch { kind: Some(kind.clone()), ..Default::default() });
                engine = engines[&kind].clone();
                probe = engine.probe(&job, &ctx).await?;
            }
        }
        ctx.check()?;
        let mut patch = probe.patch.clone();
        if let (Some(desired), None) = (probe.desired_name.clone(), job.reserved_name.clone()) {
            let name = res
                .reserve(job.id.clone(), job.spec.dest_dir.clone(), desired)
                .await
                .map_err(|e| SdmError::new(ErrorCode::PathInvalid, format!("No usable file name: {e}")))?;
            patch.reserved_name = Some(name);
        }
        patch.apply(&mut job);
        ctx.reporter.update(&job.id, patch);
        ctx.reporter.phase(&job.id, "downloading");
        engine.run(&mut job, &ctx).await
    }
    .await;
    let outcome = match result {
        Ok(o) => o,
        Err(EngineError::Stop(r)) => Outcome::Stopped(r),
        Err(EngineError::Fail(e)) => Outcome::Failed(e.0),
    };
    match outcome {
        Outcome::Failed(e) if ctx.cancel.is_set() && e.code != ErrorCode::DiskFull => {
            Outcome::Stopped(ctx.cancel.reason().unwrap_or(StopReason::Pause))
        }
        o => o,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_validation() {
        for bad in ["ftp://x/y", "file:///C:/Windows/win.ini", "javascript:alert(1)", "not a url", "", "http://"] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
        assert!(validate_url(" https://example.com/a.zip ").is_ok());
    }
    #[test]
    fn command_splitting() {
        assert_eq!(
            split_command(r#""C:\Program Files\x\scan.exe" -Scan -File "{file}""#, r"C:\d\a b.zip"),
            vec![r"C:\Program Files\x\scan.exe", "-Scan", "-File", r"C:\d\a b.zip"]
        );
    }
}
