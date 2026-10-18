//! Native HTTP engine: segmented, resumable, validated downloads.
//!
//! Durability rule: bytes are written and fsync'ed *before* the segment's committed offset is
//! updated in the database. After a crash only committed bytes are trusted.
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::FileExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rand::Rng;
use reqwest::header::{HeaderMap, HeaderValue, IF_RANGE, RANGE};
use reqwest::{Client, StatusCode};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::planner::{self, MIB};
use super::probe::{build_client, open_url, probe, status_error, HttpProbe, RequestAuth};
use super::validators::{if_range_value, parse_content_range, same_resource};
use crate::cancel::StopReason;
use crate::engine::{EResult, Engine, EngineError, Outcome, ProbeResult, RunCtx};
use crate::errors::{from_io, ErrorCode, SdmError};
use crate::fs::long_path;
use crate::fs::names::choose_filename;
use crate::limiter::Limiter;
use crate::models::{Job, JobPatch, ProgressSnapshot, Segment, SegmentProgress};
use crate::store::Store;

pub const CHUNK: usize = 256 * 1024;
pub const OVERLAP: u64 = 64 * 1024;
/// Bytes compared across a boundary between two connections' segments. Some mirrors and
/// load balancers serve different files on different connections; comparing the bytes on
/// both sides of each boundary catches that before the pieces are stitched together.
pub const BOUNDARY_VERIFY: u64 = 16 * 1024;
const DISK_MARGIN: u64 = 64 * MIB;

pub type WriteHook = Arc<dyn Fn(usize) -> io::Result<()> + Send + Sync>;

pub struct HttpEngine {
    store: Arc<Store>,
    probes: Mutex<HashMap<String, HttpProbe>>,
    pub checkpoint_bytes: u64,
    pub checkpoint_interval: Duration,
    pub seg_retries: u32,
    pub backoff_max: Duration,
    pub read_timeout: Duration,
    pub report_interval: Duration,
    /// Test hook: called before each write; returning an error simulates e.g. a full disk.
    pub write_hook: Option<WriteHook>,
}

impl HttpEngine {
    pub fn new(store: Arc<Store>) -> Self {
        HttpEngine {
            store,
            probes: Mutex::new(HashMap::new()),
            checkpoint_bytes: 4 * MIB,
            checkpoint_interval: Duration::from_secs(1),
            seg_retries: 5,
            backoff_max: Duration::from_secs(30),
            read_timeout: super::probe::READ_TIMEOUT,
            report_interval: Duration::from_millis(250),
            write_hook: None,
        }
    }

    pub fn temp_path_for(job: &Job) -> String {
        Path::new(job.dest_dir())
            .join(format!(".{}.{}.sdmpart", job.reserved_name.as_deref().unwrap_or("download"), &job.id[..8]))
            .to_string_lossy()
            .into_owned()
    }

    fn auth(job: &Job, ctx: &RunCtx) -> RequestAuth {
        RequestAuth::for_url(&job.spec.url, &ctx.settings.get(), job.spec.cookies.clone(), job.spec.referer.clone())
    }

    fn reset(&self, job: &Job) {
        let _ = self.store.delete_segments(&job.id);
        if let Some(t) = &job.temp_path {
            let _ = std::fs::remove_file(long_path(t));
        }
    }

