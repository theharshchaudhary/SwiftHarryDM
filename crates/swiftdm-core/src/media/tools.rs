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
        if cfg!(debug_assertions) {
            // Development: target\debug\app.exe -> repo\tools\bin (scripts/fetch-tools.mjs)
            for up in exe_dir.ancestors().take(4) {
                d.push(up.join("tools").join("bin"));
            }
        }
    }
    if cfg!(debug_assertions) {
        d.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tools").join("bin"));
    }
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
    update_ytdlp_in(&crate::appdata::tools_dir(), base_url).await
}

/// `update_ytdlp` into a given folder.
pub async fn update_ytdlp_in(dir: &Path, base_url: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
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

// ---------------------------------------------------------------------- on-demand components

/// Pinned, checksum-verified downloads (keep in step with scripts/fetch-tools.mjs).
pub const YTDLP_PINNED: (&str, &str) = (
    "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe",
    "66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a",
);
/// FFmpeg LGPL build (no GPL-only components).
pub const FFMPEG_PINNED: (&str, &str, &str) = (
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-30-13-08/ffmpeg-n8.1.3-9-g29e619e767-win64-lgpl-8.1.zip",
    "4a7642b2264c03e8a0ce8a3825b933ee5580656f45695a086fe7e294045ffc0a",
    "ffmpeg-n8.1.3-9-g29e619e767-win64-lgpl-8.1",
);

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentStatus {
    pub ytdlp: Option<String>,
    pub ffmpeg: bool,
    pub ffprobe: bool,
    pub ready: bool,
}

pub fn status() -> ComponentStatus {
    let t = Tools::discover();
    ComponentStatus {
        ytdlp: t.ytdlp.as_ref().map(|p| p.to_string_lossy().into_owned()),
        ffmpeg: t.ffmpeg.is_some(),
        ffprobe: t.ffprobe.is_some(),
        ready: t.ytdlp.is_some() && t.ffmpeg.is_some() && t.ffprobe.is_some(),
    }
}

/// Stream `url` to `dest`, checking SHA-256; `progress(done, total)` is called as data arrives.
pub async fn fetch_verified(url: &str, sha256: &str, dest: &Path, progress: &(dyn Fn(u64, Option<u64>) + Send + Sync)) -> Result<(), String> {
    use futures_util::StreamExt;
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let client = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let r = client.get(url).send().await.map_err(|e| format!("Could not download {url}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("Could not download {url}: {}", r.status()));
    }
    let total = r.content_length();
    let tmp = dest.with_extension("part");
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut done = 0u64;
    let mut s = r.bytes_stream();
    while let Some(chunk) = s.next().await {
        let c = chunk.map_err(|e| format!("The download was interrupted: {e}"))?;
        h.update(&c);
        f.write_all(&c).map_err(|e| e.to_string())?;
        done += c.len() as u64;
        progress(done, total);
    }
    drop(f);
    if hex::encode(h.finalize()) != sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err("The downloaded file failed its SHA-256 check; nothing was installed.".into());
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

/// Install whatever is missing of yt-dlp, FFmpeg and ffprobe into the app data tools folder.
/// `progress(step, done, total)` reports "yt-dlp" / "ffmpeg" / "unpacking".
pub async fn install_components(progress: &(dyn Fn(&str, u64, Option<u64>) + Send + Sync)) -> Result<ComponentStatus, String> {
    let dir = crate::appdata::tools_dir();
    let have = Tools::discover();
    if have.ytdlp.is_none() {
        let (url, sha) = YTDLP_PINNED;
        fetch_verified(url, sha, &dir.join("yt-dlp.exe"), &|d, t| progress("yt-dlp", d, t)).await?;
    }
    if have.ffmpeg.is_none() || have.ffprobe.is_none() {
        let (url, sha, root) = FFMPEG_PINNED;
        let work = dir.join("ffmpeg-download");
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let zip = work.join("ffmpeg.zip");
        fetch_verified(url, sha, &zip, &|d, t| progress("ffmpeg", d, t)).await?;
        progress("unpacking", 0, None);
        // Windows 10+ includes bsdtar, which reads zip archives.
        let tar = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("System32").join("tar.exe");
        let members: Vec<String> = ["bin/ffmpeg.exe", "bin/ffprobe.exe", "LICENSE.txt"].iter().map(|m| format!("{root}/{m}")).collect();
        let mut cmd = tokio::process::Command::new(&tar);
        cmd.arg("-xf").arg(&zip).arg("-C").arg(&work).args(&members).creation_flags(0x0800_0000);
        let out = cmd.output().await.map_err(|e| format!("Could not unpack FFmpeg: {e}"))?;
        if !out.status.success() {
            return Err(format!("Could not unpack FFmpeg: {}", String::from_utf8_lossy(&out.stderr)));
        }
        for (src, dst) in [("bin/ffmpeg.exe", "ffmpeg.exe"), ("bin/ffprobe.exe", "ffprobe.exe"), ("LICENSE.txt", "FFMPEG-LICENSE.txt")] {
            std::fs::rename(work.join(root).join(src), dir.join(dst)).map_err(|e| format!("Could not install {dst}: {e}"))?;
        }
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(status())
}

/// Keep yt-dlp current without bothering the user: at most once a week, only for the copy
/// in the app data folder, verified like a manual update. Returns the new version if updated.
pub async fn auto_update_ytdlp() -> Option<String> {
    let dir = crate::appdata::tools_dir();
    let exe = dir.join("yt-dlp.exe");
    let stamp = dir.join("yt-dlp.checked");
    if !exe.is_file() {
        return None;
    }
    let fresh = std::fs::metadata(&stamp).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < std::time::Duration::from_secs(7 * 24 * 3600));
    if fresh {
        return None;
    }
    let _ = std::fs::write(&stamp, b"");
    let before = version_of(&exe).await;
    match update_ytdlp(YTDLP_RELEASE).await {
        Ok(v) if Some(&v) != before.as_ref() => {
            tracing::info!("yt-dlp updated to {v}");
            Some(v)
        }
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("yt-dlp update check failed: {e}");
            None
        }
    }
}
