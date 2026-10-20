//! Browser integration that needs no typing: the app registers itself as a native messaging
//! host for Chrome and Edge, so the SwiftHarryDM extension (and only it: the browser checks
//! the extension ID) can ask for its access token directly. When the extension is published
//! in the stores, the store IDs below make the browsers offer it automatically.
use std::io::{Read, Write};
use std::path::PathBuf;

use serde_json::{json, Value};
use windows::core::HSTRING;
use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

pub const HOST_NAME: &str = "np.com.harshchaudhary.swiftharrydm";
/// ID of the extension as shipped with the app (fixed by the "key" in its manifest).
pub const EXTENSION_ID: &str = "eogdkmmfefponkkkcfgdlgnngmdjckhe";
/// Fill in once the extension is published; the browsers then install it on their own.
pub const CHROME_STORE_ID: Option<&str> = None;
pub const EDGE_STORE_ID: Option<&str> = None;

fn allowed_ids() -> Vec<&'static str> {
    let mut v = vec![EXTENSION_ID];
    v.extend(CHROME_STORE_ID);
    v.extend(EDGE_STORE_ID);
    v
}

pub fn set_reg(key: &str, name: Option<&str>, value: &str) -> windows::core::Result<()> {
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
    let name = name.map(HSTRING::from);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            name.as_ref().map(|n| windows::core::PCWSTR(n.as_ptr())).unwrap_or(windows::core::PCWSTR::null()),
            REG_SZ.0,
            Some(bytes.as_ptr() as *const _),
            bytes.len() as u32,
        )
        .ok()
    }
}

/// Register (or refresh) the native messaging host and the store install hints. Runs at every
/// start, so moving or updating the app keeps the registration correct.
pub fn register() {
    let Ok(exe) = std::env::current_exe() else { return };
    let dir = swiftdm_core::appdata::app_dir();
    let manifest = dir.join("native-host.json");
    // A custom profile folder (portable use, tests) must reach the host process too, which the
    // browser starts without our environment: point the browser at a wrapper that sets it.
    let host_path = match std::env::var_os("SWIFTDM_HOME") {
        Some(home) => {
            let cmd = dir.join("native-host.cmd");
            let script = format!("@echo off\r\nset \"SWIFTDM_HOME={}\"\r\n\"{}\" %*\r\n", home.to_string_lossy(), exe.display());
            if std::fs::write(&cmd, script).is_err() {
                return;
            }
            cmd
        }
        None => exe.clone(),
    };
    let origins: Vec<String> = allowed_ids().iter().map(|id| format!("chrome-extension://{id}/")).collect();
    let body = json!({
        "name": HOST_NAME,
        "description": "SwiftHarryDM download manager",
        "path": host_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": origins,
    });
    if std::fs::write(&manifest, serde_json::to_vec_pretty(&body).unwrap()).is_err() {
        return;
    }
    let m = manifest.to_string_lossy().into_owned();
    for browser in [r"Software\Google\Chrome", r"Software\Microsoft\Edge", r"Software\BraveSoftware\Brave-Browser", r"Software\Chromium"] {
        if let Err(e) = set_reg(&format!(r"{browser}\NativeMessagingHosts\{HOST_NAME}"), None, &m) {
            tracing::warn!("could not register the browser connection for {browser}: {e}");
        }
    }
    if let Some(id) = CHROME_STORE_ID {
        let _ = set_reg(&format!(r"Software\Google\Chrome\Extensions\{id}"), Some("update_url"), "https://clients2.google.com/service/update2/crx");
        // Brave installs from the Chrome Web Store too.
        let _ = set_reg(&format!(r"Software\BraveSoftware\Brave-Browser\Extensions\{id}"), Some("update_url"), "https://clients2.google.com/service/update2/crx");
    }
    if let Some(id) = EDGE_STORE_ID {
        let _ = set_reg(&format!(r"Software\Microsoft\Edge\Extensions\{id}"), Some("update_url"), "https://edge.microsoft.com/extensionwebstorebase/v1/crx");
    }
}

/// Where the extension that ships with the app lives (for installing it by hand).
pub fn bundled_extension() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let mut c = vec![exe_dir.join("extension"), exe_dir.join("resources").join("extension")];
    if cfg!(debug_assertions) {
        c.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("extension"));
    }
    c.into_iter().find(|p| p.join("manifest.json").is_file())
}