    fn prepare(&self, job: &mut Job, ctx: &RunCtx, hp: &HttpProbe, temp: &str, single: bool) -> EResult<Vec<Segment>> {
        let ranges = hp.ranges && !single;
        let mut segments = self.store.load_segments(&job.id).map_err(db_err)?;
        let have_data = segments.iter().any(|s| s.committed > 0);
        let temp_exists = long_path(temp).exists();
        if !segments.is_empty() && !temp_exists {
            ctx.reporter.log(&job.id, "warning", "The partial file is missing; starting over.");
            segments.clear();
        } else if have_data {
            let mut ok = same_resource(
                job.total_bytes,
                job.etag.as_deref(),
                job.last_modified.as_deref(),
                hp.total,
                hp.etag.as_deref(),
                hp.last_modified.as_deref(),
            );
            if ok.is_ok() && !ranges {
                ok = Err("the server does not support resuming".into());
            }
            match ok {
                Err(why) => {
                    ctx.reporter.log(&job.id, "warning", &format!("Cannot resume: {why}. Restarting from the beginning."));
                    self.reset(job);
                    segments.clear();
                }
                Ok(()) => {
                    let n: u64 = segments.iter().map(|s| s.committed).sum();
                    ctx.reporter.log(&job.id, "info", &format!("Resuming at {n} bytes."));
                }
            }
        } else if !segments.is_empty() {
            self.reset(job);
            segments.clear();
        }
        let fresh = segments.is_empty();
        if fresh {
            segments = planner::plan(hp.total, ranges, ctx.settings.get().segments_for(&job.spec.url));
            self.store.replace_segments(&job.id, &segments).map_err(db_err)?;
        }
        let patch = JobPatch {
            total_bytes: Some(hp.total),
            etag: Some(hp.etag.clone()),
            last_modified: Some(hp.last_modified.clone()),
            accept_ranges: Some(ranges),
            final_url: Some(hp.final_url.clone()),
            ..Default::default()
        };
        patch.apply(job);
        ctx.reporter.update(&job.id, patch);
        let remaining = if fresh {
            hp.total.unwrap_or(0)
        } else {
            segments.iter().map(|s| s.length().unwrap_or(0).saturating_sub(s.committed)).sum()
        };
        let dest = job.dest_dir().to_string();
        std::fs::create_dir_all(long_path(&dest)).map_err(|e| from_io(&e, &dest))?;
        if let Some(total) = hp.total {
            let free = crate::fs::free_space(Path::new(&dest)).unwrap_or(u64::MAX);
            if free < remaining + DISK_MARGIN {
                return Err(SdmError::new(
                    ErrorCode::DiskFull,
                    format!("Not enough free space in {dest}: {} MB needed, {} MB free.", (remaining + DISK_MARGIN) / MIB, free / MIB),
                )
                .into());
            }
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(fresh || !temp_exists)
                .open(long_path(temp))
                .map_err(|e| from_io(&e, &dest))?;
            if f.metadata().map(|m| m.len()).unwrap_or(0) != total {
                f.set_len(total).map_err(|e| from_io(&e, &dest))?; // reserve the space up front
            }
        } else {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(long_path(temp))
                .map_err(|e| from_io(&e, &dest))?;
        }
        Ok(segments)
    }
}

fn db_err(e: crate::store::StoreError) -> EngineError {
    EngineError::Fail(SdmError::new(ErrorCode::Internal, "The download database could not be updated.").detail(e.to_string()))
}

#[async_trait::async_trait]
impl Engine for HttpEngine {
    fn name(&self) -> &'static str {
        "http"
    }

    async fn probe(&self, job: &Job, ctx: &RunCtx) -> EResult<ProbeResult> {
        let client = build_client(&ctx.settings.get())?;
        let hp = probe(&client, &job.spec.url, &Self::auth(job, ctx), &ctx.cancel).await?;
        self.probes.lock().unwrap().insert(job.id.clone(), hp.clone());
        if hp.is_html() && job.spec.kind == "auto" && job.committed_bytes == 0 {
            return Ok(ProbeResult { reroute: Some("media".into()), ..Default::default() });
        }
        let name = choose_filename(
            job.spec.filename.as_deref(),
            hp.content_disposition.as_deref(),
            Some(&hp.final_url),
            hp.content_type.as_deref(),
        );
        // Validators are compared with the stored ones in run(), so they are not updated here.
        Ok(ProbeResult {
            patch: JobPatch {
                final_url: Some(hp.final_url.clone()),
                content_type: Some(hp.content_type.clone()),
                title: Some(job.title.clone().unwrap_or_else(|| name.clone())),
                ..Default::default()
            },
            desired_name: Some(name),
            reroute: None,
        })
    }

    async fn run(&self, job: &mut Job, ctx: &RunCtx) -> EResult<Outcome> {
        let client = build_client(&ctx.settings.get())?;
        let auth = Self::auth(job, ctx);
        let mut hp = self.probes.lock().unwrap().remove(&job.id);
        let temp = job.temp_path.clone().unwrap_or_else(|| Self::temp_path_for(job));
        if job.temp_path.as_deref() != Some(temp.as_str()) {
            job.temp_path = Some(temp.clone());
            ctx.reporter.update(&job.id, JobPatch { temp_path: Some(temp.clone()), ..Default::default() });
        }
        let job_limiter = Some(ctx.job_limiter.clone());
        let mut single = false;
        for _ in 0..3 {
            let h = match hp.take() {
                Some(h) => h,
                None => probe(&client, &job.spec.url, &auth, &ctx.cancel).await?,
            };
            if h.is_html() && job.spec.kind != "http" {
                return Err(SdmError::new(ErrorCode::NotAFile, "The link opens a web page, not a downloadable file.").into());
            }
            let segments = self.prepare(job, ctx, &h, &temp, single)?;
            let t = Transfer::new(self, job, ctx, h, temp.clone(), segments, client.clone(), auth.clone(), job_limiter.clone());
            match t.execute().await {
                Ok(o) => return Ok(o),
                Err(TErr::Restart { reason, single: s }) => {
                    ctx.reporter.log(&job.id, "warning", &format!("{reason}; restarting from the beginning."));
                    single |= s;
                    self.reset(job);
                }
                Err(TErr::Stop(r)) => return Err(EngineError::Stop(r)),
                Err(TErr::Fail(e)) => return Err(EngineError::Fail(e)),
            }
        }
        Err(SdmError::new(ErrorCode::RangeViolation, "The server keeps sending inconsistent data.").retryable().into())
    }

    fn discard(&self, job: &Job) {
        let _ = self.store.delete_segments(&job.id);
        let mut paths = vec![];
        if let Some(t) = &job.temp_path {
            paths.push(t.clone());
        }
        if job.reserved_name.is_some() {
            paths.push(Self::temp_path_for(job));
        }
        for p in paths {
            let _ = std::fs::remove_file(long_path(&p));
        }
    }

    fn cleanup(&self, job: &Job) {
        let _ = self.store.delete_segments(&job.id);
    }
}

