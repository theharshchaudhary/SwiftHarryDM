//! SwiftHarryDM desktop shell: hosts the core download manager, forwards its events to the
//! windows, and adds what only a desktop app can do (tray, clipboard, notifications, power).
mod bridge;
mod browser;
mod upkeep;

pub use browser::native_host;
mod commands;
mod system;
mod windows;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use swiftdm_core::events::Event;
use swiftdm_core::manager::{Manager, ManagerConfig};
use swiftdm_core::media::{MediaEngine, Tools};
use swiftdm_core::models::JobState;
use swiftdm_core::scheduler::AfterAction;
use swiftdm_core::settings::SettingsStore;
use swiftdm_core::store::Store;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager as _, RunEvent, WindowEvent};
use tauri_plugin_notification::NotificationExt;

pub struct AppState {
    pub mgr: Manager,
    /// What to do when a particular download finishes (set in its progress window).
    pub after: Mutex<HashMap<String, AfterAction>>,
    /// Downloads started from the UI whose progress window should open once data flows.
    pub watch_progress: Mutex<HashSet<String>>,
    /// Downloads started while "Download File Info" is still open: no progress or complete
    /// window until the user decides there.
    pub held: Mutex<HashSet<String>>,
    /// How often each browser download was cancelled in "Download File Info" (this session).
    pub cancels: Mutex<HashMap<String, u32>>,
    /// Last state seen per download, to detect transitions.
    seen: Mutex<HashMap<String, JobState>>,
    exiting: Mutex<bool>,
    /// The browser-extension API, or why it could not start.
    pub api: Mutex<Result<swiftdm_core::api::ApiHandle, String>>,
    pub media: Arc<MediaEngine>,
    pub update: Mutex<Option<upkeep::UpdateInfo>>,
    pub grabber: Arc<swiftdm_core::grabber::crawl::Grabber>,
}

impl AppState {
    pub fn watch(&self, id: &str) {
        self.watch_progress.lock().unwrap().insert(id.to_string());
    }
}

fn start_manager(media: Arc<MediaEngine>) -> Result<(Manager, Arc<Store>), String> {
    let store = Arc::new(Store::open(swiftdm_core::appdata::db_path()).map_err(|e| e.to_string())?);
    let settings = SettingsStore::open(swiftdm_core::appdata::settings_path());
    let http = Arc::new(swiftdm_core::http::HttpEngine::new(store.clone())) as Arc<dyn swiftdm_core::engine::Engine>;
    let engines = HashMap::from([("http".to_string(), http), ("media".to_string(), media as Arc<dyn swiftdm_core::engine::Engine>)]);
    let mgr = Manager::start(store.clone(), engines, settings, ManagerConfig::default()).map_err(|e| e.to_string())?;
    Ok((mgr, store))
}

/// Site grabber progress for its window.
async fn pump_grabber(app: AppHandle) {
    let mut rx = app.state::<AppState>().grabber.subscribe();
    loop {
        match rx.recv().await {
            Ok(ev) => {
                let _ = app.emit("grabber", &ev);
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
}

/// Stop downloads cleanly (their progress is saved), then quit.
pub fn exit_gracefully(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut e = state.exiting.lock().unwrap();
        if *e {
            return;
        }
        *e = true;
    }
    let mgr = state.mgr.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        mgr.shutdown(Duration::from_secs(8)).await;
        app.exit(0);
    });
}

fn open_logged(app: &AppHandle, kind: &str, params: &[(String, String)]) {
    if let Err(e) = windows::open(app, kind, params) {
        tracing::error!("could not open the {kind} window: {e}");
    }
}

pub(crate) fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn file_label(s: &swiftdm_core::models::JobSnapshot) -> String {
    s.file_name.clone().unwrap_or_else(|| s.title.clone())
}

