//! License / trial checks with the same rules as v2: same server, token file, 7-day trial
//! and 60-day offline grace. License keys and machine identifiers are never logged.
use std::path::PathBuf;

use chrono::{DateTime, Local, NaiveDateTime};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const LICENSE_SERVER: &str = "https://swift.harshchaudhary.com.np/";
pub const TRIAL_DAYS: i64 = 7;
pub const OFFLINE_GRACE_DAYS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LicenseStatus {
    pub text: String,
    pub allowed: bool,
    /// None | "ask_key" | "hardware_changed" | "machine_mismatch" | "trial_started" | "error"
    pub action: Option<String>,
    pub detail: String,
}

impl LicenseStatus {
    fn new(text: impl Into<String>, allowed: bool, action: Option<&str>, detail: impl Into<String>) -> Self {
        LicenseStatus { text: text.into(), allowed, action: action.map(Into::into), detail: detail.into() }
    }
}

pub fn clean_key(key: &str) -> String {
    key.trim().replace([' ', '-'], "").to_uppercase()
}

pub struct LicenseService {
    pub server: String,
    pub token_file: PathBuf,
    pub machine_id_file: PathBuf,
    pub app_version: String,
    /// Clock (tests).
    pub now: fn() -> NaiveDateTime,
    http: reqwest::Client,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

fn local_now() -> NaiveDateTime {
    Local::now().naive_local()
}

fn parse_time(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .ok()
        .or_else(|| DateTime::parse_from_rfc3339(s).ok().map(|d| d.naive_local()))
}

enum Verify {
    Valid(Option<String>),
    Invalid(String),
}

impl LicenseService {
    pub fn new(app_version: &str) -> Self {
        Self::with(LICENSE_SERVER, home().join(".swift_dm_token.json"), home().join(".swift_dm_machine_id"), app_version)
    }

    pub fn with(server: &str, token_file: PathBuf, machine_id_file: PathBuf, app_version: &str) -> Self {
        LicenseService {
            server: server.trim_end_matches('/').to_string(),
            token_file,
            machine_id_file,
            app_version: app_version.into(),
            now: local_now,
            http: reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap(),
        }
    }

    // ------------------------------------------------------------------ storage
    pub fn machine_id(&self) -> String {
        if let Ok(s) = std::fs::read_to_string(&self.machine_id_file) {
            let s = s.trim().to_string();
            if s.len() == 64 {
                return s;
            }
        }
        let base = machine_guid().map(|g| format!("WIN_GUID_{g}")).unwrap_or_else(|| {
            format!("FALLBACK_{}_{}", std::env::var("COMPUTERNAME").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default())
        });
        let mid = hex::encode(Sha256::digest(base.as_bytes()));
        let _ = std::fs::write(&self.machine_id_file, &mid);
        mid
    }

    pub fn load_token(&self) -> Option<Map<String, Value>> {
        std::fs::read_to_string(&self.token_file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned())
    }

