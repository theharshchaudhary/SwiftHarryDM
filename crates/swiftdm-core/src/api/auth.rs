//! Extension pairing and request authentication (same protocol as v2).
//!
//! The extension asks to pair; the app shows a one-time 6-digit code; the user types it into
//! the extension; only then is a 256-bit bearer token issued. Only a SHA-256 hash of each
//! token is stored. Codes expire after 2 minutes and allow 5 attempts.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CODE_TTL: Duration = Duration::from_secs(120);
pub const CODE_ATTEMPTS: u32 = 5;

/// `chrome-extension://<32 letters a-p>` -> the extension id.
pub fn extension_id_from_origin(origin: Option<&str>) -> Option<String> {
    let id = origin?.strip_prefix("chrome-extension://")?;
    (id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))).then(|| id.to_string())
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// Constant-time equality for equal-length secrets.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand::thread_rng().fill(&mut b[..]);
    hex::encode(b)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Paired {
    id: String,
    token_sha256: String,
    created: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PairFile {
    extensions: Vec<Paired>,
}

/// Paired extensions, persisted as JSON (compatible with the v2 file).
pub struct PairingStore {
    path: PathBuf,
    data: Mutex<PairFile>,
}

impl PairingStore {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let data = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        PairingStore { path, data: Mutex::new(data) }
    }

    fn save(&self, d: &PairFile) {
        if self.path.as_os_str().is_empty() {
            return;
        }
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(d).unwrap()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }

    /// Latest file contents (another process may have written), else what is in memory.
    fn reload(&self, d: &mut PairFile) {
        if let Some(fresh) = std::fs::read_to_string(&self.path).ok().and_then(|t| serde_json::from_str::<PairFile>(&t).ok()) {
            *d = fresh;
        }
    }

    pub fn issue(&self, extension_id: &str) -> String {
        let token = random_hex(32);
        let mut d = self.data.lock().unwrap();
        self.reload(&mut d);
        d.extensions.retain(|e| e.id != extension_id);
        d.extensions.push(Paired { id: extension_id.into(), token_sha256: sha256_hex(&token), created: crate::now() });
        self.save(&d);
        token
    }

    /// The extension id owning `token`, or None. Tokens issued by another process (the
    /// native messaging host) are picked up by re-reading the file once on a miss.
    pub fn verify(&self, token: &str) -> Option<String> {
        let h = sha256_hex(token);
        let find = |d: &PairFile| {
            let mut owner = None;
            for e in &d.extensions {
                if ct_eq(e.token_sha256.as_bytes(), h.as_bytes()) {
                    owner = Some(e.id.clone());
                }
            }
            owner
        };
        let mut d = self.data.lock().unwrap();
        if let Some(o) = find(&d) {
            return Some(o);
        }
        if self.path.as_os_str().is_empty() {
            return None;
        }
        let fresh: PairFile = std::fs::read_to_string(&self.path).ok().and_then(|t| serde_json::from_str(&t).ok())?;
        *d = fresh;
        find(&d)
    }

    pub fn paired(&self) -> Vec<String> {
        let mut d = self.data.lock().unwrap();
        self.reload(&mut d);
        d.extensions.iter().map(|e| e.id.clone()).collect()
    }

    /// Forget one extension, or all of them (None).
    pub fn revoke(&self, extension_id: Option<&str>) {
        let mut d = self.data.lock().unwrap();
        self.reload(&mut d);
        d.extensions.retain(|e| extension_id.is_some_and(|id| e.id != id));
        self.save(&d);
    }
}