/// React to core events (open windows, sounds, power actions) and forward them to the UI.
async fn pump_events(app: AppHandle) {
    let state = app.state::<AppState>();
    let mut rx = state.mgr.subscribe();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => break,
        };
        let settings = state.mgr.settings().get();
        match &ev {
            Event::JobChanged { snapshot: s } => {
                let prev = state.seen.lock().unwrap().insert(s.id.clone(), s.state);
                if s.state == JobState::Downloading && prev != Some(JobState::Downloading) && state.watch_progress.lock().unwrap().remove(&s.id) {
                    if settings.show_progress_window {
                        open_logged(&app, "progress", &[("id".into(), s.id.clone())]);
                    }
                }
                let changed = prev.is_some() && prev != Some(s.state);
                if s.state == JobState::Completed && changed {
                    windows::close(&app, &format!("progress-{}", s.id));
                    if settings.sound_on_complete {
                        system::beep();
                    }
                    if settings.show_complete_dialog && s.queue_id.is_none() && !state.held.lock().unwrap().contains(&s.id) {
                        open_logged(&app, "complete", &[("id".into(), s.id.clone())]);
                    }
                    let after = state.after.lock().unwrap().remove(&s.id).unwrap_or_default();
                    if after != AfterAction::None {
                        let what = format!("“{}” has finished downloading.", file_label(s));
                        open_logged(&app, "countdown", &[("action".into(), after.as_str().into()), ("what".into(), what)]);
                    }
                }
                // The server wants a login: ask for it, like IDM's "Connecting to the site".
                if s.state == JobState::Failed && changed && s.error.as_ref().is_some_and(|e| e.http_status == Some(401)) {
                    let realm = s.error.as_ref().map(|e| e.detail.clone()).unwrap_or_default();
                    open_logged(&app, "login", &[("id".into(), s.id.clone()), ("realm".into(), realm)]);
                } else if s.state == JobState::Failed && changed && s.queue_id.is_none() {
                    let why = s.error.as_ref().map(|e| e.message.clone()).unwrap_or_default();
                    notify(&app, "Download failed", &format!("{}\n{why}", file_label(s)));
                }
                if matches!(s.state, JobState::Cancelled) {
                    windows::close(&app, &format!("progress-{}", s.id));
                }
            }
            // A virus scan that found something must be seen, not only logged.
            Event::JobLog { job_id, level, message } if level == "warning" && message.starts_with("Antivirus") => {
                let name = state.mgr.get(job_id).map(|s| file_label(&s)).unwrap_or_default();
                notify(&app, "Virus check", &format!("{name}
{message}"));
            }
            Event::JobRemoved { job_id } => {
                state.seen.lock().unwrap().remove(job_id);
                windows::close(&app, &format!("progress-{job_id}"));
            }
            Event::QueueChanged { .. } => {
                let _ = app.emit("core", json!({ "type": "queues_changed", "queues": state.mgr.queues() }));
            }
            Event::QueueFinished { queue_id, action } => {
                let name = state.mgr.queues().into_iter().find(|q| &q.id == queue_id).map(|q| q.name).unwrap_or_else(|| queue_id.clone());
                notify(&app, "Queue finished", &format!("All downloads in “{name}” are done."));
                if action != "none" {
                    let what = format!("The queue “{name}” has finished.");
                    open_logged(&app, "countdown", &[("action".into(), action.clone()), ("what".into(), what)]);
                }
            }
            Event::QuotaExceeded { used, limit, resume_in_secs } => {
                let mins = (resume_in_secs / 60.0).ceil() as u64;
                notify(
                    &app,
                    "Download quota reached",
                    &format!("{} MB of {} MB used. Downloads continue in about {mins} min.", used / (1 << 20), limit / (1 << 20)),
                );
            }
            _ => {}
        }
        let _ = app.emit("core", &ev);
    }
}