// ------------------------------------------------------------------ transfer

#[derive(Debug)]
enum TErr {
    Restart { reason: String, single: bool },
    Stop(StopReason),
    Fail(SdmError),
}

impl From<SdmError> for TErr {
    fn from(e: SdmError) -> Self {
        TErr::Fail(e)
    }
}

struct SegState {
    seg: Segment,
    pos: u64,
    end: i64,
    resumed: bool,
    launched: bool,
    running: bool,
    finished: bool,
}

impl SegState {
    fn done(&self) -> bool {
        self.end >= 0 && self.pos as i64 > self.end
    }
}

struct Shared {
    segs: BTreeMap<u32, SegState>,
    next_idx: u32,
    fatal: Option<TErr>,
    /// Boundary offsets whose bytes were compared across two connections.
    verified: std::collections::BTreeSet<u64>,
}

struct Ctx {
    store: Arc<Store>,
    job_id: String,
    dest: String,
    url: String,
    temp: String,
    total: Option<u64>,
    ranged: bool,
    if_range: Option<String>,
    client: Client,
    auth: RequestAuth,
    run: RunCtx,
    stop: CancellationToken,
    shared: Mutex<Shared>,
    job_limiter: Option<Arc<Limiter>>,
    checkpoint_bytes: u64,
    checkpoint_interval: Duration,
    seg_retries: u32,
    backoff_max: Duration,
    read_timeout: Duration,
    write_hook: Option<WriteHook>,
}

struct Transfer {
    c: Arc<Ctx>,
    max_active: usize,
    report_interval: Duration,
}

impl Transfer {
    #[allow(clippy::too_many_arguments)]
    fn new(
        eng: &HttpEngine,
        job: &Job,
        run: &RunCtx,
        hp: HttpProbe,
        temp: String,
        segments: Vec<Segment>,
        client: Client,
        auth: RequestAuth,
        job_limiter: Option<Arc<Limiter>>,
    ) -> Self {
        let segs: BTreeMap<u32, SegState> = segments
            .into_iter()
            .map(|s| {
                (s.idx, SegState { seg: s, pos: s.next_offset(), end: s.end, resumed: s.committed > 0, launched: false, running: false, finished: false })
            })
            .collect();
        let next_idx = segs.keys().max().map(|m| m + 1).unwrap_or(0);
        let c = Ctx {
            store: eng.store.clone(),
            job_id: job.id.clone(),
            dest: job.dest_dir().to_string(),
            url: job.final_url.clone().unwrap_or_else(|| job.spec.url.clone()),
            temp,
            total: hp.total,
            ranged: job.accept_ranges.unwrap_or(false),
            if_range: if_range_value(job.etag.as_deref(), job.last_modified.as_deref()),
            client,
            auth,
            run: run.clone(),
            stop: CancellationToken::new(),
            shared: Mutex::new(Shared { segs, next_idx, fatal: None, verified: Default::default() }),
            job_limiter,
            checkpoint_bytes: eng.checkpoint_bytes,
            checkpoint_interval: eng.checkpoint_interval,
            seg_retries: eng.seg_retries,
            backoff_max: eng.backoff_max,
            read_timeout: eng.read_timeout,
            write_hook: eng.write_hook.clone(),
        };
        Transfer {
            c: Arc::new(c),
            max_active: run.settings.get().segments_for(&job.spec.url) as usize,
            report_interval: eng.report_interval,
        }
    }

