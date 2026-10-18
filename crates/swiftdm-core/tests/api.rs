//! Extension API: security checks, pairing, confirmation flow, capture decisions, links.
mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use swiftdm_core::api::{self, ApiHandle, ApiHooks, DownloadRequest, Overrides};
use swiftdm_core::api::auth::PairingStore;
use swiftdm_core::manager::Manager;
use swiftdm_core::settings::{Settings, SettingsStore};
use swiftdm_core::store::Store;

const EXT: &str = "abcdefghijklmnopabcdefghijklmnop";
const OTHER_EXT: &str = "ponmlkjihgfedcbaponmlkjihgfedcba";

#[derive(Default)]
struct Hooks {
    codes: Mutex<Vec<(String, String)>>,
    confirms: Mutex<Vec<(String, DownloadRequest)>>,
    links: Mutex<Vec<Vec<String>>>,
    started: Mutex<Vec<String>>,
}

impl ApiHooks for Hooks {
    fn show_pairing(&self, pairing_id: &str, _ext: &str, code: &str) {
        self.codes.lock().unwrap().push((pairing_id.into(), code.into()));
    }
    fn close_pairing(&self, _pairing_id: &str) {}
    fn confirm(&self, request_id: &str, req: &DownloadRequest) {
        self.confirms.lock().unwrap().push((request_id.into(), req.clone()));
    }
    fn links(&self, urls: Vec<String>, _page: Option<String>) {
        self.links.lock().unwrap().push(urls);
    }
    fn started(&self, job_id: &str) {
        self.started.lock().unwrap().push(job_id.into());
    }
}

struct Env {
    _dir: tempfile::TempDir,
    mgr: Manager,
    api: ApiHandle,
    hooks: Arc<Hooks>,
    http: reqwest::Client,
    base: String,
}

async fn env(trust: bool) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("s.db")).unwrap());
    let s = SettingsStore::in_memory(Settings { download_dir: dir.path().join("dl").to_string_lossy().into_owned(), trust_extension: trust, ..Settings::default() });
    let mgr = common::manager(store, Arc::new(common::FakeEngine::default()), s);
    let hooks = Arc::new(Hooks::default());
    let media = swiftdm_core::media::MediaEngine::new(swiftdm_core::media::Tools {
        ytdlp: Some(env!("CARGO_BIN_EXE_sdm-fake-ytdlp").into()),
        ..Default::default()
    });
    let api = api::start(mgr.clone(), PairingStore::open(dir.path().join("pairings.json")), hooks.clone(), Some(Arc::new(media)), 0).await.unwrap();
    let base = format!("http://127.0.0.1:{}", api.port());
    Env { _dir: dir, mgr, api, hooks, http: reqwest::Client::new(), base }
}