fn read_message(r: &mut impl Read) -> Option<Value> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).ok()?;
    let n = u32::from_le_bytes(len) as usize;
    if n > 64 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

fn write_message(w: &mut impl Write, v: &Value) {
    let b = serde_json::to_vec(v).unwrap();
    let _ = w.write_all(&(b.len() as u32).to_le_bytes());
    let _ = w.write_all(&b);
    let _ = w.flush();
}

/// Entry point when a browser starts the app as its native messaging host: the first
/// argument is the calling extension's origin. Answers one request and exits.
pub fn native_host(origin: &str) -> i32 {
    let id = origin.trim_start_matches("chrome-extension://").trim_end_matches('/');
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let Some(msg) = read_message(&mut stdin) else { return 1 };
    if !allowed_ids().contains(&id) {
        write_message(&mut stdout, &json!({ "success": false, "error": "unknown extension" }));
        return 1;
    }
    match msg["type"].as_str() {
        Some("hello") => {
            let store = swiftdm_core::api::auth::PairingStore::open(swiftdm_core::appdata::pairings_path());
            let token = store.issue(id);
            let port = swiftdm_core::settings::SettingsStore::open(swiftdm_core::appdata::settings_path()).get().api_port;
            write_message(&mut stdout, &json!({ "success": true, "token": token, "port": port, "version": swiftdm_core::VERSION }));
            0
        }
        _ => {
            write_message(&mut stdout, &json!({ "success": false, "error": "unknown request" }));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn message_framing() {
        let mut buf = vec![];
        write_message(&mut buf, &json!({ "type": "hello" }));
        assert_eq!(read_message(&mut buf.as_slice()), Some(json!({ "type": "hello" })));
        let mut huge = (1u32 << 20).to_le_bytes().to_vec();
        huge.extend([0u8; 8]);
        assert_eq!(read_message(&mut huge.as_slice()), None);
    }
}

fn browser_exe(browser: &str) -> Option<PathBuf> {
    let pf = |v: &str| std::env::var_os(v).map(PathBuf::from);
    let cands: Vec<PathBuf> = match browser {
        "edge" => vec![pf("ProgramFiles(x86)"), pf("ProgramFiles")].into_iter().flatten().map(|p| p.join(r"Microsoft\Edge\Application\msedge.exe")).collect(),
        _ => vec![pf("ProgramFiles"), pf("ProgramFiles(x86)"), pf("LOCALAPPDATA")].into_iter().flatten().map(|p| p.join(r"Google\Chrome\Application\chrome.exe")).collect(),
    };
    cands.into_iter().find(|p| p.is_file())
}

#[derive(serde::Serialize)]
pub struct ExtensionSetup {
    /// "store" (the store page was opened) or "manual" (steps to follow).
    mode: &'static str,
    /// Folder to choose with "Load unpacked".
    folder: Option<String>,
    browser_found: bool,
}

/// Help the user add the extension: the store page when published, otherwise the bundled
/// copy in a fixed folder plus the browser's extensions page.
#[tauri::command]
pub fn extension_setup(browser: String) -> Result<ExtensionSetup, String> {
    let exe = browser_exe(&browser);
    let store = match browser.as_str() {
        "edge" => EDGE_STORE_ID.map(|id| format!("https://microsoftedge.microsoft.com/addons/detail/{id}")),
        _ => CHROME_STORE_ID.map(|id| format!("https://chromewebstore.google.com/detail/{id}")),
    };
    if let Some(url) = store {
        let r = match &exe {
            Some(e) => std::process::Command::new(e).arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string()),
            None => crate::system::open_path(&url),
        };
        r?;
        return Ok(ExtensionSetup { mode: "store", folder: None, browser_found: exe.is_some() });
    }
    let src = bundled_extension().ok_or("The extension files are missing from this installation.")?;
    let dest = swiftdm_core::appdata::app_dir().join("extension");
    copy_dir(&src, &dest).map_err(|e| format!("Could not prepare the extension: {e}"))?;
    if let Some(e) = &exe {
        let page = if browser == "edge" { "edge://extensions/" } else { "chrome://extensions/" };
        let _ = std::process::Command::new(e).arg(page).spawn();
    }
    let _ = crate::system::open_path(&dest.to_string_lossy());
    Ok(ExtensionSetup { mode: "manual", folder: Some(dest.to_string_lossy().into_owned()), browser_found: exe.is_some() })
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), to)?;
        }
    }
    Ok(())
}