    async fn execute(self) -> Result<Outcome, TErr> {
        if self.c.total == Some(0) {
            return Ok(Outcome::Completed { artifact: self.c.temp.clone(), size: 0, verified: true });
        }
        let mut tasks: JoinSet<()> = JoinSet::new();
        let mut samples: VecDeque<(Instant, u64)> = VecDeque::new();
        let mut last_report = Instant::now() - self.report_interval;
        loop {
            if self.c.run.cancel.is_set() || self.c.stop.is_cancelled() {
                break;
            }
            let (to_launch, can_split, all_done) = {
                let mut g = self.c.shared.lock().unwrap();
                let running = g.segs.values().filter(|s| s.running).count();
                let pending: Vec<u32> = g.segs.values().filter(|s| !s.launched && !s.done()).map(|s| s.seg.idx).collect();
                let n = self.max_active.saturating_sub(running).min(pending.len());
                let launch: Vec<u32> = pending[..n].to_vec();
                for i in &launch {
                    let s = g.segs.get_mut(i).unwrap();
                    s.launched = true;
                    s.running = true;
                }
                let all_done = g.segs.values().all(|s| s.done() || (s.end < 0 && s.finished));
                (launch, running + n < self.max_active && pending.len() == n, all_done)
            };
            if all_done {
                break;
            }
            for idx in to_launch {
                let c = self.c.clone();
                tasks.spawn(async move { segment_main(c, idx).await });
            }
            if can_split {
                self.try_split();
            }
            if last_report.elapsed() >= self.report_interval {
                self.report(&mut samples);
                last_report = Instant::now();
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                _ = self.c.run.cancel.cancelled() => {}
                _ = self.c.stop.cancelled() => {}
            }
        }
        self.c.stop.cancel();
        while tasks.join_next().await.is_some() {}
        self.report(&mut samples);
        if self.c.run.cancel.is_set() {
            return Err(TErr::Stop(self.c.run.cancel.reason().unwrap_or(StopReason::Pause)));
        }
        if let Some(f) = self.c.shared.lock().unwrap().fatal.take() {
            return Err(f);
        }
        self.audit_boundaries().await?;
        self.complete()
    }

    fn complete(&self) -> Result<Outcome, TErr> {
        let size = std::fs::metadata(long_path(&self.c.temp)).map_err(|e| from_io(&e, &self.c.dest))?.len();
        let g = self.c.shared.lock().unwrap();
        match self.c.total {
            Some(total) => {
                if size != total || !g.segs.values().all(|s| s.done()) {
                    return Err(SdmError::new(ErrorCode::RangeViolation, "The download finished with missing data.")
                        .retryable()
                        .detail(format!("size={size} expected={total}"))
                        .into());
                }
                // Segments must tile [0, total) exactly: no gaps, no overlaps. Split-off segments
                // get new indexes, so check in file order, not index order.
                let mut by_start: Vec<&SegState> = g.segs.values().collect();
                by_start.sort_by_key(|s| s.seg.start);
                let mut expect = 0u64;
                for s in by_start {
                    if s.seg.start != expect {
                        return Err(SdmError::new(ErrorCode::RangeViolation, "Internal error: segment map has a gap or overlap.").into());
                    }
                    expect = s.end as u64 + 1;
                }
                if expect != total {
                    return Err(SdmError::new(ErrorCode::RangeViolation, "Internal error: segment map is incomplete.").into());
                }
                Ok(Outcome::Completed { artifact: self.c.temp.clone(), size, verified: true })
            }
            None => Ok(Outcome::Completed { artifact: self.c.temp.clone(), size, verified: false }),
        }
    }

    /// Check every boundary between segments that was not compared during the transfer: fetch
    /// a few bytes on both sides of it in one request and compare them with the file. A segment
    /// that finishes before its neighbour has written anything is never checked otherwise.
    async fn audit_boundaries(&self) -> Result<(), TErr> {
        let c = &self.c;
        let Some(total) = c.total else { return Ok(()) };
        if !c.ranged {
            return Ok(());
        }
        let todo: Vec<(u64, u64, u64)> = {
            let g = c.shared.lock().unwrap();
            let mut segs: Vec<&SegState> = g.segs.values().collect();
            segs.sort_by_key(|s| s.seg.start);
            segs.windows(2)
                .map(|w| (w[1].seg.start, w[1].seg.start - w[0].seg.start, total - w[1].seg.start))
                .filter(|(b, _, _)| !g.verified.contains(b))
                .collect()
        };
        if todo.is_empty() {
            return Ok(());
        }
        let f = File::open(long_path(&c.temp)).map_err(|e| from_io(&e, &c.dest))?;
        for (b, left_room, right_room) in todo {
            let (from, to) = (b - BOUNDARY_VERIFY.min(left_room), b + BOUNDARY_VERIFY.min(right_room) - 1);
            let mut headers = HeaderMap::new();
            headers.insert(RANGE, HeaderValue::from_str(&format!("bytes={from}-{to}")).unwrap());
            if let Some(ir) = c.if_range.as_deref().and_then(|v| HeaderValue::from_str(v).ok()) {
                headers.insert(IF_RANGE, ir);
            }
            let Ok((resp, _)) = open_url(&c.client, &c.url, &headers, &c.auth, &c.run.cancel).await else {
                if c.run.cancel.is_set() {
                    return Err(TErr::Stop(c.run.cancel.reason().unwrap_or(StopReason::Pause)));
                }
                // The data passed every other check; a failed spot check is not a reason to fail.
                c.run.reporter.log(&c.job_id, "warning", "Could not re-check a segment boundary; keeping the data.");
                continue;
            };
            let exact = resp.status() == StatusCode::PARTIAL_CONTENT
                && parse_content_range(resp.headers().get("content-range").and_then(|v| v.to_str().ok()))
                    .is_some_and(|(s, e, t)| s == from && e == to && t == Some(total));
            if !exact {
                return Err(TErr::Restart { reason: "The server stopped answering byte ranges consistently".into(), single: true });
            }
            let got = resp.bytes().await.map_err(|e| TErr::Fail(super::probe::map_reqwest(&e)))?;
            let mut on_disk = vec![0u8; (to - from + 1) as usize];
            read_exact_at(&f, &mut on_disk, from).map_err(|e| from_io(&e, &c.dest))?;
            if got.as_ref() != on_disk.as_slice() {
                return Err(mismatch(true));
            }
        }
        Ok(())
    }

