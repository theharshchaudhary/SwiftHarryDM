//! Manager behaviour with a scriptable fake engine (port of the v2 suite + queues).
mod common;

use common::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use swiftdm_core::errors::ErrorCode;
use swiftdm_core::manager::{StartMode, QUEUED_REASON};
use swiftdm_core::models::{Job, JobSpec, JobState as S};
use swiftdm_core::scheduler::{AfterAction, Queue, Schedule, MAIN_QUEUE};
use swiftdm_core::store::Store;

const T: Duration = Duration::from_secs(10);
const DONE: &[S] = &[S::Completed, S::Failed, S::Cancelled];

struct Env {
    _dir: tempfile::TempDir,
    dest: String,
    store: Arc<Store>,
    eng: Arc<FakeEngine>,
    m: swiftdm_core::manager::Manager,
}

fn env(max_concurrent: u32) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("dl").to_string_lossy().into_owned();
    std::fs::create_dir_all(&dest).unwrap();
    let store = Arc::new(Store::open(dir.path().join("s.db")).unwrap());
    let eng = Arc::new(FakeEngine::default());
    let m = manager(store.clone(), eng.clone(), settings(max_concurrent));
    Env { _dir: dir, dest, store, eng, m }
}

fn spec(e: &Env, q: &str, name: &str) -> JobSpec {
    JobSpec::new(format!("http://fake.test/{name}?{q}"), &e.dest)
}

