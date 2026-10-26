//! Versioned user settings (JSON, atomic writes, validated on load).
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::categories::{self, Category};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    System,
    None,
    Manual,
    /// An automatic configuration (PAC) script at `proxy_pac_url`.
    Pac,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiteLogin {
    /// Host or "*.example.com" pattern.
    pub host: String,
    pub username: String,
    /// DPAPI-protected (hex); never stored in plain text.
    pub password_enc: String,
}

/// Connection cap for matching servers (some servers ban clients that open many connections).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerLimit {
    /// Host or wildcard pattern, e.g. "files.example.com" or "dl*.mirror.net".
    pub host: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub download_dir: String,
    pub use_categories: bool,
    pub categories: Vec<Category>,
    pub ask_save_location: bool,
    pub show_download_info_dialog: bool,
    pub show_progress_window: bool,
    pub show_complete_dialog: bool,
    pub max_concurrent: u32,
    pub max_segments: u32,
    pub collision_policy: String, // rename | overwrite
    pub auto_resume: bool,
    pub retries: u32,
    pub speed_limit: u64, // bytes/s, 0 = unlimited
    pub proxy_mode: ProxyMode,
    pub proxy_url: String,
    /// Addresses that skip the manual proxy ("*.corp.example", "10.*", "<local>").
    pub proxy_bypass: Vec<String>,
    pub proxy_pac_url: String,
    pub user_agent: String,
    pub site_logins: Vec<SiteLogin>,
    pub clipboard_monitor: bool,
    pub capture_extensions: Vec<String>,
    pub capture_min_size_kb: u64,
    pub trust_extension: bool,
    pub minimize_to_tray: bool,
    pub start_with_windows: bool,
    pub start_minimized: bool,
    /// The download starts while "Download File Info" is shown.
    pub start_while_info: bool,
    /// Shutting down after downloads closes programs that would hold it up.
    pub force_close_apps: bool,
    /// Ask before removing downloads from the list.
    pub confirm_delete: bool,
    /// After the same browser download is cancelled twice, offer to stop taking it over.
    pub offer_exception_on_cancel: bool,
    /// The download button the extension shows on video and audio players.
    pub player_button: bool,
    /// Icon only instead of icon and text.
    pub player_button_mini: bool,
    /// Not on players shorter than this (seconds; 0 = any length).
    pub player_button_min_seconds: u32,
    /// Sites where the button never shows (same patterns as the capture lists).
    pub player_button_skip_sites: Vec<String>,
    /// Held while clicking a link in the browser: "alt", "shift", "ctrl", "insert" or "none".
    /// The prevent key leaves the download to the browser; the force key sends the link here.
    pub prevent_key: String,
    pub force_key: String,
    /// Which items the extension adds to the browser's context menu.
    pub menu_download_with: bool,
    pub menu_page_media: bool,
    pub menu_all_links: bool,
    pub menu_selected_links: bool,
    /// Icon in the notification area (View > Notification area icon).
    pub tray_icon: bool,
    /// Font of the windows; empty / 0 = the default (View > Font).
    pub ui_font: String,
    pub ui_font_size: u32,
    /// "Tip of the day" when the main window opens.
    pub tips_on_startup: bool,
    /// The speed limit to use when the limiter is turned on again.
    pub last_speed_limit: u64,
    pub av_scan_enabled: bool,
    pub av_scan_command: String,
    pub sound_on_complete: bool,
    pub theme: String, // system | light | dark
    pub api_port: u16,
    pub server_limits: Vec<ServerLimit>,
    /// Browser downloads from these sites are left to the browser.
    pub capture_skip_sites: Vec<String>,
    /// Browser downloads whose address matches one of these `*` patterns are left to the browser.
    pub capture_skip_urls: Vec<String>,
    /// What to do when the same address is added again: ask | number | overwrite | resume.
    pub duplicate_policy: String,
    /// "Download no more than quota_mb every quota_hours hours" (0 MB = off).
    pub quota_mb: u64,
    pub quota_hours: u32,
}

