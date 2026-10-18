//! Secondary windows (dialogs). Every window loads the same page; the kind and its
//! parameters are injected before the page runs, so no URL parsing is involved.
use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

static SEQ: AtomicU64 = AtomicU64::new(1);

struct Spec {
    title: &'static str,
    width: f64,
    height: f64,
    resizable: bool,
    /// One window per value of this parameter (e.g. one progress window per download);
    /// None = one window of this kind; Some("") = always a new window.
    key: Option<&'static str>,
    on_top: bool,
}

fn spec(kind: &str) -> Option<Spec> {
    let s = |title, width, height, resizable, key, on_top| Spec { title, width, height, resizable, key, on_top };
    Some(match kind {
        "add" => s("Add download", 540.0, 210.0, false, None, false),
        "info" => s("Download file info", 640.0, 420.0, true, Some(""), false),
        "progress" => s("Download progress", 560.0, 520.0, true, Some("id"), false),
        "complete" => s("Download complete", 580.0, 280.0, false, Some("id"), true),
        "options" => s("Options", 800.0, 600.0, true, None, false),
        "scheduler" => s("Scheduler", 800.0, 540.0, true, None, false),
        "batch" => s("Batch download", 640.0, 560.0, true, None, false),
        "links" => s("Choose files to download", 720.0, 540.0, true, Some(""), false),
        "properties" => s("Properties", 620.0, 560.0, true, Some("id"), false),
        "about" => s("About SwiftHarryDM", 460.0, 280.0, false, None, false),
        "countdown" => s("SwiftHarryDM", 400.0, 250.0, false, None, true),
        "pairing" => s("Connect browser extension", 440.0, 300.0, false, Some("pid"), true),
        _ => return None,
    })
}

fn encode(v: &str) -> String {
    let mut out = String::new();
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn open(app: &AppHandle, kind: &str, params: &[(String, String)]) -> Result<(), String> {
    let sp = spec(kind).ok_or_else(|| format!("unknown window kind {kind}"))?;
    let label = match sp.key {
        None => kind.to_string(),
        Some("") => format!("{kind}-{}", SEQ.fetch_add(1, Ordering::Relaxed)),
        Some(k) => {
            let v = params.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.as_str()).unwrap_or("");
            // Labels allow only a limited alphabet.
            format!("{kind}-{}", v.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>())
        }
    };
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let mut query = format!("w={}", encode(kind));
    for (k, v) in params {
        query.push_str(&format!("&{}={}", encode(k), encode(v)));
    }
    let init = format!("window.__SDM_PARAMS__ = {};", serde_json::to_string(&query).unwrap());
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(sp.title)
        .inner_size(sp.width, sp.height)
        .min_inner_size(sp.width.min(400.0), sp.height.min(200.0))
        .resizable(sp.resizable)
        .maximizable(sp.resizable)
        .always_on_top(sp.on_top)
        .disable_drag_drop_handler()
        .initialization_script(&init)
        .center()
        .focused(true);
    if let Some(main) = app.get_webview_window("main") {
        if main.is_visible().unwrap_or(false) && kind != "countdown" && kind != "complete" {
            b = b.parent(&main).map_err(|e| e.to_string())?;
        }
    }
    b.build().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn close(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.close();
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
