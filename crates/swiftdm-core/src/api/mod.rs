//! Local API for the browser extension.
//!
//! Security model (unchanged from v2):
//!   * bound to 127.0.0.1 only; Host must be 127.0.0.1/localhost:<port> (DNS-rebinding defence)
//!   * no CORS headers: web pages can neither read responses nor pass a JSON preflight
//!   * Origin must be absent or chrome-extension://<id>; any web Origin gets 403
//!   * every endpoint except /health and pairing needs a bearer token from pairing
//!   * strict request schema, body size limit, rate limits, and in-app confirmation
//!
//! New in v3: /api/capture/check (should a browser download be taken over?), cookies and
//! file details on /api/download, and /api/links (download several links from a page).
pub mod auth;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::capture::{self, Candidate, Decision};
use crate::manager::{validate_url, Manager, StartMode};
use crate::models::{JobSpec, JobState, MediaOptions, MEDIA_PRESETS};
use auth::{extension_id_from_origin, ConfirmResult, PairingRequests, PairingStore, RateLimiter};

pub const PORTS: [u16; 2] = [5001, 5002];
pub const API_VERSION: u32 = 3;
const MAX_BODY: usize = 64 * 1024;
const MAX_LINKS: usize = 5000;

/// A download the extension asked for, after validation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadRequest {
    pub url: String,
    pub title: String,
    pub format: String,
    /// "now" or "queue".
    pub mode: String,
    pub page_url: Option<String>,
    pub file_name: Option<String>,
    pub file_size: Option<u64>,
    /// "video" / "audio" for media found on a page; None for plain files.
    pub media_type: Option<String>,
    /// Exact yt-dlp format chosen in the page's format list.
    pub format_id: Option<String>,
    /// Browser cookies for the download's site. Never shown or logged.
    #[serde(skip)]
    pub cookies: Option<String>,
}

const ALLOWED_FIELDS: [&str; 14] = [
    "url", "title", "format", "mode", "pageUrl", "type", "thumbnail", "duration", "instantDownload", "fileName", "fileSize", "cookies", "referrer", "formatId",
];

fn opt_str(v: &Value, k: &str, max: usize) -> Result<Option<String>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.len() <= max => Ok((!s.is_empty()).then(|| s.clone())),
        Some(_) => Err(format!("{k} must be a string of at most {max} characters")),
    }
}

fn web_url(s: Option<String>) -> Option<String> {
    s.filter(|p| url::Url::parse(p).is_ok_and(|u| matches!(u.scheme(), "http" | "https")))
}