    fn try_split(&self) {
        // Without byte ranges a new segment would receive the file from byte 0 (bug B1 in v2).
        if !self.c.ranged {
            return;
        }
        let mut g = self.c.shared.lock().unwrap();
        let cand = g
            .segs
            .values()
            .filter(|s| s.running && s.end >= 0 && !s.done())
            .max_by_key(|s| s.end - s.pos as i64)
            .map(|s| (s.seg.idx, s.pos, s.end));
        let Some((idx, pos, end)) = cand else { return };
        let Some(keep_to) = planner::split_point(pos + CHUNK as u64, end as u64) else { return };
        let new = Segment::new(g.next_idx, keep_to + 1, end);
        // Persist both halves atomically before the running segment can stop at the new end.
        if self.c.store.split_segment(&self.c.job_id, idx, keep_to as i64, &new).is_err() {
            return;
        }
        let s = g.segs.get_mut(&idx).unwrap();
        s.end = keep_to as i64;
        s.seg.end = keep_to as i64;
        g.next_idx += 1;
        g.segs.insert(new.idx, SegState { seg: new, pos: new.start, end: new.end, resumed: false, launched: false, running: false, finished: false });
    }

    fn report(&self, samples: &mut VecDeque<(Instant, u64)>) {
        let (done, segs) = {
            let g = self.c.shared.lock().unwrap();
            let done: u64 = g.segs.values().map(|s| s.pos - s.seg.start).sum();
            let segs: Vec<SegmentProgress> = g
                .segs
                .values()
                .map(|s| SegmentProgress { idx: s.seg.idx, start: s.seg.start, end: s.end, downloaded: s.pos - s.seg.start, active: s.running })
                .collect();
            (done, segs)
        };
        let now = Instant::now();
        samples.push_back((now, done));
        while samples.len() > 2 && now.duration_since(samples[0].0) > Duration::from_secs(5) {
            samples.pop_front();
        }
        let speed = match (samples.front(), samples.back()) {
            (Some(a), Some(b)) if b.0.duration_since(a.0) >= Duration::from_millis(500) => {
                Some((b.1.saturating_sub(a.1)) as f64 / b.0.duration_since(a.0).as_secs_f64())
            }
            _ => None,
        };
        let eta = match (speed, self.c.total) {
            (Some(s), Some(t)) if s > 0.0 => Some(t.saturating_sub(done) as f64 / s),
            _ => None,
        };
        self.c.run.reporter.progress(ProgressSnapshot {
            job_id: self.c.job_id.clone(),
            downloaded: done,
            total: self.c.total,
            speed,
            eta,
            phase: "downloading".into(),
            segments: segs,
        });
    }
}

fn set_fatal(c: &Ctx, e: TErr) {
    let mut g = c.shared.lock().unwrap();
    if g.fatal.is_none() {
        g.fatal = Some(e);
    }
    drop(g);
    c.stop.cancel();
}

async fn segment_main(c: Arc<Ctx>, idx: u32) {
    let r = match OpenOptions::new().read(true).write(true).open(long_path(&c.temp)) {
        Ok(f) => segment_loop(&c, idx, &f).await,
        Err(e) => Err(TErr::Fail(from_io(&e, &c.dest))),
    };
    if let Err(e) = r {
        match e {
            TErr::Stop(_) => {}
            other => set_fatal(&c, other),
        }
    }
    let mut g = c.shared.lock().unwrap();
    if let Some(s) = g.segs.get_mut(&idx) {
        s.running = false;
        s.finished = true;
    }
}

