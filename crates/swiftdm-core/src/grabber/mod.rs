//! Site grabber (IDM's "Grabber"): explore a web site from a start page, list the files it
//! links to, and download the ones chosen. A project keeps its settings and what was found,
//! so it can be explored again later.
pub mod crawl;
pub mod html;

use serde::{Deserialize, Serialize};

/// What a project looks for (IDM's project templates).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Template {
    Images,
    Audio,
    Video,
    /// Every file that is not a web page.
    #[default]
    Files,
    /// Pages and files: a copy of the site.
    Site,
    /// Only the file types listed in the project.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SaveMode {
    /// Each file in its category's folder.
    #[default]
    Categories,
    /// Everything in one folder.
    Folder,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub start_url: String,
    pub template: Template,
    // Where files go.
    pub save_mode: SaveMode,
    /// For `SaveMode::Folder` ("" = the download folder).
    pub folder: String,
    /// Recreate the site's folders under the destination.
    pub keep_subfolders: bool,
    // Which pages are explored.
    /// Link levels followed on the start page's site (0 = the start page only).
    pub depth: u32,
    /// Link levels followed on other sites (0 = never explore them).
    pub other_sites_depth: u32,
    /// Do not go above the start page's folder.
    pub no_parent: bool,
    /// Subdomains of the start page's domain count as the same site.
    pub whole_domain: bool,
    /// Address patterns (with *) that pages must match / must not match.
    pub include_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
    // Which files are listed.
    /// Extensions for `Template::Custom` (and extra ones for the other templates).
    pub file_types: Vec<String>,
    pub exclude_types: Vec<String>,
    /// Only files on the start page's site.
    pub files_this_site_only: bool,
    /// Hide a file with the same name and size as one already listed.
    pub hide_duplicates: bool,
    /// Bytes; 0 = no limit.
    pub min_size: u64,
    pub max_size: u64,
    /// Download matching files as soon as they are found.
    pub start_at_once: bool,
    // Politeness.
    pub max_pages: u32,
    pub parallel: u32,
    pub delay_ms: u32,
    pub respect_robots: bool,
    pub created_at: f64,
    pub updated_at: f64,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            id: String::new(),
            name: String::new(),
            start_url: String::new(),
            template: Template::Files,
            save_mode: SaveMode::Categories,
            folder: String::new(),
            keep_subfolders: false,
            depth: 1,
            other_sites_depth: 0,
            no_parent: true,
            whole_domain: false,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
            file_types: Vec::new(),
            exclude_types: Vec::new(),
            files_this_site_only: false,
            hide_duplicates: true,
            min_size: 0,
            max_size: 0,
            start_at_once: false,
            max_pages: 500,
            parallel: 2,
            delay_ms: 250,
            respect_robots: true,
            created_at: 0.0,
            updated_at: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Page,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// A file waiting to be chosen, or a page waiting to be explored.
    Found,
    Explored,
    /// A page not explored (robots.txt, limits) or a file outside the filters.
    Skipped,
    Failed,
    /// Sent to the download list.
    Queued,
}

impl ItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemStatus::Found => "found",
            ItemStatus::Explored => "explored",
            ItemStatus::Skipped => "skipped",
            ItemStatus::Failed => "failed",
            ItemStatus::Queued => "queued",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "explored" => ItemStatus::Explored,
            "skipped" => ItemStatus::Skipped,
            "failed" => ItemStatus::Failed,
            "queued" => ItemStatus::Queued,
            _ => ItemStatus::Found,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub project_id: String,
    pub url: String,
    pub parent_url: Option<String>,
    pub depth: u32,
    pub kind: ItemKind,
    pub content_type: Option<String>,
    pub size: Option<u64>,
    pub link_text: Option<String>,
    pub checked: bool,
    pub status: ItemStatus,
    /// Why a page was skipped or a request failed.
    pub note: Option<String>,
    pub job_id: Option<String>,
}

