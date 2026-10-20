//! Locations of per-user application state: %LOCALAPPDATA%\SwiftHarryDM unless
//! SWIFTDM_HOME is set (tests and portable installs).
use std::path::PathBuf;

pub fn app_dir() -> PathBuf {
    let base = match std::env::var_os("SWIFTDM_HOME") {
        Some(p) => PathBuf::from(p),
        None => dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("SwiftHarryDM"),
    };
    let _ = std::fs::create_dir_all(&base);
    base
}

pub fn db_path() -> PathBuf {
    app_dir().join("state3.db")
}
pub fn settings_path() -> PathBuf {
    app_dir().join("settings3.json")
}
pub fn pairings_path() -> PathBuf {
    app_dir().join("pairings.json")
}
pub fn logs_dir() -> PathBuf {
    let d = app_dir().join("logs");
    let _ = std::fs::create_dir_all(&d);
    d
}
pub fn tools_dir() -> PathBuf {
    let d = app_dir().join("tools");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The user's Downloads folder, like IDM (categories add Video, Music, ... below it).
pub fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
        .unwrap_or_else(std::env::temp_dir)
}