impl Env {
    async fn call(&self, method: &str, path: &str, ext: Option<&str>, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
        let mut r = self.http.request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(e) = ext {
            r = r.header("Origin", format!("chrome-extension://{e}"));
        }
        if let Some(t) = token {
            r = r.header("Authorization", format!("Bearer {t}"));
        }
        if let Some(b) = body {
            r = r.header("Content-Type", "application/json").body(b.to_string());
        }
        let resp = r.send().await.unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn pair(&self) -> String {
        let (s, v) = self.call("POST", "/api/pair", Some(EXT), None, None).await;
        assert_eq!(s, 202, "{v}");
        let pid = v["pairing_id"].as_str().unwrap().to_string();
        let code = self.hooks.codes.lock().unwrap().last().unwrap().1.clone();
        let (s, v) = self.call("POST", "/api/pair/confirm", Some(EXT), None, Some(json!({ "pairing_id": pid, "code": code }))).await;
        assert_eq!(s, 200, "{v}");
        v["token"].as_str().unwrap().to_string()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_foreign_hosts_origins_and_unpaired_callers() {
    let e = env(false).await;
    let r = e.http.get(format!("{}/health", e.base)).header("Host", "evil.example:5001").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403, "DNS-rebinding host must be refused");
    let r = e.http.get(format!("{}/health", e.base)).header("Origin", "https://evil.example").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403, "web pages must be refused");
    assert!(r.headers().get("access-control-allow-origin").is_none());
    let (s, v) = e.call("GET", "/health", None, None, None).await;
    assert_eq!((s, v["app"].as_str(), v["paired"].as_bool()), (200, Some("SwiftHarryDM"), Some(false)));
    let (s, _) = e.call("POST", "/api/download", Some(EXT), None, Some(json!({ "url": "https://a.com/x.zip" }))).await;
    assert_eq!(s, 401);
    let (s, _) = e.call("POST", "/api/pair", None, None, None).await;
    assert_eq!(s, 403, "pairing needs an extension origin");
    let (s, v) = e.call("GET", "/nope", None, None, None).await;
    assert_eq!((s, v["success"].as_bool()), (404, Some(false)));
    let big = "x".repeat(100 * 1024);
    let r = e.http.post(format!("{}/api/download", e.base)).header("Content-Type", "application/json").body(big).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 413);
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_issues_a_token_bound_to_the_extension() {
    let e = env(false).await;
    let (_, v) = e.call("POST", "/api/pair", Some(EXT), None, None).await;
    let pid = v["pairing_id"].as_str().unwrap().to_string();
    let (s, _) = e.call("POST", "/api/pair/confirm", Some(EXT), None, Some(json!({ "pairing_id": pid, "code": "not-it" }))).await;
    assert_eq!(s, 401);
    let token = {
        let code = e.hooks.codes.lock().unwrap().last().unwrap().1.clone();
        let (s, v) = e.call("POST", "/api/pair/confirm", Some(EXT), None, Some(json!({ "pairing_id": pid, "code": code }))).await;
        assert_eq!(s, 200);
        v["token"].as_str().unwrap().to_string()
    };
    let (_, v) = e.call("GET", "/health", Some(EXT), Some(&token), None).await;
    assert_eq!(v["paired"].as_bool(), Some(true));
    let (s, _) = e.call("GET", "/api/jobs", Some(OTHER_EXT), Some(&token), None).await;
    assert_eq!(s, 401, "a token is only valid for the extension it was issued to");
    assert_eq!(e.api.paired(), vec![EXT.to_string()]);
    e.api.revoke(Some(EXT));
    let (s, _) = e.call("GET", "/api/jobs", Some(EXT), Some(&token), None).await;
    assert_eq!(s, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_wait_for_confirmation_in_the_app() {
    let e = env(false).await;
    let t = e.pair().await;
    let body = json!({ "url": "http://dl.example/file.zip?size=500", "fileName": "file.zip", "fileSize": 500, "cookies": "sid=1", "pageUrl": "https://dl.example/page", "mode": "now" });
    let (s, v) = e.call("POST", "/api/download", Some(EXT), Some(&t), Some(body)).await;
    assert_eq!((s, v["status"].as_str()), (202, Some("pending_confirmation")));
    let rid = v["request_id"].as_str().unwrap().to_string();
    let (rid2, req) = e.hooks.confirms.lock().unwrap()[0].clone();
    assert_eq!((rid2.as_str(), req.cookies.as_deref(), req.file_name.as_deref()), (rid.as_str(), Some("sid=1"), Some("file.zip")));
    let (_, v) = e.call("GET", &format!("/api/requests/{rid}"), Some(EXT), Some(&t), None).await;
    assert_eq!(v["status"].as_str(), Some("pending"));
    let id = e.api.resolve(&rid, Overrides { description: Some("from test".into()), ..Default::default() }).unwrap();
    assert_eq!(e.hooks.started.lock().unwrap().as_slice(), [id.clone()]);
    let (_, v) = e.call("GET", &format!("/api/requests/{rid}"), Some(EXT), Some(&t), None).await;
    assert_eq!((v["status"].as_str(), v["job_id"].as_str()), (Some("accepted"), Some(id.as_str())));
    let snap = e.mgr.wait_for(&id, &[swiftdm_core::models::JobState::Completed], Duration::from_secs(10)).await.unwrap();
    assert_eq!((snap.source.as_str(), snap.category.as_deref(), snap.description.as_deref()), ("extension", Some("Compressed"), Some("from test")));
    assert_eq!(snap.referer.as_deref(), Some("https://dl.example/page"));
    assert!(e.api.resolve(&rid, Overrides::default()).is_err(), "a request is accepted only once");
    // Rejected requests report it.
    let (_, v) = e.call("POST", "/api/download", Some(EXT), Some(&t), Some(json!({ "url": "http://dl.example/b.zip" }))).await;
    let rid = v["request_id"].as_str().unwrap().to_string();
    e.api.reject(&rid);
    let (_, v) = e.call("GET", &format!("/api/requests/{rid}"), Some(EXT), Some(&t), None).await;
    assert_eq!(v["status"].as_str(), Some("rejected"));
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_extension_starts_downloads_directly() {
    let e = env(true).await;
    let t = e.pair().await;
    let (s, v) = e.call("POST", "/api/download", Some(EXT), Some(&t), Some(json!({ "url": "http://dl.example/a.zip?size=10", "mode": "now" }))).await;
    assert_eq!((s, v["status"].as_str()), (200, Some("started")));
    assert_eq!(e.hooks.started.lock().unwrap().len(), 1);
    let (_, v) = e.call("POST", "/api/download", Some(EXT), Some(&t), Some(json!({ "url": "http://dl.example/b.zip?size=10" }))).await;
    assert_eq!(v["status"].as_str(), Some("queued"));
    let (s, v) = e.call("POST", "/api/download", Some(EXT), Some(&t), Some(json!({ "url": "http://dl.example/c.zip", "extra": 1 }))).await;
    assert_eq!(s, 400, "{v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn capture_decisions_and_link_lists() {
    let e = env(false).await;
    let t = e.pair().await;
    let check = |body: Value| e.call("POST", "/api/capture/check", Some(EXT), Some(&t), Some(body));
    assert_eq!(check(json!({ "url": "https://a.com/x.zip" })).await.1["capture"].as_bool(), Some(true));
    assert_eq!(check(json!({ "url": "https://a.com/page.html" })).await.1["capture"].as_bool(), Some(false));
    assert_eq!(check(json!({ "url": "https://a.com/page.html", "forced": true })).await.1["capture"].as_bool(), Some(true));
    assert_eq!(check(json!({ "url": "https://a.com/get?id=7", "fileName": "Report.pdf" })).await.1["capture"].as_bool(), Some(true));
    assert_eq!(check(json!({ "url": "https://a.com/get?id=8", "mime": "application/zip" })).await.1["capture"].as_bool(), Some(true));
    let (s, v) = e.call("POST", "/api/links", Some(EXT), Some(&t), Some(json!({ "urls": ["https://a.com/1.zip", "javascript:x", "https://a.com/2.zip", "https://a.com/1.zip"] }))).await;
    assert_eq!((s, v["count"].as_u64()), (202, Some(2)));
    assert_eq!(e.hooks.links.lock().unwrap()[0], vec!["https://a.com/1.zip".to_string(), "https://a.com/2.zip".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn video_formats_and_exact_format_downloads() {
    let e = env(true).await;
    let t = e.pair().await;
    let (s, v) = e.call("POST", "/api/media/info", Some(EXT), Some(&t), Some(json!({ "url": "https://fake.test/ok" }))).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["info"]["title"].as_str(), Some("Fake Video"));
    assert_eq!(v["info"]["formats"].as_array().unwrap().len(), 3);
    let (s, _) = e.call("POST", "/api/media/info", Some(EXT), Some(&t), Some(json!({ "url": "https://fake.test/private" }))).await;
    assert_eq!(s, 422);
    let (s, _) = e.call("POST", "/api/media/info", Some(EXT), None, Some(json!({ "url": "https://fake.test/ok" }))).await;
    assert_eq!(s, 401);
    let (s, v) = e.call("POST", "/api/download", Some(EXT), Some(&t), Some(json!({ "url": "https://www.youtube.com/watch?v=x", "formatId": "18", "type": "video_page", "mode": "queue" }))).await;
    assert_eq!(s, 200, "{v}");
    e.mgr.sync().await;
    let job = e.mgr.get(v["job_id"].as_str().unwrap()).unwrap();
    assert_eq!((job.source.as_str(), job.queue_id.as_deref()), ("extension", Some("main")));
    let (s, _) = e.call("POST", "/api/download", Some(EXT), Some(&t), Some(json!({ "url": "https://a.com/v", "formatId": "18;rm -rf" }))).await;
    assert_eq!(s, 400, "format ids are validated");
}
