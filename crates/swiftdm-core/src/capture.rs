//! Should a download the browser started be taken over by the app? Shared by the local API
//! (extension requests) and the UI (so the Options dialog can explain a decision).
use serde::Serialize;

use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", content = "reason", rename_all = "snake_case")]
pub enum Decision {
    Capture,
    Skip(String),
}

pub struct Candidate<'a> {
    pub url: &'a str,
    /// File name suggested by the browser or the server, if known.
    pub file_name: Option<&'a str>,
    pub size: Option<u64>,
    /// Content type reported by the browser; used when the name has no extension yet.
    pub mime: Option<&'a str>,
    /// The user held the "force" key: take it over regardless of type and size.
    pub forced: bool,
}

/// Usual file extension for a content type (only types that are downloads, not pages).
pub fn mime_extension(mime: &str) -> Option<&'static str> {
    let m = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    Some(match m.as_str() {
        "application/pdf" => "pdf",
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/x-7z-compressed" => "7z",
        "application/vnd.rar" | "application/x-rar-compressed" => "rar",
        "application/gzip" | "application/x-gzip" => "gz",
        "application/x-tar" => "tar",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" | "application/x-msdos-program" => "exe",
        "application/x-msi" | "application/x-ms-installer" => "msi",
        "application/vnd.android.package-archive" => "apk",
        "application/x-iso9660-image" => "iso",
        "application/x-apple-diskimage" => "dmg",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/flac" => "flac",
        "audio/wav" | "audio/x-wav" => "wav",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/quicktime" => "mov",
        "video/x-msvideo" => "avi",
        _ => return None,
    })
}

pub fn decide(s: &Settings, c: &Candidate) -> Decision {
    let Ok(u) = url::Url::parse(c.url) else { return Decision::Skip("not a web address".into()) };
    if !matches!(u.scheme(), "http" | "https") {
        return Decision::Skip("only http and https downloads are taken over".into());
    }
    let host = u.host_str().unwrap_or_default();
    if crate::wildcard::any_host(&s.capture_skip_sites, host) {
        return Decision::Skip(format!("{host} is in the list of sites to leave to the browser"));
    }
    if s.capture_skip_urls.iter().any(|p| crate::wildcard::matches(p, c.url)) {
        return Decision::Skip("the address is in the list of addresses to leave to the browser".into());
    }
    if c.forced {
        return Decision::Capture;
    }
    let name = c.file_name.map(str::to_string).unwrap_or_else(|| {
        u.path_segments().and_then(|mut p| p.next_back().map(str::to_string)).unwrap_or_default()
    });
    let ext = std::path::Path::new(&name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .or_else(|| c.mime.and_then(mime_extension).map(str::to_string));
    match ext {
        Some(e) if s.capture_extensions.contains(&e) => {}
        Some(e) => return Decision::Skip(format!(".{e} files are not in the list of file types to take over")),
        None => return Decision::Skip("the file type is unknown".into()),
    }
    if let Some(size) = c.size {
        if s.capture_min_size_kb > 0 && size < s.capture_min_size_kb * 1024 {
            return Decision::Skip(format!("smaller than {} KB", s.capture_min_size_kb));
        }
    }
    Decision::Capture
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cand(url: &str) -> Candidate<'_> {
        Candidate { url, file_name: None, size: None, mime: None, forced: false }
    }
    #[test]
    fn rules() {
        let s = Settings {
            capture_skip_sites: vec!["intranet.local".into()],
            capture_skip_urls: vec!["https://*.example.com/*.exe".into()],
            capture_min_size_kb: 100,
            ..Settings::default()
        };
        assert_eq!(decide(&s, &cand("https://a.org/f.zip")), Decision::Capture);
        assert!(matches!(decide(&s, &cand("https://a.org/page.html")), Decision::Skip(_)));
        assert!(matches!(decide(&s, &cand("https://a.org/noext")), Decision::Skip(_)));
        assert!(matches!(decide(&s, &cand("https://files.intranet.local/f.zip")), Decision::Skip(_)));
        assert!(matches!(decide(&s, &cand("https://dl.example.com/x/setup.exe")), Decision::Skip(_)));
        assert!(matches!(decide(&s, &cand("ftp://a.org/f.zip")), Decision::Skip(_)));
        let small = Candidate { size: Some(10 * 1024), ..cand("https://a.org/f.zip") };
        assert!(matches!(decide(&s, &small), Decision::Skip(_)));
        let named = Candidate { file_name: Some("Report.PDF"), ..cand("https://a.org/get?id=1") };
        assert_eq!(decide(&s, &named), Decision::Capture);
        let forced = Candidate { forced: true, ..cand("https://a.org/page.html") };
        assert_eq!(decide(&s, &forced), Decision::Capture);
        let forced_excluded = Candidate { forced: true, ..cand("https://intranet.local/f.zip") };
        assert!(matches!(decide(&s, &forced_excluded), Decision::Skip(_)));
        // The browser often knows only the content type when the download starts.
        let typed = Candidate { mime: Some("application/pdf; charset=binary"), ..cand("https://a.org/get?id=1") };
        assert_eq!(decide(&s, &typed), Decision::Capture);
        let page = Candidate { mime: Some("text/html"), ..cand("https://a.org/get?id=1") };
        assert!(matches!(decide(&s, &page), Decision::Skip(_)));
    }
}
