//! Media engine against a stand-in yt-dlp (no network): success, refusals, errors, pause and
//! resume, retries and the format list.
mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use swiftdm_core::engine::Engine;
use swiftdm_core::errors::ErrorCode;
use swiftdm_core::manager::{Manager, ManagerConfig, StartMode};
use swiftdm_core::media::{MediaEngine, Tools};
use swiftdm_core::models::{JobSnapshot, JobSpec, JobState as S};
use swiftdm_core::settings::{Settings, SettingsStore};
use swiftdm_core::store::Store;

const DONE: &[S] = &[S::Completed, S::Failed, S::Cancelled];

fn fake() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sdm-fake-ytdlp"))
}

fn engine() -> MediaEngine {
    // FFmpeg is only checked for presence here; the fake yt-dlp "merges" by itself. No
    // ffprobe, so files are not media-verified in these tests.
    MediaEngine::new(Tools { ytdlp: Some(fake()), ffmpeg: Some(fake()), ffprobe: None, js: None })
}

struct Env {
    dir: tempfile::TempDir,
    m: Manager,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("SWIFTDM_HOME", dir.path().join("home"));
    let store = Arc::new(Store::open(dir.path().join("s.db")).unwrap());
    let engines: HashMap<String, Arc<dyn Engine>> = HashMap::from([("media".to_string(), Arc::new(engine()) as Arc<dyn Engine>)]);
    let s = SettingsStore::in_memory(Settings { download_dir: dir.path().join("dl").to_string_lossy().into_owned(), retries: 3, ..Settings::default() });
    let m = Manager::start(store, engines, s, ManagerConfig { retry_base: Duration::from_millis(100), scheduler_ticker: false, ..Default::default() }).unwrap();
    Env { dir, m }
}

impl Env {
    fn dest(&self) -> String {
        self.dir.path().join("dl").to_string_lossy().into_owned()
    }
    fn submit(&self, path: &str) -> String {
        let mut spec = JobSpec::new(format!("https://fake.test{path}"), self.dest());
        spec.kind = "media".into();
        spec.format_label = "720".into();
        self.m.submit(spec, StartMode::Now).unwrap()
    }
    async fn run(&self, path: &str) -> JobSnapshot {
        let id = self.submit(path);
        self.m.wait_for(&id, DONE, Duration::from_secs(60)).await.unwrap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_merges_and_cleans_up() {
    let e = env();
    let s = e.run("/ok").await;
    assert_eq!(s.state, S::Completed, "{:?}", s.error);
    let p = s.final_path.unwrap();
    assert!(p.ends_with("Fake Video.mp4"), "{p}");
    assert_eq!(std::fs::metadata(&p).unwrap().len(), 2_000_000);
    assert_eq!(s.title, "Fake Video");
    let leftovers: Vec<_> = std::fs::read_dir(e.dest()).unwrap().map(|d| d.unwrap().file_name()).collect();
    assert_eq!(leftovers.len(), 1, "temporary folder must be removed: {leftovers:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_private_drm_playlists_and_live() {
    let e = env();
    for (path, code) in [("/private", ErrorCode::Unavailable), ("/drm", ErrorCode::Unsupported), ("/playlist", ErrorCode::Unsupported), ("/live", ErrorCode::Unsupported)] {
        let s = e.run(path).await;
        assert_eq!(s.state, S::Failed, "{path}");
        let err = s.error.unwrap();
        assert_eq!(err.code, code, "{path}: {}", err.message);
        assert!(!err.retryable, "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_and_resume_continue_partial_files() {
    let e = env();
    let id = e.submit("/slow");
    e.m.wait_for(&id, &[S::Downloading], Duration::from_secs(20)).await;
    tokio::time::sleep(Duration::from_millis(900)).await;
    e.m.pause(&id);
    let s = e.m.wait_for(&id, &[S::Paused], Duration::from_secs(10)).await.unwrap();
    assert_eq!(s.state, S::Paused);
    let parts: Vec<String> = walk(&e.dest()).into_iter().filter(|p| p.ends_with(".part")).collect();
    assert!(!parts.is_empty(), "partial data must be kept while paused");
    e.m.resume(&id);
    let s = e.m.wait_for(&id, DONE, Duration::from_secs(60)).await.unwrap();
    assert_eq!(s.state, S::Completed, "{:?}", s.error);
    assert_eq!(std::fs::metadata(s.final_path.unwrap()).unwrap().len(), 2_000_000, "resume must not duplicate data");
}

#[tokio::test(flavor = "multi_thread")]
async fn temporary_errors_are_retried() {
    let e = env();
    let s = e.run("/flaky").await;
    assert_eq!(s.state, S::Completed, "{:?}", s.error);
    assert!(s.attempts >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_formats_for_the_picker() {
    let _e = env();
    let info = engine().inspect("https://fake.test/ok", None, &direct()).await.unwrap();
    assert_eq!(info.title, "Fake Video");
    let heights: Vec<Option<u32>> = info.formats.iter().map(|f| f.height).collect();
    assert_eq!(heights, vec![Some(720), Some(360), None]);
    assert!(engine().inspect("https://fake.test/private", None, &direct()).await.unwrap_err().contains("private"));
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_tool_is_reported() {
    let e = MediaEngine::new(Tools::default());
    assert!(e.inspect("https://fake.test/ok", None, &direct()).await.unwrap_err().contains("yt-dlp was not found"));
}

fn walk(dir: &str) -> Vec<String> {
    let mut out = vec![];
    for d in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = d.path();
        if p.is_dir() {
            out.extend(walk(&p.to_string_lossy()));
        } else {
            out.push(p.to_string_lossy().into_owned());
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn ytdlp_update_is_verified_before_use() {
    use axum::routing::get;
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let exe = std::fs::read(fake()).unwrap();
    let good = format!("{}  yt-dlp.exe\n", hex::encode(Sha256::digest(&exe)));
    let serve = |sums: String| {
        let exe = exe.clone();
        async move {
            let app = axum::Router::new()
                .route("/SHA2-256SUMS", get(move || async move { sums }))
                .route("/yt-dlp.exe", get(move || async move { exe }));
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}/", l.local_addr().unwrap());
            tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
            base
        }
    };
    let bad = serve(format!("{}  yt-dlp.exe\n", "0".repeat(64))).await;
    assert!(swiftdm_core::media::tools::update_ytdlp_in(dir.path(), &bad).await.unwrap_err().contains("SHA-256"));
    let ok = serve(good).await;
    assert_eq!(swiftdm_core::media::tools::update_ytdlp_in(dir.path(), &ok).await.unwrap(), "2026.01.01-fake");
    assert!(dir.path().join("yt-dlp.exe").is_file());
}

fn direct() -> swiftdm_core::settings::Settings {
    swiftdm_core::settings::Settings { proxy_mode: swiftdm_core::settings::ProxyMode::None, ..Default::default() }
}

#[tokio::test(flavor = "multi_thread")]
async fn ytdlp_gets_the_proxy_from_options() {
    let manual = swiftdm_core::settings::Settings {
        proxy_mode: swiftdm_core::settings::ProxyMode::Manual,
        proxy_url: "px.test:3128".into(),
        ..Default::default()
    };
    engine().inspect("https://fake.test/needs-proxy", None, &manual).await.unwrap();
    assert!(engine().inspect("https://fake.test/needs-proxy", None, &direct()).await.is_err(), "no proxy when Options say none");
}
