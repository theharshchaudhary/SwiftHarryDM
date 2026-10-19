//! Commands the windows call. Thin wrappers over the core manager; errors become strings
//! the UI shows as they are.
use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::json;
use swiftdm_core::batch::{self, Series};
use swiftdm_core::manager::{QuotaStatus, StartMode, UrlInfo};
use swiftdm_core::models::{JobSnapshot, JobSpec, JobState, ProgressSnapshot};
use swiftdm_core::scheduler::{AfterAction, Queue};
use swiftdm_core::settings::Settings;
use tauri::{AppHandle, Emitter, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::{system, windows, AppState};

type R<T> = Result<T, String>;

#[derive(Serialize)]
pub struct AppSnapshot {
    jobs: Vec<JobSnapshot>,
    progress: Vec<ProgressSnapshot>,
    queues: Vec<Queue>,
    settings: Settings,
    quota: QuotaStatus,
    version: &'static str,
}

#[tauri::command]
pub fn get_state(st: State<AppState>) -> AppSnapshot {
    let jobs = st.mgr.snapshot();
    let progress = jobs.iter().filter(|j| j.state.running()).filter_map(|j| st.mgr.progress(&j.id)).collect();
    AppSnapshot { jobs, progress, queues: st.mgr.queues(), settings: st.mgr.settings().get(), quota: st.mgr.quota_status(), version: swiftdm_core::VERSION }
}

fn mode(m: &str) -> StartMode {
    if m == "later" {
        StartMode::Later
    } else {
        StartMode::Now
    }
}

fn allowed(app: &AppHandle) -> R<()> {
    if crate::upkeep::downloads_allowed(app) {
        Ok(())
    } else {
        Err(crate::upkeep::NOT_ALLOWED.into())
    }
}

#[tauri::command]
pub fn submit(app: AppHandle, st: State<AppState>, spec: JobSpec, mode: String) -> R<String> {
    allowed(&app)?;
    let interactive = spec.source == "ui" || spec.source == "clipboard" || spec.source == "extension";
    let m = self::mode(&mode);
    let id = st.mgr.submit(spec, m)?;
    if interactive && m == StartMode::Now {
        st.watch(&id);
    }
    Ok(id)
}

#[tauri::command]
pub fn submit_many(app: AppHandle, st: State<AppState>, specs: Vec<JobSpec>, mode: String) -> R<Vec<String>> {
    allowed(&app)?;
    let m = self::mode(&mode);
    let mut ids = vec![];
    let mut errors = vec![];
    for s in specs {
        let url = s.url.clone();
        match st.mgr.submit(s, m) {
            Ok(id) => ids.push(id),
            Err(e) => errors.push(format!("{url}: {e}")),
        }
    }
    if ids.is_empty() && !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(ids)
}

#[tauri::command]
pub fn pause(st: State<AppState>, ids: Vec<String>) {
    ids.iter().for_each(|id| st.mgr.pause(id));
}

#[tauri::command]
pub fn resume(app: AppHandle, st: State<AppState>, ids: Vec<String>) -> R<()> {
    allowed(&app)?;
    for id in &ids {
        match st.mgr.get(id).map(|s| s.state) {
            Some(JobState::Failed) => st.mgr.retry(id),
            _ => st.mgr.resume(id),
        }
        if ids.len() == 1 {
            st.watch(id);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn cancel(st: State<AppState>, ids: Vec<String>) {
    ids.iter().for_each(|id| st.mgr.cancel(id));
}

#[tauri::command]
pub fn remove(st: State<AppState>, ids: Vec<String>, delete_files: bool) {
    ids.iter().for_each(|id| st.mgr.remove(id, delete_files));
}

/// Add the same addresses again as new downloads (a fresh copy, numbered if the file exists).
#[tauri::command]
pub fn redownload(app: AppHandle, st: State<AppState>, ids: Vec<String>) -> R<Vec<String>> {
    allowed(&app)?;
    let mut out = vec![];
    for id in ids {
        let Some(s) = st.mgr.get(&id) else { continue };
        let mut spec = JobSpec::new(s.url.clone(), s.dest_dir.clone());
        spec.referer = s.referer.clone();
        spec.category = s.category.clone();
        spec.description = s.description.clone();
        spec.filename = s.file_name.clone();
        spec.source = "ui".into();
        out.push(st.mgr.submit(spec, StartMode::Now)?);
    }
    Ok(out)
}

#[tauri::command]
pub fn pause_all(st: State<AppState>) {
    st.mgr.pause_all();
}

#[tauri::command]
pub fn resume_all(st: State<AppState>) {
    st.mgr.resume_all();
}

#[tauri::command]
pub fn start_queue(st: State<AppState>, id: String) {
    st.mgr.start_queue(&id);
}

#[tauri::command]
pub fn stop_queue(st: State<AppState>, id: String) {
    st.mgr.stop_queue(&id);
}

async fn emit_queues(app: &AppHandle, st: &AppState) {
    st.mgr.sync().await;
    let _ = app.emit("core", json!({ "type": "queues_changed", "queues": st.mgr.queues() }));
}

#[tauri::command]
pub async fn save_queue(app: AppHandle, st: State<'_, AppState>, queue: Queue) -> R<()> {
    if queue.name.trim().is_empty() {
        return Err("Give the queue a name.".into());
    }
    st.mgr.save_queue(queue);
    emit_queues(&app, &st).await;
    Ok(())
}

#[tauri::command]
pub async fn delete_queue(app: AppHandle, st: State<'_, AppState>, id: String) -> R<()> {
    st.mgr.delete_queue(&id);
    emit_queues(&app, &st).await;
    Ok(())
}

#[tauri::command]
pub fn move_to_queue(st: State<AppState>, ids: Vec<String>, queue_id: Option<String>) {
    ids.iter().for_each(|id| st.mgr.move_to_queue(id, queue_id.clone()));
}

#[tauri::command]
pub fn set_description(st: State<AppState>, id: String, description: Option<String>) {
    st.mgr.set_description(&id, description);
}

#[tauri::command]
pub fn set_speed_limit(st: State<AppState>, id: String, limit: Option<u64>) {
    st.mgr.set_speed_limit(&id, limit);
}

#[tauri::command]
pub fn set_priority(st: State<AppState>, id: String, priority: i32) {
    st.mgr.set_priority(&id, priority);
}

#[tauri::command]
pub fn set_job_after(st: State<AppState>, id: String, action: AfterAction) {
    let mut m = st.after.lock().unwrap();
    if action == AfterAction::None {
        m.remove(&id);
    } else {
        m.insert(id, action);
    }
}

#[tauri::command]
pub fn get_job_after(st: State<AppState>, id: String) -> AfterAction {
    st.after.lock().unwrap().get(&id).copied().unwrap_or_default()
}

#[tauri::command]
pub async fn refresh_url(st: State<'_, AppState>, id: String, url: String) -> R<()> {
    st.mgr.refresh_url(&id, &url).await
}

#[tauri::command]
pub async fn inspect_url(st: State<'_, AppState>, url: String) -> R<UrlInfo> {
    st.mgr.inspect_url(&url, None).await
}

#[tauri::command]
pub fn find_duplicates(st: State<AppState>, url: String) -> Vec<JobSnapshot> {
    st.mgr.find_duplicates(&url)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, st: State<AppState>, settings: Settings) -> R<Settings> {
    let before = st.mgr.settings().get();
    let saved = st.mgr.settings().set(settings).map_err(|e| format!("Could not save the settings: {e}"))?;
    if before.start_with_windows != saved.start_with_windows {
        use tauri_plugin_autostart::ManagerExt;
        let r = if saved.start_with_windows { app.autolaunch().enable() } else { app.autolaunch().disable() };
        if let Err(e) = r {
            tracing::warn!("autostart change failed: {e}");
        }
    }
    let _ = app.emit("core", json!({ "type": "settings_changed", "settings": saved }));
    Ok(saved)
}

#[tauri::command]
pub fn expand_batch(pattern: String, series: Series) -> R<Vec<String>> {
    batch::expand(&pattern, &series)
}

#[tauri::command]
pub fn extract_urls(text: String) -> Vec<String> {
    batch::extract_urls(&text)
}

#[tauri::command]
pub fn clipboard_urls(app: AppHandle) -> Vec<String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard().read_text().map(|t| batch::extract_urls(&t)).unwrap_or_default()
}

fn job_path(st: &AppState, id: &str) -> R<String> {
    let s = st.mgr.get(id).ok_or("This download is no longer in the list.")?;
    s.final_path.ok_or_else(|| "The file has not been downloaded yet.".to_string())
}

#[tauri::command]
pub fn open_file(st: State<AppState>, id: String) -> R<()> {
    system::open_path(&job_path(&st, &id)?)
}

#[tauri::command]
pub fn open_with(st: State<AppState>, id: String) -> R<()> {
    system::open_with(&job_path(&st, &id)?)
}

#[tauri::command]
pub fn open_folder(st: State<AppState>, id: String) -> R<()> {
    let s = st.mgr.get(&id).ok_or("This download is no longer in the list.")?;
    match s.final_path {
        Some(p) => system::reveal(&p),
        None => {
            std::fs::create_dir_all(&s.dest_dir).ok();
            system::open_path(&s.dest_dir)
        }
    }
}

#[tauri::command]
pub fn open_path(path: String) -> R<()> {
    system::open_path(&path)
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle, initial: Option<String>) -> Option<String> {
    let mut d = app.dialog().file().set_title("Choose a folder");
    if let Some(i) = initial.filter(|i| std::path::Path::new(i).is_dir()) {
        d = d.set_directory(i);
    }
    d.blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn pick_file(app: AppHandle, initial: Option<String>) -> Option<String> {
    let mut d = app.dialog().file();
    if let Some(i) = initial.filter(|i| std::path::Path::new(i).is_dir()) {
        d = d.set_directory(i);
    }
    d.blocking_pick_file().and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

/// Save download addresses to a text file, one per line. Returns how many were written.
#[tauri::command]
pub async fn export_list(app: AppHandle, st: State<'_, AppState>, ids: Option<Vec<String>>) -> R<usize> {
    let jobs = st.mgr.snapshot();
    let urls: Vec<String> = match ids {
        Some(ids) => ids.iter().filter_map(|id| jobs.iter().find(|j| &j.id == id)).map(|j| j.url.clone()).collect(),
        None => jobs.iter().map(|j| j.url.clone()).collect(),
    };
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Export download list")
        .set_file_name("downloads.txt")
        .add_filter("Text files", &["txt"])
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(0);
    };
    std::fs::write(&path, urls.join("\r\n") + "\r\n").map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok(urls.len())
}

/// Read web addresses from a text (or HTML) file the user picks.
#[tauri::command]
pub async fn import_list(app: AppHandle) -> R<Vec<String>> {
    let Some(path): Option<PathBuf> = app
        .dialog()
        .file()
        .set_title("Import download list")
        .add_filter("Address lists (text, web page, IDM export)", &["txt", "lst", "htm", "html", "csv", "ef2"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(vec![]);
    };
    let bytes = std::fs::read(&path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if bytes.len() > 32 << 20 {
        return Err("The file is too large to import.".into());
    }
    let urls = batch::extract_urls(&String::from_utf8_lossy(&bytes));
    if urls.is_empty() {
        return Err("No web addresses were found in that file.".into());
    }
    Ok(urls)
}

/// Async on purpose: creating a webview from a synchronous command deadlocks WebView2.
#[tauri::command]
pub async fn open_window(app: AppHandle, kind: String, params: HashMap<String, String>) -> R<()> {
    let p: Vec<(String, String)> = params.into_iter().collect();
    windows::open(&app, &kind, &p)
}

#[tauri::command]
pub fn close_self(window: WebviewWindow) {
    let _ = window.close();
}

#[tauri::command]
pub fn power_action(app: AppHandle, action: AfterAction) -> R<()> {
    match action {
        AfterAction::None => Ok(()),
        AfterAction::Exit => {
            crate::exit_gracefully(&app);
            Ok(())
        }
        AfterAction::Sleep => system::suspend(false),
        AfterAction::Hibernate => system::suspend(true),
        AfterAction::Shutdown => system::shutdown(),
    }
}

#[tauri::command]
pub fn exit_app(app: AppHandle) {
    crate::exit_gracefully(&app);
}

#[tauri::command]
pub fn protect_password(password: String) -> R<String> {
    swiftdm_core::secrets::protect_str(&password).ok_or_else(|| "Windows could not encrypt the password.".into())
}

#[derive(Serialize)]
pub struct ApiStatus {
    port: Option<u16>,
    paired: Vec<String>,
    error: Option<String>,
}

#[tauri::command]
pub fn api_status(st: State<AppState>) -> ApiStatus {
    match &*st.api.lock().unwrap() {
        Ok(h) => ApiStatus { port: Some(h.port()), paired: h.paired(), error: None },
        Err(e) => ApiStatus { port: None, paired: vec![], error: Some(e.clone()) },
    }
}

/// Forget one paired browser extension, or all of them.
#[tauri::command]
pub fn revoke_extension(st: State<AppState>, id: Option<String>) {
    if let Ok(h) = &*st.api.lock().unwrap() {
        h.revoke(id.as_deref());
    }
}

#[tauri::command]
pub fn deny_pairing(st: State<AppState>, pairing_id: String) {
    if let Ok(h) = &*st.api.lock().unwrap() {
        h.deny_pairing(&pairing_id);
    }
}

/// Accept a download the extension sent, with the edits made in the file-info window.
#[tauri::command]
pub fn resolve_request(app: AppHandle, st: State<AppState>, rid: String, overrides: swiftdm_core::api::Overrides) -> R<String> {
    allowed(&app)?;
    let h = st.api.lock().unwrap().clone().map_err(|e| format!("The browser connection is not running: {e}"))?;
    h.resolve(&rid, overrides)
}

#[tauri::command]
pub fn reject_request(st: State<AppState>, rid: String) {
    if let Ok(h) = &*st.api.lock().unwrap() {
        h.reject(&rid);
    }
}

#[tauri::command]
pub fn is_media_url(url: String) -> bool {
    swiftdm_core::router::is_media_url(&url)
}

/// Title and formats of a video page, for the quality picker.
#[tauri::command]
pub async fn inspect_media(st: State<'_, AppState>, url: String) -> R<swiftdm_core::media::MediaInfo> {
    let url = swiftdm_core::manager::validate_url(&url)?;
    st.media.inspect(&url).await
}

#[tauri::command]
pub fn close_window(app: AppHandle, label: String) {
    windows::close(&app, &label);
}

#[tauri::command]
pub fn show_main(app: AppHandle) {
    windows::show_main(&app);
}
