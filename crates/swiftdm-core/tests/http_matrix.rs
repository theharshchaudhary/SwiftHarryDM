//! Native HTTP engine fault matrix: every case ends with the right bytes or the specified error.
mod common;

use common::sha_file;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use swiftdm_core::engine::Engine;
use swiftdm_core::errors::ErrorCode;
use swiftdm_core::http::HttpEngine;
use swiftdm_core::manager::{Manager, ManagerConfig, StartMode};
use swiftdm_core::models::{JobSnapshot, JobSpec, JobState as S};
use swiftdm_core::settings::{ProxyMode, Settings, SettingsStore, SiteLogin};
use swiftdm_core::store::Store;
use swiftdm_testkit::{Body, Fault, RangeBug, Resource, TestServer, MIB};

const DONE: &[S] = &[S::Completed, S::Failed, S::Cancelled];

struct Env {
    _dir: tempfile::TempDir,
    dest: String,
    store: Arc<Store>,
    m: Manager,
    srv: TestServer,
}

async fn env_with(tweak: impl FnOnce(&mut HttpEngine), settings: Settings) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("dl").to_string_lossy().into_owned();
    std::fs::create_dir_all(&dest).unwrap();
    let store = Arc::new(Store::open(dir.path().join("s.db")).unwrap());
    let mut eng = HttpEngine::new(store.clone());
    eng.read_timeout = Duration::from_secs(2);
    eng.backoff_max = Duration::from_millis(300);
    eng.checkpoint_interval = Duration::from_millis(200);
    tweak(&mut eng);
    let engines: HashMap<String, Arc<dyn Engine>> = [("http".to_string(), Arc::new(eng) as Arc<dyn Engine>)].into();
    let s = SettingsStore::in_memory(Settings { max_concurrent: 3, max_segments: 8, proxy_mode: ProxyMode::None, ..settings });
    let m = Manager::start(
        store.clone(),
        engines,
        s,
        ManagerConfig { retry_base: Duration::from_millis(100), scheduler_ticker: false, ..Default::default() },
    )
    .unwrap();
    Env { _dir: dir, dest, store, m, srv: TestServer::start().await }
}

async fn env() -> Env {
    env_with(|_| {}, Settings::default()).await
}

impl Env {
    fn submit(&self, url: &str) -> String {
        self.m.submit(JobSpec::new(url, &self.dest), StartMode::Now).unwrap()
    }
    async fn run(&self, url: &str, secs: u64) -> JobSnapshot {
        let id = self.submit(url);
        self.m.wait_for(&id, DONE, Duration::from_secs(secs)).await.unwrap()
    }
    fn files(&self) -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(&self.dest).unwrap().map(|d| d.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }
}

fn ok(s: &JobSnapshot, body: &Body) {
    assert_eq!(s.state, S::Completed, "{:?}", s.error);
    assert_eq!(sha_file(s.final_path.as_ref().unwrap()), body.sha256());
}