pub fn parse_download_request(v: &Value) -> Result<DownloadRequest, String> {
    let obj = v.as_object().ok_or("Request body must be a JSON object.")?;
    let mut unknown: Vec<&String> = obj.keys().filter(|k| !ALLOWED_FIELDS.contains(&k.as_str())).collect();
    if !unknown.is_empty() {
        unknown.sort();
        return Err(format!("Unknown fields: {}", unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let url = validate_url(v.get("url").and_then(Value::as_str).unwrap_or(""))?;
    let title = opt_str(v, "title", 300)?.unwrap_or_default();
    let format = opt_str(v, "format", 16)?.unwrap_or_else(|| "best".into());
    if !MEDIA_PRESETS.contains(&format.as_str()) {
        return Err("unsupported format".into());
    }
    let instant = v.get("instantDownload").and_then(Value::as_bool).unwrap_or(false);
    let mode = opt_str(v, "mode", 8)?.unwrap_or_else(|| if instant { "now".into() } else { "queue".into() });
    if mode != "queue" && mode != "now" {
        return Err("mode must be 'queue' or 'now'".into());
    }
    let page_url = web_url(opt_str(v, "pageUrl", 8192)?).or(web_url(opt_str(v, "referrer", 8192)?));
    let file_name = opt_str(v, "fileName", 1024)?;
    let file_size = match v.get("fileSize") {
        None | Some(Value::Null) => None,
        Some(n) => Some(n.as_u64().ok_or("fileSize must be a non-negative number")?),
    };
    // The page script reports kinds like "video", "video_page" or "audio_link".
    let media_type = opt_str(v, "type", 32)?.and_then(|t| {
        if t.starts_with("video") {
            Some("video".to_string())
        } else if t.starts_with("audio") {
            Some("audio".to_string())
        } else {
            None
        }
    });
    let format_id = opt_str(v, "formatId", 64)?;
    if format_id.as_deref().is_some_and(|f| !f.chars().all(|c| c.is_ascii_alphanumeric() || "-_+.".contains(c))) {
        return Err("formatId has unexpected characters".into());
    }
    let cookies = opt_str(v, "cookies", 16 * 1024)?;
    Ok(DownloadRequest { url, title, format, mode, page_url, file_name, file_size, media_type, format_id, cookies })
}

/// Edits made in the app's confirmation dialog.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Overrides {
    pub dest_dir: Option<String>,
    pub filename: Option<String>,
    pub category: Option<String>,
    pub description: Option<String>,
    /// "now" or "later".
    pub mode: Option<String>,
    pub queue_id: Option<String>,
}

/// What the app does for the API (all calls must return quickly).
pub trait ApiHooks: Send + Sync {
    /// Show the pairing code; the user can deny it with `ApiHandle::deny_pairing`.
    fn show_pairing(&self, pairing_id: &str, extension_id: &str, code: &str);
    fn close_pairing(&self, pairing_id: &str);
    /// Ask the user about a download; answer with `ApiHandle::resolve` or `reject`.
    fn confirm(&self, request_id: &str, req: &DownloadRequest);
    /// Several links from a page: let the user choose.
    fn links(&self, urls: Vec<String>, page_url: Option<String>);
    /// A download from the extension was added and starts now.
    fn started(&self, job_id: &str);
}

#[derive(Debug, Clone)]
enum Confirmation {
    Pending(Box<DownloadRequest>),
    Rejected,
    Accepted(String),
}

struct Inner {
    mgr: Manager,
    pairings: PairingStore,
    pending: PairingRequests,
    hooks: Arc<dyn ApiHooks>,
    media: Option<Arc<crate::media::MediaEngine>>,
    port: u16,
    confirmations: Mutex<HashMap<String, Confirmation>>,
    lim_download: RateLimiter,
    lim_pair: RateLimiter,
    lim_read: RateLimiter,
}

#[derive(Clone)]
pub struct ApiHandle {
    inner: Arc<Inner>,
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "success": false, "error": msg }))).into_response()
}

impl ApiHandle {
    pub fn port(&self) -> u16 {
        self.inner.port
    }
    pub fn paired(&self) -> Vec<String> {
        self.inner.pairings.paired()
    }
    pub fn revoke(&self, extension_id: Option<&str>) {
        self.inner.pairings.revoke(extension_id);
    }
    pub fn deny_pairing(&self, pairing_id: &str) {
        self.inner.pending.deny(pairing_id);
    }
    /// The request waiting for confirmation (for the dialog).
    pub fn pending_request(&self, request_id: &str) -> Option<DownloadRequest> {
        match self.inner.confirmations.lock().unwrap().get(request_id) {
            Some(Confirmation::Pending(r)) => Some((**r).clone()),
            _ => None,
        }
    }
    /// Accept a confirmed download, with the user's edits.
    pub fn resolve(&self, request_id: &str, o: Overrides) -> Result<String, String> {
        let req = match self.inner.confirmations.lock().unwrap().get(request_id) {
            Some(Confirmation::Pending(r)) => (**r).clone(),
            _ => return Err("This request is no longer waiting.".into()),
        };
        let (spec, mode) = self.inner.spec_for(&req, &o);
        let id = self.inner.mgr.submit(spec, mode)?;
        self.inner.confirmations.lock().unwrap().insert(request_id.into(), Confirmation::Accepted(id.clone()));
        if mode == StartMode::Now {
            self.inner.hooks.started(&id);
        }
        Ok(id)
    }
    pub fn reject(&self, request_id: &str) {
        let mut c = self.inner.confirmations.lock().unwrap();
        if matches!(c.get(request_id), Some(Confirmation::Pending(_))) {
            c.insert(request_id.into(), Confirmation::Rejected);
        }
    }
}

