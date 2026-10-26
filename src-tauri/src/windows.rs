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
        "browser" => s("Connect your browser", 600.0, 330.0, false, None, false),
        "drop" => s("Drop target", 88.0, 88.0, false, None, true),
        "login" => s("Login required", 460.0, 270.0, false, Some("id"), false),
        "tips" => s("Tip of the day", 480.0, 230.0, false, None, false),
        "grabber" => s("Site grabber", 960.0, 620.0, true, None, false),
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

/// Extra WebView2 arguments, the same for every window (windows sharing a profile must agree).
/// Debug builds started with SWIFTDM_DEVTOOLS_PORT=<port> open the DevTools protocol on that
/// port for the end-to-end tests; WebView2's own environment variable for this is ignored on
/// some machines (GitHub's Windows runners).
pub fn browser_args() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let port: u16 = std::env::var("SWIFTDM_DEVTOOLS_PORT").ok()?.parse().ok()?;
    // Tauri's defaults, which setting our own arguments would otherwise drop.
    Some(format!("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"))
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
        let w2 = w.clone();
        let _ = app.run_on_main_thread(move || bring_to_front(&w2));
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
    if let Some(args) = browser_args() {
        b = b.additional_browser_args(&args);
    }
    if kind == "drop" {
        // A small round button that floats over other windows and accepts dropped links.
        b = b.decorations(false).transparent(true).shadow(false).skip_taskbar(true).min_inner_size(sp.width, sp.height);
        if let Some(m) = app.primary_monitor().ok().flatten() {
            let s = m.size().to_logical::<f64>(m.scale_factor());
            b = b.position(s.width - 140.0, s.height - 200.0);
        }
        b.build().map_err(|e| e.to_string())?;
        return Ok(());
    }
    // Windows that answer something done in the browser open in front of it.
    let to_front = matches!(kind, "info" | "links" | "pairing" | "complete" | "countdown" | "login");
    if let Some(main) = app.get_webview_window("main") {
        if main.is_visible().unwrap_or(false) && !to_front {
            b = b.parent(&main).map_err(|e| e.to_string())?;
        }
    }
    let w = b.build().map_err(|e| e.to_string())?;
    if to_front {
        let w2 = w.clone();
        let _ = app.run_on_main_thread(move || bring_to_front(&w2));
    }
    Ok(())
}

/// Put a window in front of the app the user is working in (the browser). Being made topmost
/// for a moment shows it above every other window even when Windows refuses to hand a
/// background app the keyboard; then it asks for the keyboard as well.
pub fn bring_to_front(w: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetForegroundWindow, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };
    let Ok(h) = w.hwnd() else { return };
    let hwnd = HWND(h.0 as _);
    unsafe {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE;
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
        let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
        let _ = SetForegroundWindow(hwnd);
    }
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