    fn save_token(&self, mut data: Map<String, Value>) {
        data.insert("machine_id".into(), json!(self.machine_id()));
        data.insert("windows_platform".into(), json!(true));
        data.insert("last_updated".into(), json!(self.iso_now()));
        data.insert("app_version".into(), json!(self.app_version));
        let tmp = self.token_file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec(&Value::Object(data)).unwrap()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.token_file);
        }
    }

    fn iso_now(&self) -> String {
        (self.now)().format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    }

    fn days_since(&self, s: Option<&Value>) -> Option<i64> {
        let t = parse_time(s?.as_str()?)?;
        Some(((self.now)() - t).num_days())
    }

    // ------------------------------------------------------------------ server calls
    async fn post(&self, path: &str, payload: Value, timeout_secs: u64) -> Result<Value, String> {
        let r = self
            .http
            .post(format!("{}/{path}", self.server))
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| if e.is_timeout() { "timeout".to_string() } else { "connection_error".to_string() })?;
        if !r.status().is_success() {
            return Err(if r.status().is_server_error() { "server_error".into() } else { "connection_error".into() });
        }
        let body = r.bytes().await.map_err(|_| "connection_error".to_string())?;
        serde_json::from_slice::<Value>(&body).map_err(|_| "connection_error".to_string())
    }

    async fn verify_online(&self, key: &str, machine_id: &str) -> Verify {
        let key = clean_key(key);
        if key.len() != 24 {
            return Verify::Invalid("invalid_format".into());
        }
        match self.post("verify", json!({ "license_key": key, "machine_id": machine_id }), 10).await {
            Ok(d) if d["valid"].as_bool() == Some(true) => Verify::Valid(d["type"].as_str().map(str::to_string)),
            Ok(d) => Verify::Invalid(d["type"].as_str().unwrap_or("invalid").to_string()),
            Err(e) => {
                tracing::warn!("license verification failed: {e}");
                Verify::Invalid(e)
            }
        }
    }

    fn offline_trial(&self, token: Option<&Map<String, Value>>) -> Option<LicenseStatus> {
        let elapsed = self.days_since(token?.get("trial_start"))?;
        (elapsed < TRIAL_DAYS).then(|| LicenseStatus::new(format!("Trial: {} days left (Offline)", TRIAL_DAYS - elapsed), true, None, ""))
    }

    pub async fn start_trial(&self, machine_id: &str) -> LicenseStatus {
        match self.post("trial", json!({ "machine_id": machine_id }), 10).await {
            Err(e) => self
                .offline_trial(self.load_token().as_ref())
                .unwrap_or_else(|| LicenseStatus::new("Status: Check Failed", false, Some("error"), format!("Cannot start trial. ({e})"))),
            Ok(d) if d["valid"].as_bool() == Some(true) => {
                let days = d["days_left"].as_i64().unwrap_or(TRIAL_DAYS);
                let mut t = Map::new();
                t.insert("trial_start".into(), json!(self.iso_now()));
                t.insert("trial_start_server".into(), d["trial_started"].clone());
                t.insert("days_left".into(), json!(days));
                self.save_token(t);
                LicenseStatus::new(format!("Trial: {days} days left"), true, Some("trial_started"), "")
            }
            Ok(_) => LicenseStatus::new("Trial expired", false, Some("ask_key"), "Trial expired. Enter a license key to continue."),
        }
    }

    /// The main check (call it off the UI thread).
    pub async fn check(&self) -> LicenseStatus {
        let Some(mut token) = self.load_token() else {
            let mid = self.machine_id();
            return self.start_trial(&mid).await;
        };
        let mid = self.machine_id();
        if let Some(stored) = token.get("machine_id").and_then(Value::as_str) {
            if stored != mid {
                return LicenseStatus::new("License: Hardware change detected", false, Some("hardware_changed"), "");
            }
        }
        if let Some(key) = token.get("license_key").and_then(Value::as_str).map(str::to_string) {
            return match self.verify_online(&key, &mid).await {
                Verify::Valid(_) => {
                    token.insert("machine_id".into(), json!(mid));
                    token.insert("last_verified".into(), json!(self.iso_now()));
                    token.insert("verification_status".into(), json!("valid"));
                    token.insert("windows_verified".into(), json!(true));
                    self.save_token(token);
                    LicenseStatus::new("License: Active ✓", true, None, "")
                }
                Verify::Invalid(t) if matches!(t.as_str(), "connection_error" | "timeout" | "server_error") => {
                    let days = self.days_since(token.get("last_verified"));
                    match days {
                        Some(d) if d < OFFLINE_GRACE_DAYS => LicenseStatus::new(format!("License: Active (Offline - {}d left)", OFFLINE_GRACE_DAYS - d), true, None, ""),
                        _ => LicenseStatus::new("License: Verification Required", false, Some("ask_key"), ""),
                    }
                }
                Verify::Invalid(t) if t == "machine_mismatch" => LicenseStatus::new("License: Activated on another computer", false, Some("machine_mismatch"), ""),
                Verify::Invalid(_) => LicenseStatus::new("License: Invalid", false, Some("ask_key"), ""),
            };
        }
        if token.contains_key("trial_start") {
            return match self.post("trial", json!({ "machine_id": mid }), 5).await {
                Err(_) => self.offline_trial(Some(&token)).unwrap_or_else(|| LicenseStatus::new("Trial expired", false, Some("ask_key"), "")),
                Ok(d) if d["valid"].as_bool() == Some(true) => LicenseStatus::new(format!("Trial: {} days left", d["days_left"].as_i64().unwrap_or(TRIAL_DAYS)), true, None, ""),
                Ok(_) => LicenseStatus::new("Trial expired", false, Some("ask_key"), ""),
            };
        }
        self.start_trial(&mid).await
    }

    pub async fn activate(&self, key: &str) -> LicenseStatus {
        let key = clean_key(key);
        if key.len() != 24 {
            return LicenseStatus::new("License: Invalid format", false, Some("ask_key"), "License key must be 24 characters long.");
        }
        let mid = self.machine_id();
        match self.verify_online(&key, &mid).await {
            Verify::Valid(ltype) => {
                let mut t = Map::new();
                t.insert("license_key".into(), json!(key));
                t.insert("license_type".into(), json!(ltype));
                t.insert("activated_at".into(), json!(self.iso_now()));
                t.insert("last_verified".into(), json!(self.iso_now()));
                self.save_token(t);
                LicenseStatus::new("License: Active", true, None, "License activated successfully! Lifetime access granted.")
            }
            Verify::Invalid(t) if t == "connection_error" || t == "timeout" => {
                LicenseStatus::new("License: Verification Required", false, Some("error"), "Cannot connect to license server. Please check your internet connection.")
            }
            Verify::Invalid(_) => LicenseStatus::new("License: Invalid", false, Some("ask_key"), "License key is invalid or already used on another machine."),
        }
    }

    /// Forget the stored license so a new key can be entered (hardware change).
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.token_file);
    }

    /// What the License Information window shows (the key is masked).
    pub fn info(&self) -> Value {
        let Some(t) = self.load_token() else { return json!({ "status": "No license or trial on this computer." }) };
        let masked = t.get("license_key").and_then(Value::as_str).map(|k| format!("{}…{}", &k[..4.min(k.len())], &k[k.len().saturating_sub(4)..]));
        json!({
            "license_key": masked,
            "license_type": t.get("license_type"),
            "activated_at": t.get("activated_at"),
            "last_verified": t.get("last_verified"),
            "trial_start": t.get("trial_start"),
        })
    }
}

/// HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid
fn machine_guid() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY};
    let mut buf = [0u16; 128];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Cryptography"),
            w!("MachineGuid"),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
    };
    if r.is_err() {
        return None;
    }
    let n = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n]))
}

// ---------------------------------------------------------------------- app updates

pub const UPDATE_CHECK_URL: &str = "https://swiftharrydm.harshchaudhary.com.np/version.txt";
pub const DOWNLOAD_PAGE_URL: &str = "https://swiftharrydm.harshchaudhary.com.np/downloads";

pub fn parse_version(text: &str) -> Option<Vec<u64>> {
    let t = text.trim().trim_start_matches(['v', 'V']);
    let parts: Vec<&str> = t.split('.').collect();
    if parts.is_empty() || parts.len() > 4 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    parts.iter().map(|p| p.parse().ok()).collect()
}

/// The newer version if the server offers one; Err for an unexpected response.
pub fn newer(current: &str, server_text: &str) -> Result<Option<String>, String> {
    let latest = parse_version(server_text).ok_or("The update server returned an unexpected response.")?;
    let cur = parse_version(current).unwrap_or_else(|| vec![0]);
    let w = latest.len().max(cur.len());
    let pad = |v: &Vec<u64>| {
        let mut v = v.clone();
        v.resize(w, 0);
        v
    };
    Ok((pad(&latest) > pad(&cur)).then(|| server_text.trim().to_string()))
}

pub async fn check_for_update(current: &str, url: &str) -> Result<Option<String>, String> {
    let r = reqwest::Client::new().get(url).timeout(std::time::Duration::from_secs(8)).send().await.map_err(|e| format!("Could not reach the update server ({e})."))?;
    if !r.status().is_success() {
        return Err(format!("The update server answered {}.", r.status()));
    }
    newer(current, &r.text().await.map_err(|e| e.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert_eq!(newer("3.0.0", "3.0.1").unwrap().as_deref(), Some("3.0.1"));
        assert_eq!(newer("3.0.0", "3.0").unwrap(), None);
        assert_eq!(newer("3.0.0", "v3.1").unwrap().as_deref(), Some("v3.1"));
        assert!(newer("3.0.0", "<html>error</html>").is_err());
        assert_eq!(clean_key(" abcd-efgh ijkl "), "ABCDEFGHIJKL");
    }
}
