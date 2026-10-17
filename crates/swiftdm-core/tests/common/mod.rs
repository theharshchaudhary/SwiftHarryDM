#![allow(dead_code)]
//! Shared helpers for core integration tests.
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use swiftdm_core::cancel::StopReason;
use swiftdm_core::engine::{EResult, Engine, EngineError, Outcome, ProbeResult, RunCtx};
use swiftdm_core::errors::{ErrorCode, SdmError};
use swiftdm_core::manager::{Manager, ManagerConfig};
use swiftdm_core::models::{Job, JobPatch, ProgressSnapshot};
use swiftdm_core::settings::{Settings, SettingsStore};
use swiftdm_core::store::Store;

/// Behaviour comes from the URL query: size, steps, delay_ms, fail=network|forbidden|diskfull,
/// times=N (fail N times then succeed), hang=1 (run until stopped).
#[derive(Default)]
pub struct FakeEngine {
    pub failures: Mutex<HashMap<String, u32>>,
    pub concurrent: AtomicUsize,
    pub max_seen: AtomicUsize,
}

fn opts(job: &Job) -> HashMap<String, String> {
    url::Url::parse(&job.spec.url).unwrap().query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

impl FakeEngine {
    fn temp(job: &Job) -> String {
        std::path::Path::new(job.dest_dir())
            .join(format!(".{}.{}.sdmpart", job.reserved_name.as_deref().unwrap_or("x"), &job.id[..8]))
            .to_string_lossy()
            .into_owned()
    }
}

#[async_trait::async_trait]
impl Engine for FakeEngine {
    fn name(&self) -> &'static str {
        "http"
    }
    async fn probe(&self, job: &Job, _ctx: &RunCtx) -> EResult<ProbeResult> {
        let o = opts(job);
        let name = url::Url::parse(&job.spec.url).unwrap().path().rsplit('/').next().unwrap().to_string();
        Ok(ProbeResult {
            patch: JobPatch {
                total_bytes: Some(Some(o.get("size").and_then(|s| s.parse().ok()).unwrap_or(1000))),
                final_url: Some(job.spec.url.clone()),
                ..Default::default()
            },
            desired_name: Some(name),
            reroute: None,
        })
    }
    async fn run(&self, job: &mut Job, ctx: &RunCtx) -> EResult<Outcome> {
        let n = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_seen.fetch_max(n, Ordering::SeqCst);
        let r = self.run_inner(job, ctx).await;
        self.concurrent.fetch_sub(1, Ordering::SeqCst);
        r
    }
    fn discard(&self, job: &Job) {
        if let Some(t) = &job.temp_path {
            let _ = std::fs::remove_file(t);
        }
    }
    fn cleanup(&self, _job: &Job) {}
}

impl FakeEngine {
    async fn run_inner(&self, job: &mut Job, ctx: &RunCtx) -> EResult<Outcome> {
        let o = opts(job);
        if let Some(fail) = o.get("fail") {
            let times: u32 = o.get("times").and_then(|t| t.parse().ok()).unwrap_or(u32::MAX);
            let n = {
                let mut f = self.failures.lock().unwrap();
                let e = f.entry(job.id.clone()).or_default();
                *e += 1;
                *e - 1
            };
            if n < times {
                return Err(EngineError::Fail(match fail.as_str() {
                    "network" => SdmError::new(ErrorCode::Network, "network down").retryable(),
                    "diskfull" => SdmError::new(ErrorCode::DiskFull, "disk full"),
                    _ => SdmError::new(ErrorCode::Forbidden, "forbidden"),
                }));
            }
        }
        let size: u64 = o.get("size").and_then(|s| s.parse().ok()).unwrap_or(1000);
        let steps: u64 = o.get("steps").and_then(|s| s.parse().ok()).unwrap_or(3);
        let delay: u64 = o.get("delay_ms").and_then(|s| s.parse().ok()).unwrap_or(0);
        let temp = Self::temp(job);
        ctx.reporter.update(&job.id, JobPatch { temp_path: Some(temp.clone()), ..Default::default() });
        std::fs::create_dir_all(job.dest_dir()).map_err(|e| EngineError::Fail(swiftdm_core::errors::from_io(&e, job.dest_dir())))?;
        let mut f = std::fs::File::create(&temp).map_err(|e| EngineError::Fail(swiftdm_core::errors::from_io(&e, job.dest_dir())))?;
        let mut written = 0u64;
        let mut i = 0;
        loop {
            ctx.check()?;
            if o.contains_key("hang") {
                if ctx.cancel.sleep(Duration::from_millis(10)).await {
                    return Err(EngineError::Stop(ctx.cancel.reason().unwrap_or(StopReason::Pause)));
                }
                continue;
            }
            if i >= steps {
                break;
            }
            let chunk = size / steps;
            f.write_all(&vec![b'x'; chunk as usize]).unwrap();
            written += chunk;
            i += 1;
            ctx.reporter.progress(ProgressSnapshot {
                job_id: job.id.clone(),
                downloaded: written,
                total: Some(size),
                speed: None,
                eta: None,
                phase: String::new(),
                segments: vec![],
            });
            if delay > 0 && ctx.cancel.sleep(Duration::from_millis(delay)).await {
                return Err(EngineError::Stop(ctx.cancel.reason().unwrap_or(StopReason::Pause)));
            }
        }
        f.write_all(&vec![b'x'; (size - written) as usize]).unwrap();
        drop(f);
        Ok(Outcome::Completed { artifact: temp, size, verified: true })
    }
}

pub fn settings(max_concurrent: u32) -> SettingsStore {
    SettingsStore::in_memory(Settings { max_concurrent, ..Settings::default() })
}

pub fn manager(store: Arc<Store>, engine: Arc<dyn Engine>, s: SettingsStore) -> Manager {
    let mut engines: HashMap<String, Arc<dyn Engine>> = HashMap::new();
    engines.insert("http".into(), engine);
    Manager::start(
        store,
        engines,
        s,
        ManagerConfig { retry_base: Duration::from_millis(50), scheduler_ticker: false, ..Default::default() },
    )
    .unwrap()
}

pub fn sha_file(p: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(p).unwrap();
    std::io::copy(&mut f, &mut h).unwrap();
    hex::encode(h.finalize())
}
