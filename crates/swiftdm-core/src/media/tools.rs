//! Locate the external media tools: yt-dlp.exe, ffmpeg.exe, ffprobe.exe and an optional
//! JavaScript runtime (yt-dlp uses one for full YouTube support).
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Tools {
    pub ytdlp: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    /// ("deno" | "node", path)
    pub js: Option<(String, PathBuf)>,
}

/// Folders searched in order: an updated copy in the app data folder, the folder next to
/// the program (installed build), `tools\bin` of a source checkout, then PATH.
fn search_dirs() -> Vec<PathBuf> {
    let mut d = vec![crate::appdata::tools_dir()];
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        d.push(exe_dir.join("tools"));
        d.push(exe_dir.clone());
        // target\debug\app.exe -> repo\tools\bin
        for up in exe_dir.ancestors().take(4) {
            d.push(up.join("tools").join("bin"));
        }
    }
    d.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tools").join("bin"));
    if let Some(path) = std::env::var_os("PATH") {
        d.extend(std::env::split_paths(&path));
    }
    d
}

pub fn find(exe: &str) -> Option<PathBuf> {
    search_dirs().into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

impl Tools {
    pub fn discover() -> Tools {
        let js = ["deno", "node"].iter().find_map(|n| find(&format!("{n}.exe")).map(|p| (n.to_string(), p)));
        Tools { ytdlp: find("yt-dlp.exe"), ffmpeg: find("ffmpeg.exe"), ffprobe: find("ffprobe.exe"), js }
    }
}

pub const YTDLP_RELEASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/";

/// `<exe> --version`, or None if it does not run.
pub async fn version_of(exe: &Path) -> Option<String> {
    let mut out = String::new();
    let cmd = vec![exe.to_string_lossy().into_owned(), "--version".to_string()];
    let r = super::runner::run(
        &cmd,
        &crate::cancel::CancelToken::new(),
        |l| {
            if let super::runner::Line::Out(l) = l {
                if out.is_empty() {
                    out = l.trim().to_string();
                }
            }
        },
        Some(std::time::Duration::from_secs(60)),
    )
    .await
    .ok()?;
    (r.code == Some(0) && !out.is_empty()).then_some(out)
}

/// Download the latest yt-dlp.exe into the app data tools folder, verify it against the
/// release's SHA2-256SUMS and check that it runs, then swap it in (the previous copy is kept
/// as yt-dlp.exe.prev). Returns the new version.
pub async fn update_ytdlp(base_url: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let dir = crate::appdata::tools_dir();
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(300)).build().map_err(|e| e.to_string())?;
    let get = |url: String| {
        let client = client.clone();
        async move {
            let r = client.get(&url).send().await.map_err(|e| format!("Download failed: {e}"))?;
            if !r.status().is_success() {
                return Err(format!("Download failed: {} for {url}", r.status()));
            }
            r.bytes().await.map_err(|e| format!("Download failed: {e}"))
        }
    };
    let sums = get(format!("{base_url}SHA2-256SUMS")).await?;
    let expected = String::from_utf8_lossy(&sums)
        .lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .find(|(_, name)| name.trim().trim_start_matches('*') == "yt-dlp.exe")
        .map(|(h, _)| h.trim().to_lowercase())
        .ok_or("yt-dlp.exe is not listed in the release's SHA2-256SUMS.")?;
    let bin = get(format!("{base_url}yt-dlp.exe")).await?;
    if hex::encode(Sha256::digest(&bin)) != expected {
        return Err("The downloaded yt-dlp.exe failed its SHA-256 check; nothing was changed.".into());
    }
    let new = dir.join("yt-dlp.exe.new");
    std::fs::write(&new, &bin).map_err(|e| e.to_string())?;
    let Some(ver) = version_of(&new).await else {
        let _ = std::fs::remove_file(&new);
        return Err("The downloaded yt-dlp.exe does not run; nothing was changed.".into());
    };
    let cur = dir.join("yt-dlp.exe");
    if cur.exists() {
        let _ = std::fs::rename(&cur, dir.join("yt-dlp.exe.prev"));
    }
    std::fs::rename(&new, &cur).map_err(|e| e.to_string())?;
    Ok(ver)
}

/// Go back to the copy that was replaced by the last update.
pub fn rollback_ytdlp() -> bool {
    let dir = crate::appdata::tools_dir();
    let prev = dir.join("yt-dlp.exe.prev");
    prev.exists() && std::fs::rename(&prev, dir.join("yt-dlp.exe")).is_ok()
}