pub const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
Chrome/130.0 Safari/537.36 Keepfetch/3";

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: 3,
            download_dir: crate::appdata::default_download_dir().to_string_lossy().into_owned(),
            use_categories: true,
            categories: categories::defaults(),
            ask_save_location: false,
            show_download_info_dialog: true,
            show_progress_window: true,
            show_complete_dialog: true,
            max_concurrent: 4,
            max_segments: 8,
            collision_policy: "rename".into(),
            auto_resume: false,
            retries: 3,
            speed_limit: 0,
            proxy_mode: ProxyMode::System,
            proxy_url: String::new(),
            proxy_bypass: vec!["<local>".into()],
            proxy_pac_url: String::new(),
            user_agent: DEFAULT_UA.into(),
            site_logins: vec![],
            clipboard_monitor: true,
            capture_extensions: "3gp 7z aac ace apk arj asf avi bin bz2 deb dmg exe flac flv gz img iso jar m4a m4v \
                mkv mov mp3 mp4 mpeg mpg msi msix ogg pdf rar rpm tar tgz wav webm wma wmv xz zip"
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            capture_min_size_kb: 0,
            trust_extension: false,
            minimize_to_tray: true,
            start_with_windows: true,
            start_minimized: false,
            start_while_info: true,
            force_close_apps: false,
            confirm_delete: true,
            offer_exception_on_cancel: true,
            player_button: true,
            player_button_mini: false,
            player_button_min_seconds: 0,
            player_button_skip_sites: Vec::new(),
            prevent_key: "alt".into(),
            force_key: "insert".into(),
            menu_download_with: true,
            menu_page_media: true,
            menu_all_links: true,
            menu_selected_links: true,
            tray_icon: true,
            ui_font: String::new(),
            ui_font_size: 0,
            tips_on_startup: true,
            last_speed_limit: 500 * 1024,
            av_scan_enabled: false,
            av_scan_command: r#""C:\Program Files\Windows Defender\MpCmdRun.exe" -Scan -ScanType 3 -File "{file}" -DisableRemediation"#
                .into(),
            sound_on_complete: true,
            theme: "system".into(),
            api_port: 5001,
            server_limits: vec![],
            capture_skip_sites: vec![],
            capture_skip_urls: vec![],
            duplicate_policy: "ask".into(),
            quota_mb: 0,
            quota_hours: 24,
        }
    }
}

impl Settings {
    pub fn normalized(mut self) -> Self {
        self.version = 3;
        if self.download_dir.trim().is_empty() {
            self.download_dir = Settings::default().download_dir;
        }
        self.download_dir = self.download_dir.replace('/', "\\");
        self.max_concurrent = self.max_concurrent.clamp(1, 16);
        self.max_segments = self.max_segments.clamp(1, 32);
        self.retries = self.retries.min(20);
        if self.collision_policy != "overwrite" {
            self.collision_policy = "rename".into();
        }
        if !matches!(self.api_port, 5001 | 5002) {
            self.api_port = 5001;
        }
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = "system".into();
        }
        if self.user_agent.trim().is_empty() {
            self.user_agent = DEFAULT_UA.into();
        }
        for e in self.capture_extensions.iter_mut() {
            *e = e.trim().trim_start_matches('.').to_lowercase();
        }
        self.capture_extensions.retain(|e| !e.is_empty());
        for l in self.server_limits.iter_mut() {
            l.host = l.host.trim().to_lowercase();
            l.max_connections = l.max_connections.clamp(1, 32);
        }
        self.server_limits.retain(|l| !l.host.is_empty());
        for list in [&mut self.capture_skip_sites, &mut self.capture_skip_urls] {
            for e in list.iter_mut() {
                *e = e.trim().to_string();
            }
            list.retain(|e| !e.is_empty());
        }
        if !matches!(self.duplicate_policy.as_str(), "ask" | "number" | "overwrite" | "resume") {
            self.duplicate_policy = "ask".into();
        }
        self.quota_hours = self.quota_hours.clamp(1, 24 * 31);
        self
    }

    /// Connections allowed for a download address: the per-server cap if one matches,
    /// otherwise the global maximum.
    pub fn segments_for(&self, url: &str) -> u32 {
        let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        self.server_limits
            .iter()
            .find(|l| crate::wildcard::host_matches(&l.host, &host))
            .map(|l| l.max_connections.min(self.max_segments))
            .unwrap_or(self.max_segments)
            .max(1)
    }

    /// Destination folder for a file name (from `url`, for site-limited categories),
    /// honouring categories.
    pub fn folder_for(&self, file_name: &str, url: Option<&str>) -> (PathBuf, Option<String>) {
        let base = Path::new(&self.download_dir);
        if !self.use_categories {
            return (base.to_path_buf(), None);
        }
        let host = url.and_then(|u| url::Url::parse(u).ok()).and_then(|u| u.host_str().map(str::to_string));
        let cat = categories::categorize(&self.categories, file_name, host.as_deref());
        (categories::folder_for(base, cat), cat.map(|c| c.name.clone()))
    }

    pub fn login_for(&self, host: &str) -> Option<(String, String)> {
        let host = host.to_lowercase();
        self.site_logins.iter().find_map(|l| {
            let pat = l.host.to_lowercase();
            let ok = if let Some(suffix) = pat.strip_prefix("*.") {
                host == suffix || host.ends_with(&format!(".{suffix}"))
            } else {
                host == pat
            };
            ok.then(|| crate::secrets::unprotect_str(&l.password_enc).map(|p| (l.username.clone(), p)))
                .flatten()
        })
    }
}