fn range_starts(e: &Env, path: &str, skip: usize) -> Vec<u64> {
    e.srv.requests(path)[skip..]
        .iter()
        .filter_map(|r| r.range.as_ref())
        .filter(|r| r.as_str() != "bytes=0-0")
        .map(|r| r[6..].split('-').next().unwrap().parse().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn small_file_keeps_name() {
    let e = env().await;
    let b = Body::gen("small", MIB);
    let s = e.run(&e.srv.add("/file.bin", Resource::new(b.clone())), 30).await;
    ok(&s, &b);
    assert_eq!(e.files(), vec!["file.bin"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn segmented_parallel_ranges() {
    let e = env().await;
    let b = Body::gen("seg", 24 * MIB);
    ok(&e.run(&e.srv.add("/big.bin", Resource::new(b.clone()).etag("\"v1\"")), 60).await, &b);
    assert!(range_starts(&e, "/big.bin", 0).len() >= 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_range_server_and_ignored_ranges() {
    let e = env().await;
    let b = Body::gen("nr", 9 * MIB);
    ok(&e.run(&e.srv.add("/nr.bin", Resource::new(b.clone()).no_ranges()), 60).await, &b);
    assert!(range_starts(&e, "/nr.bin", 0).is_empty());
    let b2 = Body::gen("ign", 6 * MIB);
    let mut r = Resource::new(b2.clone());
    r.range_bug = Some(RangeBug::Ignore);
    ok(&e.run(&e.srv.add("/ign.bin", r), 60).await, &b2);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_range_server_with_stalls_is_never_split() {
    // Regression for v2 bug B1 (randomized run 14).
    let e = env().await;
    let b = Body::gen("run14", 9 * MIB + 12345);
    let r = Resource::new(b.clone())
        .no_ranges()
        .throttle(8 * MIB)
        .fault(Fault::stall(800 * 1024, Duration::from_secs(3)).times(3).when(|_, r| r != Some("bytes=0-0")));
    ok(&e.run(&e.srv.add("/nr14.bin", r), 120).await, &b);
}

#[tokio::test(flavor = "multi_thread")]
async fn cuts_and_stalls_resume() {
    let e = env().await;
    let b = Body::gen("cut", 10 * MIB);
    let r = Resource::new(b.clone()).etag("\"e\"").fault(Fault::cut(3 * MIB).times(2).when(|_, r| r.is_some_and(|r| r.starts_with("bytes=0-") && r != "bytes=0-0")));
    ok(&e.run(&e.srv.add("/cut.bin", r), 60).await, &b);
    let b2 = Body::gen("stall", 6 * MIB);
    let r2 = Resource::new(b2.clone()).etag("\"s\"").fault(Fault::stall(MIB, Duration::from_secs(5)).when(|_, r| r != Some("bytes=0-0")));
    ok(&e.run(&e.srv.add("/stall.bin", r2), 60).await, &b2);
    let b3 = Body::gen("cuts", 8 * MIB);
    let r3 = Resource::new(b3.clone()).etag("\"c\"").fault(Fault::cut(256 * 1024).times(-1).when(|_, r| r != Some("bytes=0-0")));
    ok(&e.run(&e.srv.add("/cuts.bin", r3), 120).await, &b3);
}

#[tokio::test(flavor = "multi_thread")]
async fn server_errors_and_retry_after() {
    let e = env().await;
    let b = Body::gen("5xx", 2 * MIB);
    ok(&e.run(&e.srv.add("/5xx.bin", Resource::new(b.clone()).statuses(&[503, 502])), 60).await, &b);
    let b2 = Body::gen("429", MIB);
    let mut r = Resource::new(b2.clone()).statuses(&[429]);
    r.retry_after = Some("1".into());
    let t0 = Instant::now();
    ok(&e.run(&e.srv.add("/429.bin", r), 60).await, &b2);
    assert!(t0.elapsed() >= Duration::from_secs(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn forbidden_browser_agent_retries_as_ourselves() {
    // Like a CDN that turns away a "browser" without a browser's fingerprint.
    let e = env().await;
    let b = Body::gen("ua", MIB);
    ok(&e.run(&e.srv.add("/ua.bin", Resource::new(b.clone()).statuses(&[403])), 60).await, &b);
    let ua: Vec<_> = e.srv.requests("/ua.bin").into_iter().map(|r| r.user_agent.unwrap_or_default()).collect();
    assert!(ua[0].starts_with("Mozilla/"), "{ua:?}");
    assert!(ua[1].starts_with("SwiftHarryDM/"), "{ua:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn broken_content_range() {
    let e = env().await;
    let b = Body::gen("wrong", 6 * MIB);
    let mut r = Resource::new(b.clone());
    r.range_bug = Some(RangeBug::WrongStart);
    ok(&e.run(&e.srv.add("/wr.bin", r), 60).await, &b);
    let b2 = Body::gen("wt", 6 * MIB);
    let mut r2 = Resource::new(b2.clone());
    r2.range_bug = Some(RangeBug::WrongTotal);
    let s = e.run(&e.srv.add("/wt.bin", r2), 120).await;
    if s.state == S::Completed {
        ok(&s, &b2);
    } else {
        assert_eq!(s.state, S::Failed);
        assert!(!e.files().contains(&"wt.bin".to_string()));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn etag_change_mid_download_restarts_with_new_content() {
    let e = env().await;
    let old = Body::gen("old", 12 * MIB);
    let new = Body::gen("new", 12 * MIB);
    let mut r = Resource::new(old).etag("\"old\"").throttle(8 * MIB);
    r.replace_after = Some(3);
    r.replacement = Some(Box::new(Resource::new(new.clone()).etag("\"new\"")));
    ok(&e.run(&e.srv.add("/chg.bin", r), 90).await, &new);
    assert!(e.srv.requests("/chg.bin").iter().any(|r| r.if_range.as_deref() == Some("\"old\"")));
}

async fn pause_resume_case(validators: (&str, &str)) {
    let e = env().await;
    let b = Body::gen("pr", 10 * MIB);
    let mut r = Resource::new(b.clone()).throttle(384 * 1024);
    if !validators.0.is_empty() {
        r = r.etag(validators.0);
    }
    if !validators.1.is_empty() {
        r = r.last_modified(validators.1);
    }
    let id = e.submit(&e.srv.add("/pr.bin", r));
    e.m.wait_for(&id, &[S::Downloading], Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    e.m.pause(&id);
    assert_eq!(e.m.wait_for(&id, &[S::Paused], Duration::from_secs(5)).await.unwrap().state, S::Paused);
    let committed: u64 = e.store.load_segments(&id).unwrap().iter().map(|s| s.committed).sum();
    assert!(committed > 0);
    let before = e.srv.requests("/pr.bin").len();
    e.m.resume(&id);
    ok(&e.m.wait_for(&id, DONE, Duration::from_secs(60)).await.unwrap(), &b);
    let starts = range_starts(&e, "/pr.bin", before);
    assert!(!starts.is_empty() && *starts.iter().min().unwrap() > 0, "restarted from zero");
    let expected = if validators.0.starts_with('"') { Some(validators.0) } else if !validators.1.is_empty() { Some(validators.1) } else { None };
    for r in &e.srv.requests("/pr.bin")[before..] {
        if r.range.as_deref().is_some_and(|x| x != "bytes=0-0") {
            assert_eq!(r.if_range.as_deref(), expected);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_resume_strong_etag() {
    pause_resume_case(("\"strong\"", "")).await;
}
#[tokio::test(flavor = "multi_thread")]
async fn pause_resume_last_modified() {
    pause_resume_case(("", "Wed, 21 Oct 2025 07:28:00 GMT")).await;
}
#[tokio::test(flavor = "multi_thread")]
async fn pause_resume_weak_etag() {
    pause_resume_case(("W/\"weak\"", "")).await;
}
#[tokio::test(flavor = "multi_thread")]
async fn pause_resume_no_validators() {
    pause_resume_case(("", "")).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn overlap_detects_changed_content_without_validators() {
    let e = env().await;
    let a = Body::gen("same-size-a", 8 * MIB);
    let b = Body::gen("same-size-b", 8 * MIB);
    let id = e.submit(&e.srv.add("/nv.bin", Resource::new(a).throttle(384 * 1024)));
    e.m.wait_for(&id, &[S::Downloading], Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    e.m.pause(&id);
    e.m.wait_for(&id, &[S::Paused], Duration::from_secs(5)).await;
    e.srv.add("/nv.bin", Resource::new(b.clone()));
    e.m.resume(&id);
    ok(&e.m.wait_for(&id, DONE, Duration::from_secs(60)).await.unwrap(), &b);
}

#[tokio::test(flavor = "multi_thread")]
async fn chunked_gzip_redirects() {
    let e = env().await;
    let b = Body::gen("chunk", 3 * MIB);
    let mut r = Resource::new(b.clone()).no_ranges();
    r.chunked = true;
    let s = e.run(&e.srv.add("/ch.bin", r), 60).await;
    ok(&s, &b);
    assert_eq!(e.store.load_job(&s.id).unwrap().unwrap().size_verified, Some(false));
    let g = Body::gen("gz", 2 * MIB);
    let mut rg = Resource::new(g.clone());
    rg.gzip = true;
    ok(&e.run(&e.srv.add("/gz.bin", rg), 60).await, &g);
    let f = Body::gen("redir", 2 * MIB);
    e.srv.add("/final.bin", Resource::new(f.clone()));
    e.srv.add("/r2", Resource::redirect("/final.bin", 307));
    e.srv.add("/r1", Resource::redirect(&format!("{}/r2", e.srv.base), 301));
    let s = e.run(&format!("{}/r1", e.srv.base), 60).await;
    ok(&s, &f);
    assert!(s.final_path.unwrap().ends_with("final.bin"));
    e.srv.add("/la", Resource::redirect("/lb", 302));
    e.srv.add("/lb", Resource::redirect("/la", 302));
    let s = e.run(&format!("{}/la", e.srv.base), 30).await;
    assert_eq!(s.state, S::Failed);
    assert!(s.error.unwrap().message.to_lowercase().contains("redirect"));
}

#[tokio::test(flavor = "multi_thread")]
async fn client_errors_fail_without_retry_and_html_is_not_a_file() {
    let e = env().await;
    for (code, want) in [(404u16, ErrorCode::HttpStatus), (403, ErrorCode::Forbidden), (401, ErrorCode::Forbidden)] {
        let s = e.run(&e.srv.add(&format!("/e{code}"), Resource::new(Body::gen("e", 10)).statuses(&[code; 10])), 30).await;
        assert_eq!((s.state, s.error.as_ref().unwrap().code, s.attempts), (S::Failed, want, 1));
    }
    let s = e.run(&e.srv.add("/page", Resource::new(Body::raw(b"<html>x</html>".to_vec())).content_type("text/html; charset=utf-8")), 30).await;
    assert_eq!((s.state, s.error.unwrap().code), (S::Failed, ErrorCode::NotAFile));
    assert!(e.files().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn persistent_failure_never_completes() {
    let e = env().await;
    let r = Resource::new(Body::gen("dead", 8 * MIB)).etag("\"d\"").fault(Fault::cut(0).times(-1).when(|_, r| r != Some("bytes=0-0")));
    let s = e.run(&e.srv.add("/dead.bin", r), 120).await;
    assert!(s.state == S::Failed && s.error.unwrap().retryable);
    assert!(!e.files().contains(&"dead.bin".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn same_name_traversal_zero_byte() {
    let e = env().await;
    let a = Body::gen("A", MIB);
    let b = Body::gen("B", MIB);
    let s1 = e.run(&e.srv.add("/v1/data.bin", Resource::new(a.clone())), 30).await;
    let s2 = e.run(&e.srv.add("/v2/data.bin", Resource::new(b.clone())), 30).await;
    ok(&s1, &a);
    ok(&s2, &b);
    assert!(s2.final_path.unwrap().ends_with("data (1).bin"));
    let c = Body::gen("cd", 1000);
    let s = e.run(&e.srv.add("/get", Resource::new(c.clone()).disposition(r#"attachment; filename="..\\..\\evil.exe""#)), 30).await;
    ok(&s, &c);
    assert_eq!(s.final_path.unwrap(), format!("{}\\evil.exe", e.dest));
    let s = e.run(&e.srv.add("/empty.txt", Resource::new(Body::gen("z", 0))), 30).await;
    assert_eq!((s.state, s.total), (S::Completed, Some(0)));
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_stops_bytes_and_cancel_cleans_up() {
    let e = env().await;
    let id = e.submit(&e.srv.add("/t.bin", Resource::new(Body::gen("t", 40 * MIB)).etag("\"t\"").throttle(2 * MIB)));
    e.m.wait_for(&id, &[S::Downloading], Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let t0 = Instant::now();
    e.m.pause(&id);
    e.m.wait_for(&id, &[S::Paused], Duration::from_secs(5)).await;
    assert!(t0.elapsed() < Duration::from_secs(2));
    let (n, sent) = (e.srv.requests("/t.bin").len(), e.srv.bytes_sent("/t.bin"));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(e.srv.requests("/t.bin").len(), n);
    assert!(e.srv.bytes_sent("/t.bin") - sent <= 8 * 2 * 64 * 1024);
    let sent = e.srv.bytes_sent("/t.bin");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(e.srv.bytes_sent("/t.bin"), sent);
    e.m.cancel(&id);
    assert_eq!(e.m.wait_for(&id, &[S::Cancelled], Duration::from_secs(5)).await.unwrap().state, S::Cancelled);
    assert!(e.files().is_empty() && e.store.load_segments(&id).unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn insufficient_space_and_disk_full_mid_write() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let full = Arc::new(AtomicBool::new(true));
    let written = Arc::new(AtomicU64::new(0));
    let (f2, w2) = (full.clone(), written.clone());
    let e = env_with(
        move |eng| {
            eng.write_hook = Some(Arc::new(move |n| {
                if f2.load(Ordering::SeqCst) && w2.fetch_add(n as u64, Ordering::SeqCst) + n as u64 > 3 * MIB {
                    return Err(std::io::Error::from_raw_os_error(112));
                }
                Ok(())
            }))
        },
        Settings::default(),
    )
    .await;
    let b = Body::gen("df", 10 * MIB);
    let id = e.submit(&e.srv.add("/df.bin", Resource::new(b.clone()).etag("\"df\"")));
    let s = e.m.wait_for(&id, &[S::Paused, S::Failed], Duration::from_secs(20)).await.unwrap();
    assert_eq!((s.state, s.reason.as_deref()), (S::Paused, Some("disk_full")));
    full.store(false, Ordering::SeqCst);
    e.m.resume(&id);
    ok(&e.m.wait_for(&id, DONE, Duration::from_secs(30)).await.unwrap(), &b);
}

#[tokio::test(flavor = "multi_thread")]
async fn cookies_and_site_logins_are_scoped() {
    let mut st = Settings::default();
    st.site_logins.push(SiteLogin {
        host: "127.0.0.1".into(),
        username: "user".into(),
        password_enc: swiftdm_core::secrets::protect_str("pw").unwrap(),
    });
    let e = env_with(|_| {}, st).await;
    let b = Body::gen("auth", MIB);
    let mut r = Resource::new(b.clone());
    r.require_basic = Some("Basic dXNlcjpwdw==".into());
    ok(&e.run(&e.srv.add("/auth.bin", r), 30).await, &b);
    let c = Body::gen("cookie", MIB);
    let mut rc = Resource::new(c.clone());
    rc.require_cookie = Some("SID=abc".into());
    let mut spec = JobSpec::new(e.srv.add("/c.bin", rc), &e.dest);
    spec.kind = "http".into();
    spec.cookies = Some("SID=abc".into());
    let id = e.m.submit(spec, StartMode::Now).unwrap();
    ok(&e.m.wait_for(&id, DONE, Duration::from_secs(30)).await.unwrap(), &c);
    // A redirect to another host must not receive the cookie.
    let other = TestServer::start().await;
    let d = Body::gen("other", 1000);
    let target = other.add("/x.bin", Resource::new(d.clone()));
    let target = target.replace("127.0.0.1", "localhost");
    e.srv.add("/hop", Resource::redirect(&target, 302));
    let mut spec = JobSpec::new(format!("{}/hop", e.srv.base), &e.dest);
    spec.kind = "http".into();
    spec.cookies = Some("SID=abc".into());
    let id = e.m.submit(spec, StartMode::Now).unwrap();
    ok(&e.m.wait_for(&id, DONE, Duration::from_secs(30)).await.unwrap(), &d);
    assert!(other.requests("/x.bin").iter().all(|r| r.cookie.is_none() && r.authorization.is_none()));
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_link_choices() {
    let e = env().await;
    let b = Body::gen("dup", 256 * 1024);
    let url = e.srv.add("/dup.bin", Resource::new(b.clone()).etag("\"d\""));
    let first = e.run(&url, 30).await;
    ok(&first, &b);
    let add = |policy: &str| e.m.submit_or_reuse(JobSpec::new(url.as_str(), &e.dest), StartMode::Now, policy).unwrap();
    // "resume": the existing (finished) download is used, nothing is added.
    assert_eq!(add("resume"), (first.id.clone(), true));
    // "number": a second copy under a numbered name.
    let (n, existing) = add("number");
    assert!(!existing);
    let s = e.m.wait_for(&n, DONE, Duration::from_secs(30)).await.unwrap();
    ok(&s, &b);
    assert_eq!(e.files(), vec!["dup (1).bin", "dup.bin"]);
    // "overwrite": the existing downloads and their files are replaced by one new download.
    let (o, _) = add("overwrite");
    let s = e.m.wait_for(&o, DONE, Duration::from_secs(30)).await.unwrap();
    ok(&s, &b);
    e.m.sync().await;
    assert_eq!(e.m.find_duplicates(&url).len(), 1);
    assert_eq!(e.files(), vec!["dup.bin"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn relocate_while_downloading_and_after_completion() {
    // IDM starts downloading while "Download File Info" is open; the folder and name chosen
    // there are applied afterwards.
    let e = env().await;
    let b = Body::gen("reloc", 12 * MIB);
    let id = e.submit(&e.srv.add("/reloc.bin", Resource::new(b.clone()).etag("\"r\"").throttle(2 * MIB)));
    e.m.wait_for(&id, &[S::Downloading], Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    let other = std::path::Path::new(&e.dest).parent().unwrap().join("Elsewhere");
    e.m.relocate(&id, other.to_str().unwrap(), Some("renamed.bin".into()));
    let s = e.m.wait_for(&id, DONE, Duration::from_secs(60)).await.unwrap();
    ok(&s, &b);
    assert_eq!(s.final_path.as_deref(), Some(other.join("renamed.bin").to_str().unwrap()));
    assert!(e.files().is_empty(), "nothing left in the first folder: {:?}", e.files());
    // A finished file moves too.
    let third = std::path::Path::new(&e.dest).parent().unwrap().join("Third");
    e.m.relocate(&id, third.to_str().unwrap(), None);
    e.m.sync().await;
    let s = e.m.get(&id).unwrap();
    assert_eq!(s.final_path.as_deref(), Some(third.join("renamed.bin").to_str().unwrap()));
    assert!(third.join("renamed.bin").is_file() && !other.join("renamed.bin").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn overall_speed_limit_is_shared_fairly() {
    let mut st = Settings::default();
    st.speed_limit = 4 * MIB;
    let e = env_with(|_| {}, st).await;
    let a = e.submit(&e.srv.add("/fa.bin", Resource::new(Body::gen("fa", 64 * MIB)).etag("\"a\"")));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let b = e.submit(&e.srv.add("/fb.bin", Resource::new(Body::gen("fb", 64 * MIB)).etag("\"b\"")));
    e.m.wait_for(&b, &[S::Downloading], Duration::from_secs(10)).await;
    let got = |id: &str| e.m.progress(id).map(|p| p.downloaded).unwrap_or(0);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (a0, b0) = (got(&a), got(&b));
    tokio::time::sleep(Duration::from_secs(4)).await;
    let (da, db) = ((got(&a) - a0) as f64, (got(&b) - b0) as f64);
    println!("first {:.1} MB, second {:.1} MB", da / MIB as f64, db / MIB as f64);
    assert!(db > da / 2.0 && da > db / 2.0, "unfair: {da} vs {db}");
}

#[tokio::test(flavor = "multi_thread")]
async fn speed_limit_and_refresh_address() {
    let mut st = Settings::default();
    st.speed_limit = 2 * MIB;
    let e = env_with(|_| {}, st).await;
    let b = Body::gen("lim", 6 * MIB);
    let t0 = Instant::now();
    ok(&e.run(&e.srv.add("/lim.bin", Resource::new(b.clone())), 60).await, &b);
    let secs = t0.elapsed().as_secs_f64();
    assert!(secs > 2.0, "speed limit not applied: {secs}s");
    // Refresh: expired link -> new link for the same file keeps partial data.
    e.m.settings().update(|s| s.speed_limit = 0).unwrap();
    let big = Body::gen("refresh", 10 * MIB);
    let id = e.submit(&e.srv.add("/old.bin", Resource::new(big.clone()).throttle(256 * 1024)));
    e.m.wait_for(&id, &[S::Downloading], Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    e.m.pause(&id);
    e.m.wait_for(&id, &[S::Paused], Duration::from_secs(5)).await;
    let other = e.srv.add("/other.bin", Resource::new(Body::gen("x", 3 * MIB)));
    let r = e.m.refresh_url(&id, &other).await;
    assert!(r.as_ref().is_err_and(|m| m.contains("different file")), "{r:?} {:?}", e.store.load_job(&id).unwrap().unwrap().total_bytes);
    let new_url = e.srv.add("/new.bin", Resource::new(big.clone()));
    e.m.refresh_url(&id, &new_url).await.unwrap();
    e.m.resume(&id);
    let s = e.m.wait_for(&id, DONE, Duration::from_secs(60)).await.unwrap();
    ok(&s, &big);
    assert!(range_starts(&e, "/new.bin", 0).iter().all(|&x| x > 0));
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_middle_segment_is_split_and_file_is_exact() {
    // 8 segments of 3 MiB; the one starting at 9 MiB stalls, so the others finish first and
    // the stalled segment is split while it runs (the new piece lies mid-file).
    let e = env().await;
    let b = Body::gen("split-mid", 24 * MIB);
    let r = Resource::new(b.clone())
        .etag("\"m\"")
        .fault(Fault::stall(256 * 1024, Duration::from_secs(4)).when(|_, r| r.is_some_and(|r| r.starts_with(&format!("bytes={}-", 9 * MIB)))));
    let s = e.run(&e.srv.add("/mid.bin", r), 90).await;
    ok(&s, &b);
    let starts = range_starts(&e, "/mid.bin", 0);
    assert!(starts.iter().any(|&x| x > 9 * MIB && x < 12 * MIB), "no split inside the stalled segment: {starts:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn mirrors_with_different_files_fall_back_to_one_connection() {
    // Requests that start past byte 0 reach a mirror holding a different file of the same size.
    let e = env().await;
    let a = Body::gen("mirror-a", 16 * MIB);
    let r = Resource::new(a.clone()).mirror(Body::gen("mirror-b", 16 * MIB));
    let s = e.run(&e.srv.add("/mirror.bin", r), 90).await;
    ok(&s, &a);
}

#[tokio::test(flavor = "multi_thread")]
async fn per_server_connection_limit() {
    use swiftdm_core::settings::ServerLimit;
    let e = env_with(
        |_| {},
        Settings { server_limits: vec![ServerLimit { host: "127.0.0.1".into(), max_connections: 2 }], ..Settings::default() },
    )
    .await;
    let b = Body::gen("limited", 16 * MIB);
    ok(&e.run(&e.srv.add("/lim.bin", Resource::new(b.clone()).etag("\"l\"")), 60).await, &b);
    let starts = range_starts(&e, "/lim.bin", 0);
    // Planned as two halves; an 8-way plan would also start at 2, 4 and 6 MiB.
    assert!(starts.contains(&(8 * MIB)), "{starts:?}");
    assert!(!starts.contains(&(2 * MIB)) && !starts.contains(&(6 * MIB)), "{starts:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn download_quota_stops_downloads_and_releases() {
    use swiftdm_core::events::Event;
    let e = env_with(|_| {}, Settings { quota_mb: 2, quota_hours: 1, ..Settings::default() }).await;
    let mut events = e.m.subscribe();
    let b = Body::gen("quota", 8 * MIB);
    let id = e.submit(&e.srv.add("/q.bin", Resource::new(b.clone()).etag("\"q\"").throttle(4 * MIB)));
    let s = e.m.wait_for(&id, &[S::Paused, S::Completed], Duration::from_secs(30)).await.unwrap();
    assert_eq!(s.state, S::Paused, "the quota should stop the download");
    let st = e.m.quota_status();
    assert!(st.blocked && st.used >= 2 * MIB && st.limit == 2 * MIB, "{st:?}");
    let mut saw = false;
    while let Ok(ev) = events.try_recv() {
        if let Event::QuotaExceeded { resume_in_secs, .. } = ev {
            assert!(resume_in_secs > 3000.0, "{resume_in_secs}");
            saw = true;
        }
    }
    assert!(saw, "QuotaExceeded was not published");
    // New downloads do not start while blocked.
    let id2 = e.submit(&e.srv.add("/q2.bin", Resource::new(Body::gen("q2", MIB))));
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(e.m.get(&id2).unwrap().state, S::Queued);
    // Raising the quota releases it and the stopped download resumes by itself.
    e.m.settings().update(|s| s.quota_mb = 0).unwrap();
    ok(&e.m.wait_for(&id, DONE, Duration::from_secs(60)).await.unwrap(), &b);
    assert_eq!(e.m.wait_for(&id2, DONE, Duration::from_secs(30)).await.unwrap().state, S::Completed);
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicates_are_found_by_address() {
    let e = env().await;
    let url = e.srv.add("/dup.bin", Resource::new(Body::gen("dup", MIB)));
    assert!(e.m.find_duplicates(&url).is_empty());
    let id = e.submit(&url);
    e.m.wait_for(&id, DONE, Duration::from_secs(30)).await;
    assert_eq!(e.m.find_duplicates(&url).len(), 1);
    e.m.remove(&id, false);
    e.m.sync().await;
    assert!(e.m.find_duplicates(&url).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn per_download_speed_limit_changes_while_running() {
    let e = env().await;
    let b = Body::gen("live-limit", 64 * MIB);
    let id = e.submit(&e.srv.add("/ll.bin", Resource::new(b.clone()).etag("\"ll\"").throttle(8 * MIB)));
    e.m.wait_for(&id, &[S::Downloading], Duration::from_secs(10)).await;
    e.m.set_speed_limit(&id, Some(MIB));
    tokio::time::sleep(Duration::from_millis(500)).await;
    let a = e.m.progress(&id).unwrap().downloaded;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let rate = (e.m.progress(&id).unwrap().downloaded - a) as f64 / 2.0;
    assert!(rate < 1.6 * MIB as f64, "limit not applied: {rate} B/s");
    assert_eq!(e.m.get(&id).unwrap().state, S::Downloading);
    e.m.set_speed_limit(&id, None);
    ok(&e.m.wait_for(&id, DONE, Duration::from_secs(90)).await.unwrap(), &b);
}

/// Needs the internet: `cargo test -p swiftdm-core --test http_matrix -- --ignored zip`.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn zip_preview_of_a_real_archive() {
    let url = "https://www.python.org/ftp/python/3.12.0/python-3.12.0-embed-amd64.zip";
    let s = Settings { proxy_mode: ProxyMode::None, ..Settings::default() };
    let total = swiftdm_core::http::probe::probe_url(url, &s, &swiftdm_core::cancel::CancelToken::new(), None).await.unwrap().total.unwrap();
    let files = swiftdm_core::http::zip_preview::preview(url, &s, None, total).await.unwrap();
    assert!(files.iter().any(|f| f.name == "python.exe" && f.size > 50_000), "{files:?}");
    println!("{} files, e.g. {:?}", files.len(), &files[..3]);
}

#[tokio::test(flavor = "multi_thread")]
async fn synchronization_queue_downloads_changed_files_again_in_place() {
    use swiftdm_core::scheduler::{MAIN_QUEUE, SYNC_QUEUE};
    let e = env().await;
    let (a1, b1) = (Body::gen("sync-a1", MIB), Body::gen("sync-b", MIB / 2));
    let a = e.run(&e.srv.add("/sync/a.bin", Resource::new(a1.clone()).etag("\"a1\"")), 30).await;
    let b = e.run(&e.srv.add("/sync/b.bin", Resource::new(b1.clone()).etag("\"b\"")), 30).await;
    ok(&a, &a1);
    ok(&b, &b1);
    // Finished files only go into synchronization queues.
    e.m.move_to_queue(&a.id, Some(MAIN_QUEUE.into()));
    e.m.sync().await;
    assert_eq!(e.m.get(&a.id).unwrap().queue_id, None);
    e.m.move_to_queue(&a.id, Some(SYNC_QUEUE.into()));
    e.m.move_to_queue(&b.id, Some(SYNC_QUEUE.into()));
    // a.bin changes on the server; b.bin does not.
    let a2 = Body::gen("sync-a2", MIB + 12345);
    e.srv.add("/sync/a.bin", Resource::new(a2.clone()).etag("\"a2\""));
    e.m.start_queue(SYNC_QUEUE);
    e.m.sync().await; // the queue is running from here on
    let deadline = Instant::now() + Duration::from_secs(30);
    while e.m.queues().iter().any(|q| q.id == SYNC_QUEUE && q.running) {
        assert!(Instant::now() < deadline, "the synchronization queue did not finish");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    e.m.sync().await;
    let list = e.m.snapshot();
    assert!(e.m.get(&a.id).is_none(), "the old entry of the changed file is gone");
    let new_a = list.iter().find(|s| s.url.ends_with("/sync/a.bin")).expect("a.bin is still in the list");
    ok(new_a, &a2);
    assert_eq!(new_a.final_path, a.final_path, "downloaded again over the same file");
    assert_eq!(new_a.queue_id.as_deref(), Some(SYNC_QUEUE), "and it stays in the queue");
    assert_eq!(e.m.get(&b.id).unwrap().final_path, b.final_path, "the unchanged file was left alone");
    assert_eq!(e.files(), vec!["a.bin", "b.bin"]);
    // Nothing changed since: a second run leaves everything as it is.
    e.m.start_queue(SYNC_QUEUE);
    e.m.sync().await; // the queue is running from here on
    let deadline = Instant::now() + Duration::from_secs(30);
    while e.m.queues().iter().any(|q| q.id == SYNC_QUEUE && q.running) {
        assert!(Instant::now() < deadline, "the second run did not finish");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    e.m.sync().await;
    assert!(e.m.get(&new_a.id).is_some() && e.m.get(&b.id).is_some());
    assert_eq!(e.m.snapshot().len(), 2);
}