impl Inner {
    fn spec_for(&self, req: &DownloadRequest, o: &Overrides) -> (JobSpec, StartMode) {
        let s = self.mgr.settings().get();
        let guess = req.file_name.clone().unwrap_or_else(|| {
            url::Url::parse(&req.url).ok().and_then(|u| u.path_segments().and_then(|mut p| p.next_back().map(str::to_string))).unwrap_or_default()
        });
        let (folder, category) = s.folder_for(&guess);
        let mut spec = JobSpec::new(req.url.clone(), o.dest_dir.clone().unwrap_or_else(|| folder.to_string_lossy().into_owned()));
        spec.source = "extension".into();
        spec.referer = req.page_url.clone();
        spec.filename = o.filename.clone().or_else(|| req.file_name.clone());
        spec.category = o.category.clone().or(category);
        spec.description = o.description.clone().or_else(|| (!req.title.is_empty()).then(|| req.title.clone()));
        spec.cookies = req.cookies.clone();
        spec.format_label = req.format.clone();
        if req.media_type.is_some() || req.format_id.is_some() || crate::router::is_media_url(&req.url) {
            let mut m = MediaOptions::preset(&req.format);
            m.format_id = req.format_id.clone();
            spec.media = Some(m);
            spec.kind = "media".into();
        }
        let later = o.mode.as_deref().map(|m| m == "later").unwrap_or(req.mode == "queue");
        if later {
            spec.queue_id = o.queue_id.clone();
        }
        (spec, if later { StartMode::Later } else { StartMode::Now })
    }

    fn authed(&self, h: &HeaderMap) -> Option<String> {
        let token = h.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")?.trim().to_string();
        let owner = self.pairings.verify(&token)?;
        let origin = h.get("origin").and_then(|v| v.to_str().ok());
        match extension_id_from_origin(origin) {
            Some(o) if o != owner => None,
            _ => Some(owner),
        }
    }
}

fn origin_ext(h: &HeaderMap) -> Option<String> {
    extension_id_from_origin(h.get("origin").and_then(|v| v.to_str().ok()))
}

async fn guard(State(st): State<Arc<Inner>>, req: Request, next: Next) -> Response {
    let host = req.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
    let ok_host = [format!("127.0.0.1:{}", st.port), format!("localhost:{}", st.port)].iter().any(|h| h == host);
    if !ok_host {
        return err(StatusCode::FORBIDDEN, "forbidden host");
    }
    if req.headers().get("origin").is_some() && origin_ext(req.headers()).is_none() {
        return err(StatusCode::FORBIDDEN, "forbidden origin");
    }
    next.run(req).await
}

fn parse_json(body: &Bytes) -> Option<Value> {
    serde_json::from_slice(body).ok()
}

async fn health(State(st): State<Arc<Inner>>, h: HeaderMap) -> Response {
    if !st.lim_read.allow() {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }
    Json(json!({ "status": "running", "app": "SwiftHarryDM", "version": crate::VERSION, "api": API_VERSION, "paired": st.authed(&h).is_some() })).into_response()
}

async fn pair(State(st): State<Arc<Inner>>, h: HeaderMap) -> Response {
    if !st.lim_pair.allow() {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }
    let Some(ext) = origin_ext(&h) else { return err(StatusCode::FORBIDDEN, "pairing is only available to browser extensions") };
    match st.pending.create(&ext) {
        Ok(p) => {
            st.hooks.show_pairing(&p.id, &ext, &p.code);
            (StatusCode::ACCEPTED, Json(json!({ "success": true, "pairing_id": p.id, "expires_in": 120 }))).into_response()
        }
        Err(_) => err(StatusCode::TOO_MANY_REQUESTS, "too many pairing requests"),
    }
}

