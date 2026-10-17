//! Fault-injecting HTTP/1.1 test server (Rust port of the v2 Python test server).
//! Hand-written so it can cut, stall, throttle and mis-answer requests on purpose.
//! Payloads are deterministic so tests can verify SHA-256 without keeping large files.
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const MIB: u64 = 1024 * 1024;
pub const BLOCK: u64 = 64 * 1024;

#[derive(Clone)]
pub enum Body {
    Gen { seed: Vec<u8>, size: u64 },
    Raw(Arc<Vec<u8>>),
}

impl Body {
    pub fn gen(seed: &str, size: u64) -> Self {
        Body::Gen { seed: seed.as_bytes().to_vec(), size }
    }
    pub fn raw(data: Vec<u8>) -> Self {
        Body::Raw(Arc::new(data))
    }
    pub fn size(&self) -> u64 {
        match self {
            Body::Gen { size, .. } => *size,
            Body::Raw(d) => d.len() as u64,
        }
    }
    fn block(seed: &[u8], i: u64) -> Vec<u8> {
        let mut h = Sha256::new();
        h.update(seed);
        h.update(i.to_le_bytes());
        let d = h.finalize();
        let mut b = Vec::with_capacity(BLOCK as usize);
        while (b.len() as u64) < BLOCK {
            b.extend_from_slice(&d);
        }
        if i == 0 {
            // Fixed binary magic so content sniffers never mistake test data for HTML.
            b[..8].copy_from_slice(b"\x00SDMTEST");
        }
        b
    }
    pub fn read(&self, start: u64, len: u64) -> Vec<u8> {
        match self {
            Body::Raw(d) => {
                let s = start.min(d.len() as u64) as usize;
                let e = (start + len).min(d.len() as u64) as usize;
                d[s..e].to_vec()
            }
            Body::Gen { seed, size } => {
                let end = (start + len).min(*size);
                let mut out = Vec::with_capacity((end - start) as usize);
                let mut pos = start;
                let mut cache: Option<(u64, Vec<u8>)> = None;
                while pos < end {
                    let (i, off) = (pos / BLOCK, pos % BLOCK);
                    if cache.as_ref().map(|c| c.0) != Some(i) {
                        cache = Some((i, Self::block(seed, i)));
                    }
                    let take = (BLOCK - off).min(end - pos);
                    out.extend_from_slice(&cache.as_ref().unwrap().1[off as usize..(off + take) as usize]);
                    pos += take;
                }
                out
            }
        }
    }
    pub fn sha256(&self) -> String {
        let mut h = Sha256::new();
        let mut pos = 0;
        while pos < self.size() {
            let c = self.read(pos, 4 * MIB);
            pos += c.len() as u64;
            h.update(&c);
        }
        hex::encode(h.finalize())
    }
    pub fn all(&self) -> Vec<u8> {
        self.read(0, self.size())
    }
}

pub type When = Arc<dyn Fn(u64, Option<&str>) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct Fault {
    pub after_bytes: u64,
    pub stall: Option<Duration>,
    pub when: When,
    /// How many times it fires; negative = forever.
    pub times: i64,
}

