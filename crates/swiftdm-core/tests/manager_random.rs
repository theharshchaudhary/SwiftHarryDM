//! Randomized command sequences (including simulated crashes) never violate the core
//! invariants. SWIFTDM_PROP_SEQUENCES controls the count (default 200; acceptance 10000).
mod common;

use common::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::Arc;
use swiftdm_core::manager::{Manager, StartMode};
use swiftdm_core::models::{JobSpec, JobState as S};
use swiftdm_core::scheduler::{Queue, MAIN_QUEUE};
use swiftdm_core::store::Store;

const BEHAVIOURS: &[&str] = &[
    "size=64&steps=2",
    "size=64&steps=4&delay_ms=2",
    "fail=network&times=1&size=8",
    "fail=forbidden&times=1&size=8",
    "fail=diskfull&times=1&size=8",
    "hang=1",
];
const NAMES: &[&str] = &["a.bin", "b.bin", "A.BIN"];

async fn check(m: &Manager, limit: usize, qlimit: usize) {
    let ok = m
        .inspect(move |a| {
            let global = a.workers.keys().filter(|id| !a.via_queue.contains_key(*id)).count();
            let queued = a.workers.keys().filter(|id| a.via_queue.contains_key(*id)).count();
            if global > limit || queued > qlimit {
                return Err(format!("too many workers: {global} global, {queued} queue"));
            }
            let mut live = HashMap::new();
            let mut finals = HashMap::new();
            for j in a.jobs.values() {
                let has_worker = a.workers.contains_key(&j.id);
                let needs = matches!(j.state, S::Starting | S::Downloading | S::Postprocessing | S::Pausing | S::Cancelling);
                if needs != has_worker {
                    return Err(format!("{:?} with worker={has_worker}", j.state));
                }
                let p = a.store().load_job(&j.id).unwrap().ok_or("job not persisted")?;
                if p.state != j.state || p.version != j.version {
                    return Err(format!("persisted {:?}/{} vs memory {:?}/{}", p.state, p.version, j.state, j.version));
                }
                if j.state == S::Completed {
                    let fp = j.final_path.clone().ok_or("completed without path")?;
                    let len = std::fs::metadata(&fp).map_err(|_| format!("completed file missing: {fp}"))?.len();
                    if Some(len) != j.total_bytes {
                        return Err("completed file has the wrong size".into());
                    }
                    if finals.insert(fp.to_lowercase(), ()).is_some() {
                        return Err("two jobs completed into the same file".into());
                    }
                }
                if let (Some(n), false) = (&j.reserved_name, j.state.terminal()) {
                    if live.insert((j.dest_dir().to_lowercase(), n.to_lowercase()), ()).is_some() {
                        return Err(format!("duplicate reservation {n}"));
                    }
                }
            }
            Ok(())
        })
        .await;
    if let Err(e) = ok {
        panic!("invariant violated: {e}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn random_sequences_keep_invariants() {
    let n: u64 = std::env::var("SWIFTDM_PROP_SEQUENCES").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let (limit, qlimit) = (2usize, 1usize);
    for seed in 0..n {
        let mut rng = StdRng::seed_from_u64(seed);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dl").to_string_lossy().into_owned();
        std::fs::create_dir_all(&dest).unwrap();
        let store = Arc::new(Store::open(dir.path().join("s.db")).unwrap());
        let eng = Arc::new(FakeEngine::default());
        let s = settings(limit as u32);
        let mut m = manager(store.clone(), eng.clone(), s.clone());
        let mut q = Queue::main();
        q.max_concurrent = qlimit as u32;
        m.save_queue(q.clone());
        let mut ids: Vec<String> = vec![];
        for _step in 0..12 {
            match rng.gen_range(0..10) {
                0..=3 => {
                    let b = BEHAVIOURS[rng.gen_range(0..BEHAVIOURS.len())];
                    let name = NAMES[rng.gen_range(0..NAMES.len())];
                    let mode = if rng.gen_bool(0.7) { StartMode::Now } else { StartMode::Later };
                    ids.push(m.submit(JobSpec::new(format!("http://fake.test/{name}?{b}"), &dest), mode).unwrap());
                }
                4..=6 if !ids.is_empty() => {
                    let id = ids[rng.gen_range(0..ids.len())].clone();
                    match rng.gen_range(0..5) {
                        0 => m.pause(&id),
                        1 => m.resume(&id),
                        2 => m.cancel(&id),
                        3 => m.retry(&id),
                        _ => m.remove(&id, rng.gen_bool(0.5)),
                    }
                }
                7 => {
                    if rng.gen_bool(0.5) {
                        m.start_queue(MAIN_QUEUE)
                    } else {
                        m.stop_queue(MAIN_QUEUE)
                    }
                }
                8 => {
                    m.pause_all();
                    m.resume_all();
                }
                _ => {
                    // Simulated crash: nothing further is persisted; recover from the store.
                    m.halt_for_tests().await;
                    m = manager(store.clone(), eng.clone(), s.clone());
                    m.save_queue(q.clone());
                }
            }
            if rng.gen_bool(0.3) {
                tokio::time::sleep(std::time::Duration::from_millis(rng.gen_range(1..15))).await;
            }
            m.sync().await;
            check(&m, limit, qlimit).await;
        }
        m.shutdown(std::time::Duration::from_secs(2)).await;
    }
}