async fn pair_confirm(State(st): State<Arc<Inner>>, h: HeaderMap, body: Bytes) -> Response {
    if !st.lim_pair.allow() {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }
    let (Some(ext), Some(v)) = (origin_ext(&h), parse_json(&body)) else { return err(StatusCode::BAD_REQUEST, "bad request") };
    let (Some(pid), Some(code)) = (v.get("pairing_id").and_then(Value::as_str), v.get("code").and_then(Value::as_str)) else {
        return err(StatusCode::BAD_REQUEST, "bad request");
    };
    match st.pending.confirm(pid, &ext, code) {
        ConfirmResult::WrongCode => err(StatusCode::UNAUTHORIZED, "wrong code"),
        ConfirmResult::Ok => {
            let token = st.pairings.issue(&ext);
            st.hooks.close_pairing(pid);
            tracing::info!("extension {ext} paired");
            Json(json!({ "success": true, "token": token })).into_response()
        }
        r => {
            st.hooks.close_pairing(pid);
            let (code, what) = match r {
                ConfirmResult::Expired => (StatusCode::GONE, "expired"),
                ConfirmResult::Denied => (StatusCode::GONE, "denied"),
                _ => (StatusCode::NOT_FOUND, "unknown"),
            };
            err(code, &format!("pairing {what}"))
        }
    }
}

async fn download(State(st): State<Arc<Inner>>, h: HeaderMap, body: Bytes) -> Response {
    if !st.lim_download.allow() {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }
    if st.authed(&h).is_none() {
        return err(StatusCode::UNAUTHORIZED, "not paired: open the extension and pair it with SwiftHarryDM");
    }
    let req = match parse_json(&body).ok_or_else(|| "Request body must be JSON.".to_string()).and_then(|v| parse_download_request(&v)) {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_REQUEST, &e),
    };
    if st.mgr.settings().get().trust_extension {
        let (spec, mode) = st.spec_for(&req, &Overrides::default());
        return match st.mgr.submit(spec, mode) {
            Ok(id) => {
                if mode == StartMode::Now {
                    st.hooks.started(&id);
                }
                Json(json!({ "success": true, "status": if mode == StartMode::Now { "started" } else { "queued" }, "job_id": id })).into_response()
            }
            Err(e) => err(StatusCode::BAD_REQUEST, &e),
        };
    }
    let rid = {
        let mut b = [0u8; 8];
        rand::Rng::fill(&mut rand::thread_rng(), &mut b);
        hex::encode(b)
    };
    st.confirmations.lock().unwrap().insert(rid.clone(), Confirmation::Pending(Box::new(req.clone())));
    st.hooks.confirm(&rid, &req);
    tracing::info!("extension download awaiting confirmation: {}", crate::logging::redact_url(&req.url));
    (StatusCode::ACCEPTED, Json(json!({ "success": true, "status": "pending_confirmation", "request_id": rid }))).into_response()
}

async fn request_status(State(st): State<Arc<Inner>>, h: HeaderMap, Path(rid): Path<String>) -> Response {
    if st.authed(&h).is_none() {
        return err(StatusCode::UNAUTHORIZED, "not paired");
    }
    match st.confirmations.lock().unwrap().get(&rid) {
        None => err(StatusCode::NOT_FOUND, "unknown request"),
        Some(Confirmation::Pending(_)) => Json(json!({ "success": true, "status": "pending" })).into_response(),
        Some(Confirmation::Rejected) => Json(json!({ "success": true, "status": "rejected" })).into_response(),
        Some(Confirmation::Accepted(id)) => Json(json!({ "success": true, "status": "accepted", "job_id": id })).into_response(),
    }
}