#[derive(Clone)]
pub struct SettingsStore {
    path: PathBuf,
    inner: Arc<RwLock<Settings>>,
}

impl SettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let s = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
            .unwrap_or_default()
            .normalized();
        SettingsStore { path, inner: Arc::new(RwLock::new(s)) }
    }
    pub fn in_memory(s: Settings) -> Self {
        SettingsStore { path: PathBuf::new(), inner: Arc::new(RwLock::new(s.normalized())) }
    }
    pub fn get(&self) -> Settings {
        self.inner.read().unwrap().clone()
    }
    pub fn set(&self, s: Settings) -> std::io::Result<Settings> {
        let s = s.normalized();
        if !self.path.as_os_str().is_empty() {
            let tmp = self.path.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_vec_pretty(&s).unwrap())?;
            std::fs::rename(&tmp, &self.path)?;
        }
        *self.inner.write().unwrap() = s.clone();
        Ok(s)
    }
    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> std::io::Result<Settings> {
        let mut s = self.get();
        f(&mut s);
        self.set(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_validation_and_corrupt_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.json");
        let st = SettingsStore::open(&p);
        let s = st
            .update(|s| {
                s.max_concurrent = 99;
                s.max_segments = 0;
                s.collision_policy = "bogus".into();
                s.capture_extensions = vec![".ZIP".into(), " ".into()];
            })
            .unwrap();
        assert_eq!((s.max_concurrent, s.max_segments, s.collision_policy.as_str()), (16, 1, "rename"));
        assert_eq!(s.capture_extensions, vec!["zip".to_string()]);
        assert_eq!(SettingsStore::open(&p).get().max_concurrent, 16);
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(SettingsStore::open(&p).get().max_concurrent, Settings::default().max_concurrent);
        std::fs::write(&p, r#"{"max_segments": 4, "evil": 1}"#).unwrap();
        assert_eq!(SettingsStore::open(&p).get().max_segments, 4);
    }

    #[test]
    fn categories_and_logins() {
        let mut s = Settings { download_dir: "C:\\D".into(), ..Settings::default() };
        assert_eq!(s.folder_for("a.zip", None), (PathBuf::from("C:\\D\\Compressed"), Some("Compressed".into())));
        assert_eq!(s.folder_for("a.xyz", None), (PathBuf::from("C:\\D"), None));
        s.site_logins.push(SiteLogin {
            host: "*.example.com".into(),
            username: "u".into(),
            password_enc: crate::secrets::protect_str("p").unwrap(),
        });
        assert_eq!(s.login_for("files.example.com"), Some(("u".into(), "p".into())));
        assert_eq!(s.login_for("evil-example.com"), None);
    }

    #[test]
    fn per_server_connection_limits() {
        let s = Settings {
            max_segments: 8,
            server_limits: vec![
                ServerLimit { host: " Files.Example.com ".into(), max_connections: 2 },
                ServerLimit { host: "dl*.mirror.net".into(), max_connections: 99 },
                ServerLimit { host: "".into(), max_connections: 1 },
            ],
            ..Settings::default()
        }
        .normalized();
        assert_eq!(s.server_limits.len(), 2);
        assert_eq!(s.segments_for("https://files.example.com/a.zip"), 2);
        assert_eq!(s.segments_for("https://eu.files.example.com/a.zip"), 2);
        assert_eq!(s.segments_for("https://dl3.mirror.net/a.zip"), 8); // capped by the global max
        assert_eq!(s.segments_for("https://other.org/a.zip"), 8);
        assert_eq!(s.segments_for("not a url"), 8);
    }
}
