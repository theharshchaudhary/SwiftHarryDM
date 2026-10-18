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