async fn capture_check(State(st): State<Arc<Inner>>, h: HeaderMap, body: Bytes) -> Response {
    if !st.lim_read.allow() {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }
    if st.authed(&h).is_none() {
        return err(StatusCode::UNAUTHORIZED, "not paired");
    }
    let Some(v) = parse_json(&body) else { return err(StatusCode::BAD_REQUEST, "bad request") };
    let Some(url) = v.get("url").and_then(Value::as_str).filter(|u| u.len() <= 8192) else { return err(StatusCode::BAD_REQUEST, "url required") };
    let name = v.get("fileName").and_then(Value::as_str).filter(|n| n.len() <= 1024);
    let mime = v.get("mime").and_then(Value::as_str).filter(|m| m.len() <= 256);
    let c = Candidate { url, file_name: name, size: v.get("fileSize").and_then(Value::as_u64), mime, forced: v.get("forced").and_then(Value::as_bool).unwrap_or(false) };
    match capture::decide(&st.mgr.settings().get(), &c) {
        Decision::Capture => Json(json!({ "success": true, "capture": true })).into_response(),
        Decision::Skip(why) => Json(json!({ "success": true, "capture": false, "reason": why })).into_response(),
    }
}

async fn links(State(st): State<Arc<Inner>>, h: HeaderMap, body: Bytes) -> Response {
    if !st.lim_download.allow() {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }
    if st.authed(&h).is_none() {
        return err(StatusCode::UNAUTHORIZED, "not paired");
    }
    let Some(v) = parse_json(&body) else { return err(StatusCode::BAD_REQUEST, "bad request") };
    let Some(list) = v.get("urls").and_then(Value::as_array) else { return err(StatusCode::BAD_REQUEST, "urls must be a list") };
    if list.len() > MAX_LINKS {
        return err(StatusCode::BAD_REQUEST, "too many links");
    }
    let mut urls: Vec<String> = vec![];
    for u in list.iter().filter_map(Value::as_str) {
        if let Ok(u) = validate_url(u) {
            if !urls.contains(&u) {
                urls.push(u);
            }
        }
    }
    if urls.is_empty() {
        return err(StatusCode::BAD_REQUEST, "no downloadable links");
    }
    let page = web_url(v.get("pageUrl").and_then(Value::as_str).map(str::to_string));
    let n = urls.len();
    st.hooks.links(urls, page);
    (StatusCode::ACCEPTED, Json(json!({ "success": true, "count": n }))).into_response()
}

async fn jobs(State(st): State<Arc<Inner>>, h: HeaderMap) -> Response {
    if !st.lim_read.allow() {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }
    if st.authed(&h).is_none() {
        return err(StatusCode::UNAUTHORIZED, "not paired");
    }
    let out: Vec<Value> = st
        .mgr
        .snapshot()
        .iter()
        .map(|s| {
            let pct = match s.total {
                Some(t) if t > 0 => Some(((s.downloaded as f64 / t as f64) * 1000.0).round() / 10.0),
                _ if s.state == JobState::Completed => Some(100.0),
                _ => None,
            };
            json!({ "id": s.id, "title": s.title, "state": s.state.as_str(), "progress": pct, "format": s.format_label, "source": s.source })
        })
        .collect();
    Json(json!({ "success": true, "jobs": out })).into_response()
}