/// Counts shown while a project is explored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Stats {
    pub pages_explored: u32,
    pub pages_waiting: u32,
    pub files_found: u32,
    pub errors: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GrabberEvent {
    Item { project_id: String, item: Item },
    Stats { project_id: String, stats: Stats },
    /// Exploring ended: finished, stopped by the user, or an error before anything was found.
    Done { project_id: String, reason: String },
}

pub const PAGE_EXTS: &[&str] = &["", "html", "htm", "shtml", "xhtml", "php", "asp", "aspx", "jsp", "cfm", "cgi", "pl"];
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "tif", "tiff", "ico", "avif", "heic"];
const AUDIO_EXTS: &[&str] = &["mp3", "wav", "wma", "aac", "m4a", "flac", "ogg", "opus", "mid", "aiff"];
const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "3gp", "ts"];
/// Never offered as files, even by "all files" (they are parts of pages, not downloads).
const PAGE_PARTS: &[&str] = &["css", "js", "mjs", "map", "json", "xml", "woff", "woff2", "ttf", "eot", "otf"];

/// The lower-case extension of an address's last path segment ("" if none).
pub fn url_ext(url: &url::Url) -> String {
    let last = url.path_segments().and_then(|mut s| s.next_back()).unwrap_or("");
    match last.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 6 => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

impl Project {
    /// Whether a found address is a file this project lists (by its extension).
    pub fn wants_file(&self, ext: &str) -> bool {
        let ext = ext.to_ascii_lowercase();
        let listed = |l: &[String]| l.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext));
        if listed(&self.exclude_types) {
            return false;
        }
        if listed(&self.file_types) {
            return true;
        }
        match self.template {
            Template::Images => IMAGE_EXTS.contains(&ext.as_str()),
            Template::Audio => AUDIO_EXTS.contains(&ext.as_str()),
            Template::Video => VIDEO_EXTS.contains(&ext.as_str()),
            Template::Files => !PAGE_EXTS.contains(&ext.as_str()) && !PAGE_PARTS.contains(&ext.as_str()),
            Template::Site => !PAGE_EXTS.contains(&ext.as_str()),
            Template::Custom => false,
        }
    }

    /// The project with its blanks filled in (a new project, or one saved by an older app).
    pub fn normalized(mut self) -> Self {
        self.name = self.name.trim().to_string();
        self.start_url = self.start_url.trim().to_string();
        if self.name.is_empty() {
            self.name = url::Url::parse(&self.start_url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| "Project".into());
        }
        self.parallel = self.parallel.clamp(1, 8);
        self.max_pages = self.max_pages.clamp(1, 100_000);
        self.depth = self.depth.min(20);
        self.other_sites_depth = self.other_sites_depth.min(5);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_choose_file_types() {
        let p = |t| Project { template: t, ..Project::default() };
        assert!(p(Template::Images).wants_file("PNG"));
        assert!(!p(Template::Images).wants_file("pdf"));
        assert!(p(Template::Files).wants_file("pdf") && p(Template::Files).wants_file("zip"));
        assert!(!p(Template::Files).wants_file("html") && !p(Template::Files).wants_file("css") && !p(Template::Files).wants_file(""));
        assert!(p(Template::Site).wants_file("css"));
        let custom = Project { template: Template::Custom, file_types: vec![".pdf".into()], ..Project::default() };
        assert!(custom.wants_file("pdf") && !custom.wants_file("zip"));
        let no_iso = Project { exclude_types: vec!["iso".into()], ..Project::default() };
        assert!(!no_iso.wants_file("iso") && no_iso.wants_file("zip"));
    }

    #[test]
    fn extensions() {
        let e = |u: &str| url_ext(&url::Url::parse(u).unwrap());
        assert_eq!(e("https://a.b/x/report.PDF?v=1"), "pdf");
        assert_eq!(e("https://a.b/x/"), "");
        assert_eq!(e("https://a.b/.hidden"), "");
        assert_eq!(e("https://a.b/v1.2/page"), "");
        assert_eq!(e("https://a.b/file.tar.gz"), "gz");
    }
}