struct Ckpt {
    since: u64,
    last: Instant,
}

fn checkpoint(c: &Ctx, idx: u32, f: &File, ck: &mut Ckpt, force: bool) -> io::Result<()> {
    if !force && ck.since < c.checkpoint_bytes && ck.last.elapsed() < c.checkpoint_interval {
        return Ok(());
    }
    let committed = {
        let g = c.shared.lock().unwrap();
        g.segs.get(&idx).map(|s| s.pos - s.seg.start).unwrap_or(0)
    };
    tokio::task::block_in_place(|| -> io::Result<()> {
        f.sync_data()?;
        c.store
            .commit_segment(&c.job_id, idx, committed)
            .map_err(|e| io::Error::other(e.to_string()))
    })?;
    ck.since = 0;
    ck.last = Instant::now();
    Ok(())
}

fn write_all_at(f: &File, mut buf: &[u8], mut off: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = f.seek_write(buf, off)?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }
        buf = &buf[n..];
        off += n as u64;
    }
    Ok(())
}

fn read_exact_at(f: &File, buf: &mut [u8], mut off: u64) -> io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        let n = f.seek_read(&mut buf[done..], off)?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        done += n;
        off += n as u64;
    }
    Ok(())
}

fn stopped(c: &Ctx) -> bool {
    c.stop.is_cancelled() || c.run.cancel.is_set()
}

async fn backoff(c: &Ctx, failures: &mut u32, err: SdmError, quiet: bool) -> Result<(), TErr> {
    *failures += 1;
    if *failures > c.seg_retries {
        let mut e = err;
        e.0.retryable = true;
        return Err(TErr::Fail(e));
    }
    let mut delay = match err.0.retry_after {
        Some(s) => Duration::from_secs_f64(s),
        None => {
            let base = c.backoff_max.min(Duration::from_secs(1u64 << (*failures - 1).min(5)));
            base.mul_f64(0.75 + rand::thread_rng().gen::<f64>() * 0.5)
        }
    };
    if quiet {
        delay = delay.min(Duration::from_millis(200));
    }
    tokio::select! {
        _ = tokio::time::sleep(delay) => Ok(()),
        _ = c.run.cancel.cancelled() => Err(TErr::Stop(c.run.cancel.reason().unwrap_or(StopReason::Pause))),
        _ = c.stop.cancelled() => Err(TErr::Stop(StopReason::Pause)),
    }
}

async fn segment_loop(c: &Ctx, idx: u32, f: &File) -> Result<(), TErr> {
    let mut failures = 0u32;
    let mut first = true;
    let mut ck = Ckpt { since: 0, last: Instant::now() };
    let (start, resumed) = {
        let g = c.shared.lock().unwrap();
        let s = &g.segs[&idx];
        (s.seg.start, s.resumed)
    };
    if !c.ranged && start != 0 {
        return Err(SdmError::new(ErrorCode::Internal, "Internal error: a non-ranged stream must start at byte 0.").into());
    }
    let result: Result<(), TErr> = async {
        loop {
            if stopped(c) {
                return Ok(());
            }
            let (pos, end) = {
                let g = c.shared.lock().unwrap();
                let s = &g.segs[&idx];
                if s.done() {
                    return Ok(());
                }
                (s.pos, s.end)
            };
            let pos = if !c.ranged && pos != start {
                // No range support: a broken stream can only be restarted from zero.
                c.shared.lock().unwrap().segs.get_mut(&idx).unwrap().pos = start;
                f.set_len(0).map_err(|e| from_io(&e, &c.dest))?;
                start
            } else {
                pos
            };
            // Resume: re-read the end of our own committed data. Fresh segment after a finished
            // neighbour: re-read the neighbour's last bytes (a boundary check).
            let (overlap, boundary) = if first && resumed && c.ranged {
                (OVERLAP.min(pos - start), false)
            } else if first && c.ranged && pos == start && start > 0 {
                (predecessor_done_bytes(c, start).min(BOUNDARY_VERIFY), true)
            } else {
                (0, false)
            };
            let tail = Tail { end, len: if c.ranged { successor_bytes(c, end).min(BOUNDARY_VERIFY) } else { 0 } };
            let req_start = pos - overlap;
            let mut headers = HeaderMap::new();
            if c.ranged {
                let req_end = end + tail.len as i64;
                let r = if end >= 0 { format!("bytes={req_start}-{req_end}") } else { format!("bytes={req_start}-") };
                headers.insert(RANGE, HeaderValue::from_str(&r).unwrap());
                if let Some(ir) = &c.if_range {
                    if let Ok(v) = HeaderValue::from_str(ir) {
                        headers.insert(IF_RANGE, v);
                    }
                }
            }
            let resp = tokio::select! {
                r = open_url(&c.client, &c.url, &headers, &c.auth, &c.run.cancel) => r,
                _ = c.stop.cancelled() => return Ok(()),
            };
            let resp = match resp {
                Ok((r, _)) => r,
                Err(EngineError::Stop(_)) => return Ok(()),
                Err(EngineError::Fail(e)) => {
                    if !e.0.retryable {
                        return Err(e.into());
                    }
                    backoff(c, &mut failures, e, false).await?;
                    continue;
                }
            };
            check_response(c, &resp, req_start, if end >= 0 { end + tail.len as i64 } else { end })?;
            match consume(c, idx, resp, f, Verify { overlap, boundary, tail }, req_start, &mut ck).await {
                Ok(progressed) => {
                    first = false;
                    if progressed {
                        failures = 0;
                    }
                    let (done, unknown) = {
                        let g = c.shared.lock().unwrap();
                        let s = &g.segs[&idx];
                        (s.done(), s.end < 0)
                    };
                    if stopped(c) || done || unknown {
                        // Unknown length: a cleanly finished stream is the whole file.
                        return Ok(());
                    }
                    // The response ended before the segment was complete: ask again.
                    backoff(c, &mut failures, SdmError::new(ErrorCode::Network, "The server closed the connection early.").retryable(), progressed).await?;
                }
                Err(TErr::Fail(e)) if e.0.retryable && !stopped(c) => {
                    first = false;
                    backoff(c, &mut failures, e, false).await?;
                }
                Err(TErr::Fail(_)) if stopped(c) => return Ok(()),
                Err(other) => return Err(other),
            }
        }
    }
    .await;
    let _ = checkpoint(c, idx, f, &mut ck, true);
    result
}

