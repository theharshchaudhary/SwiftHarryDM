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
    /// The user held the "force" key: take it over regardless of type and size.
    pub forced: bool,
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
    let ext = std::path::Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase());
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
        Candidate { url, file_name: None, size: None, forced: false }
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
    }
}