/// Once a second: quota usage for the status bar, overall progress on the taskbar button,
/// and the tray tooltip.
async fn pump_status(app: AppHandle) {
    use tauri::window::{ProgressBarState, ProgressBarStatus};
    let mut last_quota = None;
    let mut last_tip = String::new();
    let mut had_progress = false;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let state = app.state::<AppState>();
        let q = state.mgr.quota_status();
        if last_quota != Some(q) {
            last_quota = Some(q);
            let _ = app.emit("core", json!({ "type": "quota", "quota": q }));
        }
        let (mut done, mut total, mut speed, mut active) = (0u64, 0u64, 0f64, 0usize);
        for j in state.mgr.snapshot().iter().filter(|j| j.state.active()) {
            active += 1;
            if let Some(p) = state.mgr.progress(&j.id) {
                speed += p.speed.unwrap_or(0.0);
                if let Some(t) = p.total {
                    done += p.downloaded;
                    total += t;
                }
            }
        }
        if let Some(main) = app.get_webview_window("main") {
            if active > 0 && total > 0 {
                let pct = (done as f64 / total as f64 * 100.0).round() as u64;
                let _ = main.set_progress_bar(ProgressBarState { status: Some(ProgressBarStatus::Normal), progress: Some(pct.min(100)) });
                had_progress = true;
            } else if had_progress {
                let _ = main.set_progress_bar(ProgressBarState { status: Some(ProgressBarStatus::None), progress: None });
                had_progress = false;
            }
        }
        let tip = match active {
            0 => "SwiftHarryDM".to_string(),
            n => format!("SwiftHarryDM - {n} downloading, {}/s", human(speed as u64)),
        };
        if tip != last_tip {
            if let Some(t) = app.tray_by_id("main") {
                let _ = t.set_tooltip(Some(&tip));
            }
            last_tip = tip;
        }
        save_main_window(&app);
    }
}

fn human(n: u64) -> String {
    let n = n as f64;
    if n >= 1024.0 * 1024.0 {
        format!("{:.1} MB", n / 1024.0 / 1024.0)
    } else {
        format!("{:.0} KB", n / 1024.0)
    }
}

// ---------------------------------------------------------------------- main window memory

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Clone, Copy)]
struct Placement {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    maximized: bool,
}

fn placement_file() -> std::path::PathBuf {
    swiftdm_core::appdata::app_dir().join("window.json")
}

static LAST_PLACEMENT: Mutex<Option<Placement>> = Mutex::new(None);

/// Remember where the main window is (only when it changed; maximized keeps the old size).
fn save_main_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if !w.is_visible().unwrap_or(false) || w.is_minimized().unwrap_or(false) {
        return;
    }
    let maximized = w.is_maximized().unwrap_or(false);
    let mut last = LAST_PLACEMENT.lock().unwrap();
    let p = if maximized {
        match *last {
            Some(prev) => Placement { maximized: true, ..prev },
            None => return,
        }
    } else {
        let (Ok(pos), Ok(size)) = (w.outer_position(), w.inner_size()) else { return };
        Placement { x: pos.x, y: pos.y, w: size.width, h: size.height, maximized: false }
    };
    if *last != Some(p) {
        *last = Some(p);
        let _ = std::fs::write(placement_file(), serde_json::to_vec(&p).unwrap());
    }
}

/// Put the main window back where it was, if that place is still on a screen.
fn restore_main_window(w: &tauri::WebviewWindow) {
    let Some(p) = std::fs::read(placement_file()).ok().and_then(|b| serde_json::from_slice::<Placement>(&b).ok()) else { return };
    let on_screen = w.available_monitors().unwrap_or_default().iter().any(|m| {
        let (mp, ms) = (m.position(), m.size());
        p.x + 50 >= mp.x && p.y >= mp.y - 10 && p.x + 50 < mp.x + ms.width as i32 && p.y + 30 < mp.y + ms.height as i32
    });
    if !on_screen || p.w < 400 || p.h < 300 {
        return;
    }
    let _ = w.set_size(tauri::PhysicalSize::new(p.w, p.h));
    let _ = w.set_position(tauri::PhysicalPosition::new(p.x, p.y));
    if p.maximized {
        let _ = w.maximize();
    }
    *LAST_PLACEMENT.lock().unwrap() = Some(p);
}