fn check_response(c: &Ctx, resp: &reqwest::Response, req_start: u64, end: i64) -> Result<(), TErr> {
    if !c.ranged {
        if resp.status() != StatusCode::OK {
            return Err(status_error(resp).into());
        }
        return Ok(());
    }
    match resp.status() {
        StatusCode::PARTIAL_CONTENT => {
            let cr = parse_content_range(resp.headers().get("content-range").and_then(|v| v.to_str().ok()));
            let ok = cr.is_some_and(|(s, e, t)| s == req_start && (end < 0 || e as i64 <= end) && (c.total.is_none() || t == c.total));
            if !ok {
                return Err(TErr::Restart {
                    reason: format!("The server returned the wrong byte range for bytes={req_start}-{end}"),
                    single: true,
                });
            }
            Ok(())
        }
        StatusCode::OK => {
            if c.if_range.is_some() {
                Err(TErr::Restart { reason: "The file on the server changed during the download".into(), single: false })
            } else {
                Err(TErr::Restart { reason: "The server stopped honouring byte ranges".into(), single: true })
            }
        }
        StatusCode::RANGE_NOT_SATISFIABLE => {
            Err(TErr::Restart { reason: "The server rejected the requested byte range (416)".into(), single: false })
        }
        _ => Err(status_error(resp).into()),
    }
}

/// Bytes after a segment's end that the next segment has already written (0 if none).
fn successor_bytes(c: &Ctx, end: i64) -> u64 {
    if end < 0 || c.total.is_none() {
        return 0;
    }
    let g = c.shared.lock().unwrap();
    g.segs.values().find(|s| s.seg.start as i64 == end + 1).map(|s| s.pos - s.seg.start).unwrap_or(0)
}

/// Bytes before `start` available on disk because the previous segment has finished.
fn predecessor_done_bytes(c: &Ctx, start: u64) -> u64 {
    let g = c.shared.lock().unwrap();
    g.segs
        .values()
        .find(|s| s.end >= 0 && s.end as u64 + 1 == start && s.done())
        .map(|s| start - s.seg.start)
        .unwrap_or(0)
}

/// Extra bytes requested past the segment end, to be compared with the next segment's data.
#[derive(Clone, Copy)]
struct Tail {
    /// Segment end when the request was made; if a split moved it, the tail is not checked.
    end: i64,
    len: u64,
}

struct Verify {
    /// Leading bytes to compare with data already on disk instead of writing them.
    overlap: u64,
    /// The leading bytes belong to the previous segment (a boundary between connections).
    boundary: bool,
    tail: Tail,
}

fn mismatch(boundary: bool) -> TErr {
    if boundary {
        TErr::Restart {
            reason: "Two connections received different data for the same file; switching to a single connection".into(),
            single: true,
        }
    } else {
        TErr::Restart { reason: "Already-downloaded data no longer matches the server's file".into(), single: false }
    }
}

