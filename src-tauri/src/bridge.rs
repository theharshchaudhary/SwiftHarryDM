//! The browser-extension API inside the app: its requests become windows.
use std::sync::Arc;

use serde_json::json;
use swiftdm_core::api::{self, ApiHooks, DownloadRequest};
use swiftdm_core::api::auth::PairingStore;
use tauri::{AppHandle, Emitter, Manager};

use crate::{windows, AppState};

struct Hooks {
    app: AppHandle,
}

impl Hooks {
    fn open(&self, kind: &str, params: Vec<(String, String)>) {
        // Window creation must not run on the API's request path; hand it to the runtime.
        let app = self.app.clone();
        let kind = kind.to_string();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = windows::open(&app, &kind, &params) {
                tracing::error!("could not open the {kind} window: {e}");
            }
        });
    }
}

impl ApiHooks for Hooks {
    fn show_pairing(&self, pairing_id: &str, extension_id: &str, code: &str) {
        windows::show_main(&self.app);
        self.open("pairing", vec![("pid".into(), pairing_id.into()), ("ext".into(), extension_id.into()), ("code".into(), code.into())]);
    }

    fn close_pairing(&self, pairing_id: &str) {
        let label: String = format!("pairing-{}", pairing_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>());
        windows::close(&self.app, &label);
        let _ = self.app.emit("core", json!({ "type": "extension_changed" }));
    }

    fn confirm(&self, request_id: &str, req: &DownloadRequest) {
        let mut p = vec![("url".to_string(), req.url.clone()), ("rid".into(), request_id.into()), ("source".into(), "extension".into())];
        if let Some(r) = &req.page_url {
            p.push(("referer".into(), r.clone()));
        }
        if let Some(n) = &req.file_name {
            p.push(("filename".into(), n.clone()));
        }
        if !req.title.is_empty() {
            p.push(("description".into(), req.title.clone()));
        }
        if req.mode == "queue" {
            p.push(("later".into(), "1".into()));
        }
        self.open("info", p);
    }

    fn links(&self, urls: Vec<String>, page_url: Option<String>) {
        let mut p = vec![("source".to_string(), "extension".to_string()), ("urls".into(), serde_json::to_string(&urls).unwrap())];
        if let Some(r) = page_url {
            p.push(("referer".into(), r));
        }
        self.open("links", p);
    }

    fn started(&self, job_id: &str) {
        self.app.state::<AppState>().watch(job_id);
    }
}

/// Start the API; failures are reported in the UI (Options > Browser), not fatal.
pub async fn start(app: AppHandle) {
    let state = app.state::<AppState>();
    let preferred = state.mgr.settings().get().api_port;
    let pairings = PairingStore::open(swiftdm_core::appdata::pairings_path());
    let hooks: Arc<dyn ApiHooks> = Arc::new(Hooks { app: app.clone() });
    match api::start(state.mgr.clone(), pairings, hooks, Some(state.media.clone()), preferred).await {
        Ok(h) => *state.api.lock().unwrap() = Ok(h),
        Err(e) => {
            tracing::error!("{e}");
            *state.api.lock().unwrap() = Err(e);
        }
    }
    let _ = app.emit("core", json!({ "type": "extension_changed" }));
}
