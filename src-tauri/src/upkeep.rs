//! Keeping the app and its parts current, and the license: one-click signed app updates,
//! on-demand video components, weekly yt-dlp updates, and the license check.
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use swiftdm_core::licensing::{LicenseService, LicenseStatus};
use swiftdm_core::media::tools;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

use crate::AppState;

type R<T> = Result<T, String>;

// ---------------------------------------------------------------------- license

pub fn license() -> LicenseService {
    LicenseService::new(swiftdm_core::VERSION)
}

/// Developers can run debug builds without the license server (tests use this).
fn license_skipped() -> bool {
    cfg!(debug_assertions) && std::env::var_os("SWIFTDM_SKIP_LICENSE").is_some()
}

pub async fn refresh_license(app: &AppHandle) -> LicenseStatus {
    let st = if license_skipped() {
        LicenseStatus { text: "License check skipped (developer build)".into(), allowed: true, action: None, detail: String::new() }
    } else {
        license().check().await
    };
    *app.state::<AppState>().license.lock().unwrap() = Some(st.clone());
    let _ = app.emit("core", json!({ "type": "license", "license": st }));
    st
}

/// Check at start (asking for a key if needed) and twice a day.
pub async fn license_loop(app: AppHandle) {
    let first = refresh_license(&app).await;
    if matches!(first.action.as_deref(), Some("ask_key" | "hardware_changed" | "machine_mismatch")) {
        let _ = crate::windows::open(&app, "license", &[]);
    }
    loop {
        tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
        refresh_license(&app).await;
    }
}

pub fn downloads_allowed(app: &AppHandle) -> bool {
    app.state::<AppState>().license.lock().unwrap().as_ref().map(|l| l.allowed).unwrap_or(true)
}

pub const NOT_ALLOWED: &str = "Downloads are paused until a license or trial is active. Open Help > License to continue.";

#[tauri::command]
pub fn license_status(st: State<AppState>) -> Option<LicenseStatus> {
    st.license.lock().unwrap().clone()
}

#[tauri::command]
pub async fn license_check(app: AppHandle) -> LicenseStatus {
    refresh_license(&app).await
}

#[tauri::command]
pub async fn license_activate(app: AppHandle, key: String) -> LicenseStatus {
    let st = license().activate(&key).await;
    if st.allowed {
        refresh_license(&app).await;
    }
    st
}

/// After a hardware change: forget the stored license so it can be activated again.
#[tauri::command]
pub async fn license_reset(app: AppHandle) -> LicenseStatus {
    license().clear();
    refresh_license(&app).await
}

#[tauri::command]
pub fn license_info() -> serde_json::Value {
    license().info()
}

// ---------------------------------------------------------------------- video components

#[derive(Serialize)]
pub struct Components {
    #[serde(flatten)]
    status: tools::ComponentStatus,
    ytdlp_version: Option<String>,
}

#[tauri::command]
pub async fn components_status() -> Components {
    let status = tools::status();
    let ytdlp_version = match tools::Tools::discover().ytdlp {
        Some(p) => tools::version_of(&p).await,
        None => None,
    };
    Components { status, ytdlp_version }
}

/// One click: download and verify whatever is missing, with progress events.
#[tauri::command]
pub async fn install_components(app: AppHandle) -> R<tools::ComponentStatus> {
    let a = app.clone();
    let progress = move |step: &str, done: u64, total: Option<u64>| {
        let _ = a.emit("core", json!({ "type": "components", "step": step, "done": done, "total": total }));
    };
    let r = tools::install_components(&progress).await;
    app.state::<AppState>().media.set_tools(tools::Tools::discover());
    let _ = app.emit("core", json!({ "type": "components", "step": if r.is_ok() { "done" } else { "failed" }, "done": 0, "total": null }));
    r
}

#[tauri::command]
pub async fn update_ytdlp(app: AppHandle) -> R<String> {
    if tools::Tools::discover().ytdlp.is_none() {
        return Err("The video component is not installed yet.".into());
    }
    let v = tools::update_ytdlp(tools::YTDLP_RELEASE).await?;
    app.state::<AppState>().media.set_tools(tools::Tools::discover());
    Ok(v)
}

/// Weekly, silent and verified.
pub async fn ytdlp_loop(app: AppHandle) {
    tokio::time::sleep(Duration::from_secs(90)).await;
    loop {
        if let Some(v) = tools::auto_update_ytdlp().await {
            app.state::<AppState>().media.set_tools(tools::Tools::discover());
            tracing::info!("video component updated to {v}");
        }
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
    }
}

// ---------------------------------------------------------------------- app updates

#[derive(Serialize, Clone)]
pub struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

async fn find_update(app: &AppHandle) -> R<Option<tauri_plugin_updater::Update>> {
    app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| format!("Could not check for updates: {e}"))
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> R<Option<UpdateInfo>> {
    Ok(find_update(&app).await?.map(|u| UpdateInfo { version: u.version.clone(), notes: u.body.clone() }))
}

/// Download the signed update, install it and restart. Downloads in progress are paused
/// first and resume after the restart.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> R<()> {
    let u = find_update(&app).await?.ok_or("SwiftHarryDM is up to date.")?;
    let a = app.clone();
    let mut got = 0u64;
    let bytes = u
        .download(
            move |chunk, total| {
                got += chunk as u64;
                let _ = a.emit("core", json!({ "type": "update_progress", "done": got, "total": total }));
            },
            || {},
        )
        .await
        .map_err(|e| format!("The update could not be downloaded: {e}"))?;
    app.state::<AppState>().mgr.shutdown(Duration::from_secs(8)).await;
    u.install(bytes).map_err(|e| format!("The update could not be installed: {e}"))?;
    app.restart();
}

/// Daily: tell the windows (and the user, once) when an update is ready.
pub async fn update_loop(app: AppHandle) {
    tokio::time::sleep(Duration::from_secs(30)).await;
    let mut told = None::<String>;
    loop {
        if let Ok(Some(u)) = find_update(&app).await {
            let info = UpdateInfo { version: u.version.clone(), notes: u.body.clone() };
            let _ = app.emit("core", json!({ "type": "update_available", "update": info }));
            *app.state::<AppState>().update.lock().unwrap() = Some(info.clone());
            if told.as_deref() != Some(&info.version) {
                crate::notify(&app, "Update available", &format!("SwiftHarryDM {} is ready. Click “Update now” in the main window.", info.version));
                told = Some(info.version);
            }
        }
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
    }
}

#[tauri::command]
pub fn pending_update(st: State<AppState>) -> Option<UpdateInfo> {
    st.update.lock().unwrap().clone()
}