/// Add tail bytes; once complete, compare them with the next segment's data on disk.
/// Returns true when the comparison is done.
fn feed_tail(c: &Ctx, f: &File, tail: &Tail, buf: &mut Vec<u8>, data: &[u8]) -> Result<bool, TErr> {
    let need = (tail.len as usize - buf.len()).min(data.len());
    buf.extend_from_slice(&data[..need]);
    if (buf.len() as u64) < tail.len {
        return Ok(false);
    }
    let mut on_disk = vec![0u8; tail.len as usize];
    read_exact_at(f, &mut on_disk, tail.end as u64 + 1).map_err(|e| from_io(&e, &c.dest))?;
    if on_disk != *buf {
        return Err(mismatch(true));
    }
    c.shared.lock().unwrap().verified.insert(tail.end as u64 + 1);
    Ok(true)
}

/// Stream the body into the file. Returns Ok(true) if new bytes were written.
async fn consume(c: &Ctx, idx: u32, resp: reqwest::Response, f: &File, v: Verify, req_start: u64, ck: &mut Ckpt) -> Result<bool, TErr> {
    let mut stream = resp.bytes_stream();
    let mut progressed = false;
    let mut overlap = v.overlap;
    let mut verify: Vec<u8> = Vec::new();
    let mut tail_buf: Option<Vec<u8>> = None;
    let tail_live = |c: &Ctx| v.tail.len > 0 && c.shared.lock().unwrap().segs[&idx].end == v.tail.end;
    loop {
        let next = tokio::select! {
            n = tokio::time::timeout(c.read_timeout, stream.next()) => n,
            _ = c.stop.cancelled() => return Ok(progressed),
            _ = c.run.cancel.cancelled() => return Ok(progressed),
        };
        let chunk = match next {
            Err(_) if tail_buf.is_some() => return Ok(progressed),
            Err(_) => return Err(SdmError::new(ErrorCode::Timeout, "The server stopped responding.").retryable().into()),
            Ok(None) => return Ok(progressed),
            Ok(Some(Err(_))) if tail_buf.is_some() => return Ok(progressed),
            Ok(Some(Err(e))) => return Err(super::probe::map_reqwest(&e).into()),
            Ok(Some(Ok(b))) => b,
        };
        let mut data: &[u8] = &chunk;
        if let Some(tb) = tail_buf.as_mut() {
            if feed_tail(c, f, &v.tail, tb, data)? {
                return Ok(progressed);
            }
            continue;
        }
        if overlap > 0 {
            let need = (overlap as usize - verify.len()).min(data.len());
            verify.extend_from_slice(&data[..need]);
            data = &data[need..];
            if verify.len() as u64 == overlap {
                let mut on_disk = vec![0u8; overlap as usize];
                read_exact_at(f, &mut on_disk, req_start).map_err(|e| from_io(&e, &c.dest))?;
                if on_disk != verify {
                    return Err(mismatch(v.boundary));
                }
                if v.boundary {
                    c.shared.lock().unwrap().verified.insert(req_start + overlap);
                }
                overlap = 0;
            }
            if data.is_empty() {
                continue;
            }
        }
        let (pos, room) = {
            let g = c.shared.lock().unwrap();
            let s = &g.segs[&idx];
            (s.pos, (s.end >= 0).then(|| (s.end + 1 - s.pos as i64).max(0) as usize))
        };
        if room == Some(0) {
            if tail_live(c) {
                let mut tb = Vec::new();
                if feed_tail(c, f, &v.tail, &mut tb, data)? {
                    return Ok(progressed);
                }
                tail_buf = Some(tb);
                continue;
            }
            return Ok(progressed);
        }
        let (now, rest) = data.split_at(room.map_or(data.len(), |r| r.min(data.len())));
        c.run.limiter.acquire(now.len()).await;
        if let Some(l) = &c.job_limiter {
            l.acquire(now.len()).await;
        }
        if let Some(h) = &c.write_hook {
            h(now.len()).map_err(|e| from_io(&e, &c.dest))?;
        }
        write_all_at(f, now, pos).map_err(|e| from_io(&e, &c.dest))?;
        let reached = {
            let mut g = c.shared.lock().unwrap();
            let s = g.segs.get_mut(&idx).unwrap();
            s.pos = pos + now.len() as u64;
            s.done()
        };
        ck.since += now.len() as u64;
        progressed = true;
        checkpoint(c, idx, f, ck, false).map_err(|e| from_io(&e, &c.dest))?;
        if reached {
            if tail_live(c) {
                let mut tb = Vec::new();
                if feed_tail(c, f, &v.tail, &mut tb, rest)? {
                    return Ok(progressed);
                }
                tail_buf = Some(tb);
                continue;
            }
            return Ok(progressed);
        }
    }
}