impl Fault {
    pub fn cut(after: u64) -> Self {
        Fault { after_bytes: after, stall: None, when: Arc::new(|_, _| true), times: 1 }
    }
    pub fn stall(after: u64, d: Duration) -> Self {
        Fault { after_bytes: after, stall: Some(d), when: Arc::new(|_, _| true), times: 1 }
    }
    pub fn when(mut self, f: impl Fn(u64, Option<&str>) -> bool + Send + Sync + 'static) -> Self {
        self.when = Arc::new(f);
        self
    }
    pub fn times(mut self, n: i64) -> Self {
        self.times = n;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeBug {
    Ignore,
    WrongStart,
    WrongTotal,
}

#[derive(Clone)]
pub struct Resource {
    pub body: Body,
    pub ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: String,
    pub content_disposition: Option<String>,
    pub gzip: bool,
    pub chunked: bool,
    pub statuses: Vec<u16>,
    pub retry_after: Option<String>,
    pub throttle_bps: Option<u64>,
    pub faults: Vec<Fault>,
    pub range_bug: Option<RangeBug>,
    pub replace_after: Option<u64>,
    pub replacement: Option<Box<Resource>>,
    pub redirect_to: Option<String>,
    pub redirect_status: u16,
    pub require_cookie: Option<String>,
    pub require_basic: Option<String>,
}

impl Resource {
    pub fn new(body: Body) -> Self {
        Resource {
            body,
            ranges: true,
            etag: None,
            last_modified: None,
            content_type: "application/octet-stream".into(),
            content_disposition: None,
            gzip: false,
            chunked: false,
            statuses: vec![],
            retry_after: None,
            throttle_bps: None,
            faults: vec![],
            range_bug: None,
            replace_after: None,
            replacement: None,
            redirect_to: None,
            redirect_status: 302,
            require_cookie: None,
            require_basic: None,
        }
    }
    pub fn etag(mut self, e: &str) -> Self {
        self.etag = Some(e.into());
        self
    }
    pub fn last_modified(mut self, l: &str) -> Self {
        self.last_modified = Some(l.into());
        self
    }
    pub fn no_ranges(mut self) -> Self {
        self.ranges = false;
        self
    }
    pub fn throttle(mut self, bps: u64) -> Self {
        self.throttle_bps = Some(bps);
        self
    }
    pub fn fault(mut self, f: Fault) -> Self {
        self.faults.push(f);
        self
    }
    pub fn content_type(mut self, c: &str) -> Self {
        self.content_type = c.into();
        self
    }
    pub fn disposition(mut self, d: &str) -> Self {
        self.content_disposition = Some(d.into());
        self
    }
    pub fn statuses(mut self, s: &[u16]) -> Self {
        self.statuses = s.to_vec();
        self
    }
    pub fn redirect(to: &str, status: u16) -> Self {
        let mut r = Resource::new(Body::raw(vec![]));
        r.redirect_to = Some(to.into());
        r.redirect_status = status;
        r
    }
}

struct Route {
    res: Mutex<Resource>,
    requests: AtomicU64,
    bytes_sent: AtomicU64,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub method: String,
    pub path: String,
    pub range: Option<String>,
    pub if_range: Option<String>,
    pub cookie: Option<String>,
    pub authorization: Option<String>,
}

#[derive(Default)]
struct State {
    routes: Mutex<HashMap<String, Arc<Route>>>,
    log: Mutex<Vec<LogEntry>>,
    total_sent: AtomicU64,
}

pub struct TestServer {
    pub base: String,
    state: Arc<State>,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    pub async fn start() -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(State::default());
        let st = state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { break };
                let st = st.clone();
                tokio::spawn(async move {
                    let _ = handle_conn(sock, st).await;
                });
            }
        });
        TestServer { base, state, task }
    }

    pub fn add(&self, path: &str, res: Resource) -> String {
        self.state.routes.lock().unwrap().insert(
            path.into(),
            Arc::new(Route { res: Mutex::new(res), requests: AtomicU64::new(0), bytes_sent: AtomicU64::new(0) }),
        );
        format!("{}{}", self.base, path)
    }

    pub fn requests(&self, path: &str) -> Vec<LogEntry> {
        self.state.log.lock().unwrap().iter().filter(|e| e.path == path && e.method == "GET").cloned().collect()
    }

    pub fn bytes_sent(&self, path: &str) -> u64 {
        self.state.routes.lock().unwrap().get(path).map(|r| r.bytes_sent.load(Ordering::SeqCst)).unwrap_or(0)
    }

    pub fn total_sent(&self) -> u64 {
        self.state.total_sent.load(Ordering::SeqCst)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_head(sock: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<Option<String>> {
    loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..i]).into_owned();
            buf.drain(..i + 4);
            return Ok(Some(head));
        }
        let mut tmp = [0u8; 8192];
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

