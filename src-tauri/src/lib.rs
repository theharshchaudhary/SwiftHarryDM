//! SwiftHarryDM desktop shell: hosts the core download manager, forwards its events to the
//! windows, and adds what only a desktop app can do (tray, clipboard, notifications, power).
mod commands;
mod system;
mod windows;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use swiftdm_core::events::Event;
use swiftdm_core::manager::{Manager, ManagerConfig};
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
    /// Last state seen per download, to detect transitions.
    seen: Mutex<HashMap<String, JobState>>,
    exiting: Mutex<bool>,
}

impl AppState {
    pub fn watch(&self, id: &str) {
        self.watch_progress.lock().unwrap().insert(id.to_string());
    }
}

fn start_manager() -> Result<Manager, String> {
    let store = Arc::new(Store::open(swiftdm_core::appdata::db_path()).map_err(|e| e.to_string())?);
    let settings = SettingsStore::open(swiftdm_core::appdata::settings_path());
    let http = Arc::new(swiftdm_core::http::HttpEngine::new(store.clone())) as Arc<dyn swiftdm_core::engine::Engine>;
    let engines = HashMap::from([("http".to_string(), http)]);
    Manager::start(store, engines, settings, ManagerConfig::default()).map_err(|e| e.to_string())
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

fn notify(app: &AppHandle, title: &str, body: &str) {
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
                    if settings.show_complete_dialog && s.queue_id.is_none() {
                        open_logged(&app, "complete", &[("id".into(), s.id.clone())]);
                    }
                    let after = state.after.lock().unwrap().remove(&s.id).unwrap_or_default();
                    if after != AfterAction::None {
                        let what = format!("“{}” has finished downloading.", file_label(s));
                        open_logged(&app, "countdown", &[("action".into(), after.as_str().into()), ("what".into(), what)]);
                    }
                }
                if s.state == JobState::Failed && changed && s.queue_id.is_none() {
                    let why = s.error.as_ref().map(|e| e.message.clone()).unwrap_or_default();
                    notify(&app, "Download failed", &format!("{}\n{why}", file_label(s)));
                }
                if matches!(s.state, JobState::Cancelled) {
                    windows::close(&app, &format!("progress-{}", s.id));
                }
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

/// Quota usage changes with every byte; send it to the status bar about once a second.
async fn pump_quota(app: AppHandle) {
    let mut last = None;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let q = app.state::<AppState>().mgr.quota_status();
        if last != Some(q) {
            last = Some(q);
            let _ = app.emit("core", json!({ "type": "quota", "quota": q }));
        }
    }
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
        let c = swiftdm_core::capture::Candidate { url: &url, file_name: None, size: None, forced: false };
        if swiftdm_core::capture::decide(&settings, &c) == swiftdm_core::capture::Decision::Capture {
            open_logged(&app, "info", &[("url".into(), url), ("source".into(), "clipboard".into())]);
        }
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open SwiftHarryDM", true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "Add download…", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_all", "Stop all downloads", true, None::<&str>)?;
    let sched = MenuItem::with_id(app, "scheduler", "Scheduler…", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &add, &sep, &stop, &sched, &sep2, &exit])?;
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

/// A web address passed on the command line (e.g. by a browser or a shortcut) opens the
/// download dialog.
fn handle_args(app: &AppHandle, args: &[String]) {
    for a in args.iter().skip(1) {
        if let Ok(url) = swiftdm_core::manager::validate_url(a) {
            open_logged(app, "info", &[("url".into(), url), ("source".into(), "ui".into())]);
        }
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
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(|app| {
            let mgr = tauri::async_runtime::block_on(async { start_manager() }).map_err(|e| {
                tracing::error!("cannot start: {e}");
                e
            })?;
            let start_hidden = std::env::args().any(|a| a == "--minimized") || mgr.settings().get().start_minimized;
            app.manage(AppState {
                mgr,
                after: Mutex::new(HashMap::new()),
                watch_progress: Mutex::new(HashSet::new()),
                seen: Mutex::new(HashMap::new()),
                exiting: Mutex::new(false),
            });
            build_tray(app.handle())?;
            let h = app.handle().clone();
            tauri::async_runtime::spawn(pump_events(h.clone()));
            tauri::async_runtime::spawn(pump_quota(h.clone()));
            tauri::async_runtime::spawn(watch_clipboard(h.clone()));
            if let Some(main) = app.get_webview_window("main") {
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
            commands::power_action,
            commands::exit_app,
            commands::protect_password,
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