/// Title and real formats of a video page, for the quality list in the page's download panel.
async fn media_info(State(st): State<Arc<Inner>>, h: HeaderMap, body: Bytes) -> Response {
    if !st.lim_download.allow() {
        return err(StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }
    if st.authed(&h).is_none() {
        return err(StatusCode::UNAUTHORIZED, "not paired");
    }
    let Some(engine) = st.media.clone() else { return err(StatusCode::SERVICE_UNAVAILABLE, "video downloads are not available") };
    let v = parse_json(&body).unwrap_or(Value::Null);
    let Some(url) = v.get("url").and_then(Value::as_str).map(str::to_string) else {
        return err(StatusCode::BAD_REQUEST, "url required");
    };
    let Ok(url) = validate_url(&url) else { return err(StatusCode::BAD_REQUEST, "not a web address") };
    // The browser's cookies for the site (YouTube asks visitors without them to prove they
    // are not a bot). Never shown or logged.
    let cookies = match opt_str(&v, "cookies", 16 * 1024) {
        Ok(c) => c,
        Err(e) => return err(StatusCode::BAD_REQUEST, &e),
    };
    match engine.inspect(&url, cookies.as_deref()).await {
        Ok(info) => Json(json!({ "success": true, "info": info })).into_response(),
        Err(e) => err(StatusCode::UNPROCESSABLE_ENTITY, &e),
    }
}

async fn fallback() -> Response {
    err(StatusCode::NOT_FOUND, "not found")
}

/// Bind to the first free port (preferred first) and serve until the runtime stops.
pub async fn start(
    mgr: Manager,
    pairings: PairingStore,
    hooks: Arc<dyn ApiHooks>,
    media: Option<Arc<crate::media::MediaEngine>>,
    preferred: u16,
) -> Result<ApiHandle, String> {
    // Port 0 (tests) binds any free port; otherwise try the preferred one, then the others.
    let mut ports = vec![preferred];
    if preferred != 0 {
        ports.extend(PORTS.iter().copied().filter(|p| *p != preferred));
    }
    let mut bound = None;
    for p in ports {
        if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", p)).await {
            let actual = l.local_addr().map(|a| a.port()).unwrap_or(p);
            bound = Some((l, actual));
            break;
        }
    }
    let (listener, port) = bound.ok_or("Ports 5001 and 5002 are both in use; the browser extension cannot connect.")?;
    let inner = Arc::new(Inner {
        mgr,
        pairings,
        pending: PairingRequests::default(),
        hooks,
        media,
        port,
        confirmations: Mutex::new(HashMap::new()),
        lim_download: RateLimiter::new(60),
        lim_pair: RateLimiter::new(10),
        lim_read: RateLimiter::new(600),
    });
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/pair", post(pair))
        .route("/api/pair/confirm", post(pair_confirm))
        .route("/api/download", post(download))
        .route("/api/requests/{rid}", get(request_status))
        .route("/api/capture/check", post(capture_check))
        .route("/api/links", post(links))
        .route("/api/jobs", get(jobs))
        .route("/api/media/info", post(media_info))
        .fallback(fallback)
        .layer(middleware::from_fn_with_state(inner.clone(), guard))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(inner.clone());
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("extension API stopped: {e}");
        }
    });
    tracing::info!("extension API listening on 127.0.0.1:{port}");
    Ok(ApiHandle { inner })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_schema() {
        let ok = parse_download_request(&json!({ "url": "https://a.com/x.zip", "fileName": "x.zip", "fileSize": 10, "cookies": "a=b", "mode": "now" })).unwrap();
        assert_eq!((ok.mode.as_str(), ok.file_size, ok.cookies.as_deref()), ("now", Some(10), Some("a=b")));
        assert!(parse_download_request(&json!({ "url": "https://a.com/x", "evil": 1 })).unwrap_err().contains("evil"));
        assert!(parse_download_request(&json!({ "url": "javascript:alert(1)" })).is_err());
        assert!(parse_download_request(&json!({ "url": "https://a.com/x", "format": "8k" })).is_err());
        assert!(parse_download_request(&json!({ "url": "https://a.com/x", "mode": "later" })).is_err());
        assert!(parse_download_request(&json!({ "url": "https://a.com/x", "fileSize": -1 })).is_err());
        let r = parse_download_request(&json!({ "url": "https://a.com/x", "pageUrl": "file:///etc/passwd", "instantDownload": true })).unwrap();
        assert_eq!((r.page_url, r.mode.as_str()), (None, "now"));
    }
}