async fn simple(sock: &mut TcpStream, code: u16, body: &[u8], extra: &[(&str, String)]) -> std::io::Result<()> {
    let mut h = format!("HTTP/1.1 {code} X\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in extra {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    h.push_str("\r\n");
    sock.write_all(h.as_bytes()).await?;
    sock.write_all(body).await
}

async fn handle_conn(mut sock: TcpStream, st: Arc<State>) -> std::io::Result<()> {
    let mut buf = Vec::new();
    loop {
        let Some(head) = read_head(&mut sock, &mut buf).await? else { return Ok(()) };
        let mut lines = head.split("\r\n");
        let first = lines.next().unwrap_or("");
        let mut parts = first.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let target = parts.next().unwrap_or("/").to_string();
        let path = target.split('?').next().unwrap_or("/").to_string();
        let mut hdr: HashMap<String, String> = HashMap::new();
        for l in lines {
            if let Some((k, v)) = l.split_once(':') {
                hdr.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
        let range = hdr.get("range").cloned();
        let if_range = hdr.get("if-range").cloned();
        st.log.lock().unwrap().push(LogEntry {
            method: method.clone(),
            path: path.clone(),
            range: range.clone(),
            if_range: if_range.clone(),
            cookie: hdr.get("cookie").cloned(),
            authorization: hdr.get("authorization").cloned(),
        });
        let route = st.routes.lock().unwrap().get(&path).cloned();
        let Some(route) = route else {
            simple(&mut sock, 404, b"not found", &[]).await?;
            continue;
        };
        let head_only = method == "HEAD";
        let n = if head_only { route.requests.load(Ordering::SeqCst) } else { route.requests.fetch_add(1, Ordering::SeqCst) + 1 };
        let res = {
            let mut r = route.res.lock().unwrap();
            if let (Some(after), Some(rep)) = (r.replace_after, r.replacement.clone()) {
                if n > after {
                    *r = *rep;
                }
            }
            r.clone()
        };
        if let Some(to) = &res.redirect_to {
            let h = format!("HTTP/1.1 {} X\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n", res.redirect_status);
            sock.write_all(h.as_bytes()).await?;
            continue;
        }
        if let Some(c) = &res.require_cookie {
            if hdr.get("cookie") != Some(c) {
                simple(&mut sock, 403, b"forbidden", &[]).await?;
                continue;
            }
        }
        if let Some(b) = &res.require_basic {
            if hdr.get("authorization") != Some(b) {
                simple(&mut sock, 401, b"auth", &[("WWW-Authenticate", "Basic realm=\"t\"".into())]).await?;
                continue;
            }
        }
        if !head_only && (n as usize) <= res.statuses.len() {
            let code = res.statuses[n as usize - 1];
            let mut extra = vec![];
            if code == 429 {
                if let Some(ra) = &res.retry_after {
                    extra.push(("Retry-After", ra.clone()));
                }
            }
            simple(&mut sock, code, b"error", &extra).await?;
            continue;
        }
        let size = res.body.size();
        let ranges = res.ranges && !res.gzip;
        let (mut start, mut end, mut status) = (0u64, size.saturating_sub(1), 200u16);
        if let (Some(r), true) = (&range, ranges) {
            let valid_if_range = if_range.as_ref().is_none_or(|ir| Some(ir) == res.etag.as_ref() || Some(ir) == res.last_modified.as_ref());
            if let Some(spec) = r.strip_prefix("bytes=") {
                if valid_if_range && res.range_bug != Some(RangeBug::Ignore) {
                    if let Some((a, b)) = spec.split_once('-') {
                        if a.is_empty() {
                            let suffix: u64 = b.parse().unwrap_or(0);
                            start = size.saturating_sub(suffix);
                        } else {
                            start = a.parse().unwrap_or(0);
                            if !b.is_empty() {
                                end = b.parse::<u64>().unwrap_or(end).min(size.saturating_sub(1));
                            }
                        }
                        if start >= size {
                            simple(&mut sock, 416, b"", &[("Content-Range", format!("bytes */{size}"))]).await?;
                            continue;
                        }
                        status = 206;
                    }
                }
            }
        }
        let body_len = if size == 0 { 0 } else { end - start + 1 };
        let mut h = format!("HTTP/1.1 {status} X\r\nContent-Type: {}\r\n", res.content_type);
        if let Some(e) = &res.etag {
            h.push_str(&format!("ETag: {e}\r\n"));
        }
        if let Some(l) = &res.last_modified {
            h.push_str(&format!("Last-Modified: {l}\r\n"));
        }
        if let Some(d) = &res.content_disposition {
            h.push_str(&format!("Content-Disposition: {d}\r\n"));
        }
        if ranges {
            h.push_str("Accept-Ranges: bytes\r\n");
        }
        if status == 206 {
            let cs = if res.range_bug == Some(RangeBug::WrongStart) { start + 1 } else { start };
            let total = if res.range_bug == Some(RangeBug::WrongTotal) { size + 7 } else { size };
            h.push_str(&format!("Content-Range: bytes {cs}-{end}/{total}\r\n"));
        }
        if res.gzip {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(&res.body.all())?;
            let gz = enc.finish()?;
            h.push_str(&format!("Content-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", gz.len()));
            sock.write_all(h.as_bytes()).await?;
            if !head_only {
                sock.write_all(&gz).await?;
            }
            continue;
        }
        if res.chunked {
            h.push_str("Transfer-Encoding: chunked\r\n\r\n");
        } else {
            h.push_str(&format!("Content-Length: {body_len}\r\n\r\n"));
        }
        sock.write_all(h.as_bytes()).await?;
        if head_only {
            continue;
        }
        // pick a fault
        let fault = {
            let mut r = route.res.lock().unwrap();
            let mut chosen = None;
            for f in r.faults.iter_mut() {
                if f.times != 0 && (f.when)(n, range.as_deref()) {
                    if f.times > 0 {
                        f.times -= 1;
                    }
                    chosen = Some(f.clone());
                    break;
                }
            }
            chosen
        };
        let mut sent = 0u64;
        let t0 = Instant::now();
        while sent < body_len {
            let mut take = BLOCK.min(body_len - sent);
            if let Some(f) = &fault {
                if sent + take > f.after_bytes {
                    take = f.after_bytes.saturating_sub(sent);
                }
            }
            if take > 0 {
                let chunk = res.body.read(start + sent, take);
                if res.chunked {
                    sock.write_all(format!("{:X}\r\n", chunk.len()).as_bytes()).await?;
                    sock.write_all(&chunk).await?;
                    sock.write_all(b"\r\n").await?;
                } else {
                    sock.write_all(&chunk).await?;
                }
                sent += chunk.len() as u64;
                route.bytes_sent.fetch_add(chunk.len() as u64, Ordering::SeqCst);
                st.total_sent.fetch_add(chunk.len() as u64, Ordering::SeqCst);
            }
            if let Some(f) = &fault {
                if sent >= f.after_bytes {
                    if let Some(d) = f.stall {
                        tokio::time::sleep(d).await;
                    }
                    sock.flush().await?;
                    let _ = sock.shutdown().await;
                    return Ok(());
                }
            }
            if let Some(bps) = res.throttle_bps {
                let expected = Duration::from_secs_f64(sent as f64 / bps as f64);
                let el = t0.elapsed();
                if expected > el {
                    tokio::time::sleep(expected - el).await;
                }
            }
        }
        if res.chunked {
            sock.write_all(b"0\r\n\r\n").await?;
        }
    }
}