/// Offer to download a link when one is copied, if its file type is one we take over.
async fn watch_clipboard(app: AppHandle) {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let mut last: Option<String> = app.clipboard().read_text().ok();
    loop {
        tokio::time::sleep(Duration::from_millis(900)).await;
        let Ok(text) = app.clipboard().read_text() else { continue };
        if last.as_deref() == Some(text.as_str()) {
            continue;
        }
        last = Some(text.clone());
        let settings = app.state::<AppState>().mgr.settings().get();
        if !settings.clipboard_monitor {
            continue;
        }
        let t = text.trim();
        if t.len() > 4096 || t.contains(char::is_whitespace) {
            continue;
        }
        let Ok(url) = swiftdm_core::manager::validate_url(t) else { continue };
        let c = swiftdm_core::capture::Candidate { url: &url, file_name: None, size: None, mime: None, forced: false };
        if swiftdm_core::capture::decide(&settings, &c) == swiftdm_core::capture::Decision::Capture {
            open_logged(&app, "info", &[("url".into(), url), ("source".into(), "clipboard".into())]);
        }
    }
}

fn mgr_settings_tray(app: &AppHandle) -> bool {
    app.state::<AppState>().mgr.settings().get().tray_icon
}

/// The notification-area menu, like IDM's: the main window, new downloads, the drop target,
/// queue and speed limiter, and exit.
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open SwiftHarryDM", true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "Add download…", true, None::<&str>)?;
    let batch = MenuItem::with_id(app, "batch", "Add batch download…", true, None::<&str>)?;
    let drop = MenuItem::with_id(app, "drop", "Show drop target", true, None::<&str>)?;
    let start_q = MenuItem::with_id(app, "start_queue", "Start main queue", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_all", "Stop all downloads", true, None::<&str>)?;
    let lim_on = MenuItem::with_id(app, "limiter_on", "Turn on", true, None::<&str>)?;
    let lim_off = MenuItem::with_id(app, "limiter_off", "Turn off", true, None::<&str>)?;
    let limiter = tauri::menu::Submenu::with_items(app, "Speed limiter", true, &[&lim_on, &lim_off])?;
    let sched = MenuItem::with_id(app, "scheduler", "Scheduler…", true, None::<&str>)?;
    let options = MenuItem::with_id(app, "options", "Options…", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit", true, None::<&str>)?;
    let (s1, s2, s3) = (PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?);
    let menu = Menu::with_items(app, &[&open, &add, &batch, &drop, &s1, &start_q, &stop, &limiter, &s2, &sched, &options, &s3, &exit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("SwiftHarryDM")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => windows::show_main(app),
            "add" => {
                open_logged(app, "add", &[]);
            }
            "stop_all" => app.state::<AppState>().mgr.pause_all(),
            "batch" => {
                open_logged(app, "batch", &[]);
            }
            "drop" => {
                open_logged(app, "drop", &[]);
            }
            "options" => {
                open_logged(app, "options", &[]);
            }
            "start_queue" => app.state::<AppState>().mgr.start_queue(swiftdm_core::scheduler::MAIN_QUEUE),
            "limiter_on" | "limiter_off" => {
                // On: the last limit (or 500 KB/s), off: no limit; like the main window's menu.
                let st = app.state::<AppState>();
                let on = e.id.as_ref() == "limiter_on";
                if let Ok(saved) = st.mgr.settings().update(|s| {
                    if on && s.speed_limit == 0 {
                        s.speed_limit = s.last_speed_limit.max(64 * 1024);
                    } else if !on && s.speed_limit > 0 {
                        s.last_speed_limit = s.speed_limit;
                        s.speed_limit = 0;
                    }
                }) {
                    let _ = app.emit("core", json!({ "type": "settings_changed", "settings": saved }));
                }
            }
            "scheduler" => {
                open_logged(app, "scheduler", &[]);
            }
            "exit" => exit_gracefully(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                windows::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Command line, for scripts and other programs:
///   swiftharrydm.exe <url>                     open the download window for <url>
///   swiftharrydm.exe /d <url> [/p <folder>] [/f <file name>] [/n] [/a]
///     /n  start without asking (no window)      /a  add to the main queue instead of starting
///     /q  exit SwiftHarryDM when that download finishes
///   swiftharrydm.exe /s                        start the main queue
fn handle_args(app: &AppHandle, args: &[String]) {
    let flag = |f: &str| args.iter().any(|a| a.eq_ignore_ascii_case(f) || a.eq_ignore_ascii_case(&f.replace('/', "-")));
    let value = |f: &str| {
        args.windows(2).find(|w| w[0].eq_ignore_ascii_case(f) || w[0].eq_ignore_ascii_case(&f.replace('/', "-"))).map(|w| w[1].clone())
    };
    if flag("/s") {
        app.state::<AppState>().mgr.start_queue(swiftdm_core::scheduler::MAIN_QUEUE);
    }
    let url = value("/d").or_else(|| args.iter().skip(1).find(|a| swiftdm_core::manager::validate_url(a).is_ok()).cloned());
    let Some(url) = url.and_then(|u| swiftdm_core::manager::validate_url(&u).ok()) else { return };
    let quiet = flag("/n");
    let later = flag("/a");
    if !quiet && !later {
        open_logged(app, "info", &[("url".into(), url), ("source".into(), "ui".into())]);
        return;
    }
    let state = app.state::<AppState>();
    let s = state.mgr.settings().get();
    let mut spec = swiftdm_core::models::JobSpec::new(url.clone(), value("/p").unwrap_or_else(|| {
        let name = value("/f").unwrap_or_else(|| url.rsplit('/').next().unwrap_or("").to_string());
        s.folder_for(&name, Some(&url)).0.to_string_lossy().into_owned()
    }));
    spec.filename = value("/f");
    spec.source = "ui".into();
    let mode = if later { swiftdm_core::manager::StartMode::Later } else { swiftdm_core::manager::StartMode::Now };
    // /n asks nothing, so "ask" for a duplicate link means a numbered name.
    match state.mgr.submit_or_reuse(spec, mode, &s.duplicate_policy) {
        Ok((id, _)) if flag("/q") => {
            state.after.lock().unwrap().insert(id, AfterAction::Exit);
        }
        Ok(_) => {}
        Err(e) => notify(app, "Download not started", &e),
    }
}

pub fn run() {
    swiftdm_core::logging::init(&swiftdm_core::appdata::logs_dir());
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            windows::show_main(app);
            handle_args(app, &args);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Drag a finished file out of the "Download complete" window, like IDM.
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        // The Run entry's name must match the uninstaller's cleanup (windows/hooks.nsh).
        .plugin(tauri_plugin_autostart::Builder::new().arg("--minimized").app_name("SwiftHarryDM").build())
        .setup(|app| {
            let tools = Tools::discover();
            tracing::info!("media tools: yt-dlp={:?} ffmpeg={:?} ffprobe={:?}", tools.ytdlp, tools.ffmpeg, tools.ffprobe);
            let media = Arc::new(MediaEngine::new(tools));
            let m2 = media.clone();
            let (mgr, store) = tauri::async_runtime::block_on(async move { start_manager(m2) }).map_err(|e| {
                tracing::error!("cannot start: {e}");
                e
            })?;
            let start_hidden = std::env::args().any(|a| a == "--minimized") || mgr.settings().get().start_minimized;
            let grabber = swiftdm_core::grabber::crawl::Grabber::new(store, mgr.clone());
            app.manage(AppState {
                mgr,
                after: Mutex::new(HashMap::new()),
                watch_progress: Mutex::new(HashSet::new()),
                held: Mutex::new(HashSet::new()),
                cancels: Mutex::new(HashMap::new()),
                seen: Mutex::new(HashMap::new()),
                exiting: Mutex::new(false),
                api: Mutex::new(Err("starting".into())),
                media,
                update: Mutex::new(None),
                grabber,
            });
            // The main window is described in tauri.conf.json but created here, so that it gets
            // the same browser arguments as every other window.
            if let Some(conf) = app.config().app.windows.iter().find(|w| w.label == "main") {
                let mut b = tauri::WebviewWindowBuilder::from_config(app.handle(), conf)?;
                if let Some(args) = windows::browser_args() {
                    b = b.additional_browser_args(&args);
                }
                b.build()?;
            }
            build_tray(app.handle())?;
            if !mgr_settings_tray(app.handle()) {
                if let Some(t) = app.tray_by_id("main") {
                    let _ = t.set_visible(false);
                }
            }
            browser::register();
            let h = app.handle().clone();
            tauri::async_runtime::spawn(pump_events(h.clone()));
            tauri::async_runtime::spawn(pump_status(h.clone()));
            tauri::async_runtime::spawn(pump_grabber(h.clone()));
            tauri::async_runtime::spawn(watch_clipboard(h.clone()));
            tauri::async_runtime::spawn(bridge::start(h.clone()));
            tauri::async_runtime::spawn(upkeep::ytdlp_loop(h.clone()));
            if !cfg!(debug_assertions) {
                tauri::async_runtime::spawn(upkeep::update_loop(h.clone()));
            }
            if let Some(main) = app.get_webview_window("main") {
                restore_main_window(&main);
                if !start_hidden {
                    let _ = main.show();
                }
            }
            handle_args(&h, &std::env::args().collect::<Vec<_>>());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle();
                save_main_window(app);
                if app.state::<AppState>().mgr.settings().get().minimize_to_tray {
                    let _ = window.hide();
                } else {
                    exit_gracefully(app);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::submit,
            commands::submit_many,
            commands::pause,
            commands::resume,
            commands::cancel,
            commands::remove,
            commands::redownload,
            commands::pause_all,
            commands::resume_all,
            commands::start_queue,
            commands::stop_queue,
            commands::save_queue,
            commands::delete_queue,
            commands::move_to_queue,
            commands::set_description,
            commands::relocate,
            commands::hold,
            commands::retry,
            commands::zip_preview,
            commands::count_cancel,
            commands::release,
            commands::set_speed_limit,
            commands::set_priority,
            commands::set_job_after,
            commands::get_job_after,
            commands::refresh_url,
            commands::inspect_url,
            commands::find_duplicates,
            commands::save_settings,
            commands::expand_batch,
            commands::extract_urls,
            commands::clipboard_urls,
            commands::open_file,
            commands::open_with,
            commands::open_folder,
            commands::open_path,
            commands::pick_folder,
            commands::pick_file,
            commands::export_list,
            commands::import_list,
            commands::open_window,
            commands::close_self,
            commands::close_window,
            commands::show_main,
            commands::power_action,
            commands::exit_app,
            commands::protect_password,
            commands::api_status,
            browser::extension_setup,
            upkeep::components_status,
            upkeep::install_components,
            upkeep::update_ytdlp,
            upkeep::check_update,
            upkeep::install_update,
            upkeep::pending_update,
            commands::is_media_url,
            commands::is_elevated,
            commands::grabber_projects,
            commands::grabber_save,
            commands::grabber_delete,
            commands::grabber_state,
            commands::grabber_explore,
            commands::grabber_stop,
            commands::grabber_check,
            commands::grabber_download,
            commands::inspect_media,
            commands::revoke_extension,
            commands::deny_pairing,
            commands::resolve_request,
            commands::reject_request,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the app")
        .run(|app, event| {
            // Closing the last window must not end the process while it lives in the tray.
            if let RunEvent::ExitRequested { api, code, .. } = &event {
                if code.is_none() && !*app.state::<AppState>().exiting.lock().unwrap() {
                    api.prevent_exit();
                }
            }
        });
}