fn files(e: &Env) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(&e.dest).unwrap().map(|d| d.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_path_and_no_temp_leftovers() {
    let e = env(2);
    let id = e.m.submit(spec(&e, "size=999&steps=3", "a.bin"), StartMode::Now).unwrap();
    let s = e.m.wait_for(&id, &[S::Completed], T).await.unwrap();
    assert_eq!(s.state, S::Completed);
    assert!(s.final_path.as_ref().unwrap().ends_with("a.bin"));
    assert_eq!(std::fs::metadata(s.final_path.unwrap()).unwrap().len(), 999);
    assert_eq!(files(&e), vec!["a.bin"]);
    assert_eq!(e.store.load_job(&id).unwrap().unwrap().state, S::Completed);
}

#[tokio::test(flavor = "multi_thread")]
async fn never_overwrites_and_concurrent_names_distinct() {
    let e = env(4);
    std::fs::write(format!("{}\\a.bin", e.dest), b"keep me").unwrap();
    let ids: Vec<String> = (0..4).map(|_| e.m.submit(spec(&e, "size=100&steps=4&delay_ms=20", "a.bin"), StartMode::Now).unwrap()).collect();
    let mut finals = vec![];
    for id in &ids {
        finals.push(e.m.wait_for(id, &[S::Completed], T).await.unwrap().final_path.unwrap());
    }
    finals.sort();
    finals.dedup();
    assert_eq!(finals.len(), 4);
    assert_eq!(std::fs::read(format!("{}\\a.bin", e.dest)).unwrap(), b"keep me");
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_resume_cancel() {
    let e = env(2);
    let id = e.m.submit(spec(&e, "hang=1", "a.bin"), StartMode::Now).unwrap();
    e.m.wait_for(&id, &[S::Downloading], T).await;
    e.m.pause(&id);
    assert_eq!(e.m.wait_for(&id, &[S::Paused], T).await.unwrap().reason.as_deref(), Some("paused"));
    e.m.resume(&id);
    assert_eq!(e.m.wait_for(&id, &[S::Downloading], T).await.unwrap().state, S::Downloading);
    e.m.cancel(&id);
    assert_eq!(e.m.wait_for(&id, &[S::Cancelled], T).await.unwrap().state, S::Cancelled);
    assert!(files(&e).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn retry_policy() {
    let e = env(2);
    let a = e.m.submit(spec(&e, "fail=network&times=2&size=10", "a.bin"), StartMode::Now).unwrap();
    assert_eq!(e.m.wait_for(&a, DONE, T).await.unwrap().state, S::Completed);
    let b = e.m.submit(spec(&e, "fail=network", "b.bin"), StartMode::Now).unwrap();
    let s = e.m.wait_for(&b, DONE, T).await.unwrap();
    assert_eq!((s.state, s.error.unwrap().code, s.attempts), (S::Failed, ErrorCode::Network, 3));
    let c = e.m.submit(spec(&e, "fail=forbidden&times=1&size=5", "c.bin"), StartMode::Now).unwrap();
    let s = e.m.wait_for(&c, DONE, T).await.unwrap();
    assert_eq!((s.state, s.attempts), (S::Failed, 1));
    e.m.retry(&c);
    assert_eq!(e.m.wait_for(&c, &[S::Completed], T).await.unwrap().state, S::Completed);
}

#[tokio::test(flavor = "multi_thread")]
async fn disk_full_pauses_resumably() {
    let e = env(2);
    let id = e.m.submit(spec(&e, "fail=diskfull&times=1&size=5", "a.bin"), StartMode::Now).unwrap();
    let s = e.m.wait_for(&id, &[S::Paused], T).await.unwrap();
    assert_eq!((s.reason.as_deref(), s.error.unwrap().code), (Some("disk_full"), ErrorCode::DiskFull));
    e.m.resume(&id);
    assert_eq!(e.m.wait_for(&id, &[S::Completed], T).await.unwrap().state, S::Completed);
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrency_limit_respected() {
    let e = env(2);
    let ids: Vec<String> =
        (0..7).map(|i| e.m.submit(spec(&e, "size=100&steps=10&delay_ms=20", &format!("f{i}.bin")), StartMode::Now).unwrap()).collect();
    for id in &ids {
        assert_eq!(e.m.wait_for(id, &[S::Completed], T).await.unwrap().state, S::Completed);
    }
    assert!(e.eng.max_seen.load(Ordering::SeqCst) <= 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn download_later_and_queue_start_with_limit_and_finish_event() {
    let e = env(4);
    let mut q = Queue::main();
    q.max_concurrent = 1;
    q.after = AfterAction::Shutdown;
    e.m.save_queue(q);
    let mut rx = e.m.subscribe();
    let ids: Vec<String> =
        (0..3).map(|i| e.m.submit(spec(&e, "size=50&steps=5&delay_ms=20", &format!("q{i}.bin")), StartMode::Later).unwrap()).collect();
    e.m.sync().await;
    for id in &ids {
        let s = e.m.get(id).unwrap();
        assert_eq!((s.state, s.reason.as_deref(), s.queue_id.as_deref()), (S::Paused, Some(QUEUED_REASON), Some(MAIN_QUEUE)));
    }
    e.m.start_queue(MAIN_QUEUE);
    for id in &ids {
        assert_eq!(e.m.wait_for(id, &[S::Completed], T).await.unwrap().state, S::Completed);
    }
    assert_eq!(e.eng.max_seen.load(Ordering::SeqCst), 1);
    let finished = tokio::time::timeout(T, async {
        loop {
            if let Ok(swiftdm_core::events::Event::QueueFinished { action, .. }) = rx.recv().await {
                return action;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(finished, "shutdown");
    assert!(!e.m.queues()[0].running);
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_queue_requeues_running_jobs() {
    let e = env(4);
    let id = e.m.submit(spec(&e, "hang=1", "h.bin"), StartMode::Later).unwrap();
    e.m.start_queue(MAIN_QUEUE);
    e.m.wait_for(&id, &[S::Downloading], T).await;
    e.m.stop_queue(MAIN_QUEUE);
    let s = e.m.wait_for(&id, &[S::Paused], T).await.unwrap();
    assert_eq!(s.reason.as_deref(), Some(QUEUED_REASON));
    e.m.start_queue(MAIN_QUEUE);
    assert_eq!(e.m.wait_for(&id, &[S::Downloading], T).await.unwrap().state, S::Downloading);
    e.m.cancel(&id);
    e.m.wait_for(&id, &[S::Cancelled], T).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn scheduled_start_and_stop() {
    let e = env(4);
    let mut q = Queue::main();
    q.schedule = Some(Schedule { enabled: true, start_time: "02:00".into(), days: [true; 7], stop_time: Some("03:00".into()) });
    e.m.save_queue(q);
    let id = e.m.submit(spec(&e, "hang=1", "s.bin"), StartMode::Later).unwrap();
    e.m.schedule_tick(100, 0);
    e.m.schedule_tick(119, 0);
    e.m.sync().await;
    assert_eq!(e.m.get(&id).unwrap().state, S::Paused);
    e.m.schedule_tick(120, 0);
    assert_eq!(e.m.wait_for(&id, &[S::Downloading], T).await.unwrap().state, S::Downloading);
    e.m.schedule_tick(180, 0);
    assert_eq!(e.m.wait_for(&id, &[S::Paused], T).await.unwrap().reason.as_deref(), Some(QUEUED_REASON));
    e.m.cancel(&id);
}

#[tokio::test(flavor = "multi_thread")]
async fn remove_running_job_and_bad_urls() {
    let e = env(2);
    let id = e.m.submit(spec(&e, "hang=1", "r.bin"), StartMode::Now).unwrap();
    e.m.wait_for(&id, &[S::Downloading], T).await;
    e.m.remove(&id, false);
    for _ in 0..200 {
        if e.m.get(&id).is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(e.m.get(&id).is_none() && e.store.load_job(&id).unwrap().is_none() && files(&e).is_empty());
    for bad in ["ftp://x/y", "file:///C:/Windows/win.ini", "javascript:alert(1)", "nope", ""] {
        assert!(e.m.submit(JobSpec::new(bad, &e.dest), StartMode::Now).is_err());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_marks_interrupted() {
    let e = env(2);
    let id = e.m.submit(spec(&e, "hang=1", "a.bin"), StartMode::Now).unwrap();
    e.m.wait_for(&id, &[S::Downloading], T).await;
    e.m.shutdown(Duration::from_secs(3)).await;
    let j = e.store.load_job(&id).unwrap().unwrap();
    assert_eq!((j.state, j.reason.as_deref()), (S::Paused, Some("interrupted")));
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_table() {
    for (persisted, auto, expected) in [
        (S::Starting, false, S::Paused),
        (S::Downloading, false, S::Paused),
        (S::Postprocessing, false, S::Paused),
        (S::RetryWait, false, S::Paused),
        (S::Pausing, false, S::Paused),
        (S::Downloading, true, S::Queued),
        (S::Cancelling, false, S::Cancelled),
        (S::Completed, false, S::Completed),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("s.db")).unwrap());
        let mut j = Job::new("f".repeat(32), JobSpec::new("http://fake.test/a.bin?hang=1", dir.path().to_string_lossy()));
        j.state = persisted;
        j.kind = Some("http".into());
        store.save_job(&j).unwrap();
        let s = settings(1);
        s.update(|x| x.auto_resume = auto).unwrap();
        let m = swiftdm_core::manager::Manager::start(
            store,
            [("http".to_string(), Arc::new(FakeEngine::default()) as Arc<dyn swiftdm_core::engine::Engine>)].into(),
            s,
            swiftdm_core::manager::ManagerConfig { autostart: false, scheduler_ticker: false, ..Default::default() },
        )
        .unwrap();
        assert_eq!(m.get(&j.id).unwrap().state, expected, "{persisted:?} auto={auto}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_refinalizes_and_fails_honestly() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().to_string_lossy().into_owned();
    let store = Arc::new(Store::open(dir.path().join("s.db")).unwrap());
    let art = dir.path().join(".a.bin.ffffffff.sdmpart");
    std::fs::write(&art, vec![b'z'; 42]).unwrap();
    let mut j = Job::new("f".repeat(32), JobSpec::new("http://fake.test/a.bin", &dest));
    j.state = S::Finalizing;
    j.kind = Some("http".into());
    j.reserved_name = Some("a.bin".into());
    j.artifact_path = Some(art.to_string_lossy().into());
    j.total_bytes = Some(42);
    j.size_verified = Some(true);
    store.save_job(&j).unwrap();
    let mut k = Job::new("e".repeat(32), JobSpec::new("http://fake.test/b.bin", &dest));
    k.state = S::Finalizing;
    k.kind = Some("http".into());
    k.artifact_path = Some(dir.path().join("gone").to_string_lossy().into());
    store.save_job(&k).unwrap();
    let m = manager(store, Arc::new(FakeEngine::default()), settings(1));
    let s = m.get(&j.id).unwrap();
    assert_eq!(s.state, S::Completed);
    assert_eq!(std::fs::metadata(s.final_path.unwrap()).unwrap().len(), 42);
    assert_eq!(m.get(&k.id).unwrap().state, S::Failed);
}