#[derive(Debug, Clone)]
pub struct PendingPairing {
    pub id: String,
    pub extension_id: String,
    pub code: String,
    expires: Instant,
    attempts: u32,
    denied: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfirmResult {
    Ok,
    WrongCode,
    Expired,
    Denied,
    Unknown,
}

#[derive(Default)]
pub struct PairingRequests {
    pending: Mutex<HashMap<String, PendingPairing>>,
}

impl PairingRequests {
    pub fn create(&self, extension_id: &str) -> Result<PendingPairing, &'static str> {
        let mut p = self.pending.lock().unwrap();
        let now = Instant::now();
        p.retain(|_, v| v.expires > now);
        if p.values().filter(|v| v.extension_id == extension_id).count() >= 3 {
            return Err("too many pending pairing requests");
        }
        let code = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));
        let req = PendingPairing {
            id: random_hex(16),
            extension_id: extension_id.into(),
            code,
            expires: now + CODE_TTL,
            attempts: 0,
            denied: false,
        };
        p.insert(req.id.clone(), req.clone());
        Ok(req)
    }

    pub fn deny(&self, pairing_id: &str) {
        if let Some(p) = self.pending.lock().unwrap().get_mut(pairing_id) {
            p.denied = true;
        }
    }

    /// Consumes the request on success, denial or expiry.
    pub fn confirm(&self, pairing_id: &str, extension_id: &str, code: &str) -> ConfirmResult {
        let mut m = self.pending.lock().unwrap();
        let Some(p) = m.get_mut(pairing_id) else { return ConfirmResult::Unknown };
        if p.extension_id != extension_id {
            return ConfirmResult::Unknown;
        }
        if p.denied {
            m.remove(pairing_id);
            return ConfirmResult::Denied;
        }
        if Instant::now() > p.expires || p.attempts >= CODE_ATTEMPTS {
            m.remove(pairing_id);
            return ConfirmResult::Expired;
        }
        p.attempts += 1;
        if !ct_eq(p.code.as_bytes(), code.trim().as_bytes()) {
            return ConfirmResult::WrongCode;
        }
        m.remove(pairing_id);
        ConfirmResult::Ok
    }
}

/// Fixed-window-per-minute request limiter.
pub struct RateLimiter {
    limit: usize,
    window: Duration,
    hits: Mutex<Vec<Instant>>,
}

impl RateLimiter {
    pub fn new(limit: usize) -> Self {
        RateLimiter { limit, window: Duration::from_secs(60), hits: Mutex::new(vec![]) }
    }
    pub fn allow(&self) -> bool {
        let now = Instant::now();
        let mut h = self.hits.lock().unwrap();
        h.retain(|t| now.duration_since(*t) < self.window);
        if h.len() >= self.limit {
            return false;
        }
        h.push(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins() {
        assert_eq!(extension_id_from_origin(Some(&format!("chrome-extension://{}", "a".repeat(32)))), Some("a".repeat(32)));
        assert_eq!(extension_id_from_origin(Some(&format!("chrome-extension://{}", "z".repeat(32)))), None);
        assert_eq!(extension_id_from_origin(Some("https://evil.com")), None);
        assert_eq!(extension_id_from_origin(None), None);
    }
    #[test]
    fn tokens_and_codes() {
        let d = tempfile::tempdir().unwrap();
        let s = PairingStore::open(d.path().join("p.json"));
        let t = s.issue("ext");
        assert_eq!(s.verify(&t).as_deref(), Some("ext"));
        assert!(s.verify("nope").is_none());
        assert_eq!(PairingStore::open(d.path().join("p.json")).verify(&t).as_deref(), Some("ext"));
        // A token issued by another process (the native messaging host) is accepted.
        let other = PairingStore::open(d.path().join("p.json")).issue("ext2");
        assert_eq!(s.verify(&other).as_deref(), Some("ext2"));
        s.revoke(None);
        assert!(s.verify(&t).is_none());

        let r = PairingRequests::default();
        let p = r.create("ext").unwrap();
        assert_eq!(r.confirm(&p.id, "other", &p.code), ConfirmResult::Unknown);
        assert_eq!(r.confirm(&p.id, "ext", "000000x"), ConfirmResult::WrongCode);
        assert_eq!(r.confirm(&p.id, "ext", &p.code), ConfirmResult::Ok);
        assert_eq!(r.confirm(&p.id, "ext", &p.code), ConfirmResult::Unknown);
        let p = r.create("ext").unwrap();
        for _ in 0..CODE_ATTEMPTS {
            assert_eq!(r.confirm(&p.id, "ext", "bad"), ConfirmResult::WrongCode);
        }
        assert_eq!(r.confirm(&p.id, "ext", &p.code), ConfirmResult::Expired);
        let p = r.create("ext").unwrap();
        r.deny(&p.id);
        assert_eq!(r.confirm(&p.id, "ext", &p.code), ConfirmResult::Denied);
        for _ in 0..3 {
            r.create("flood").unwrap();
        }
        assert!(r.create("flood").is_err());
    }
}
