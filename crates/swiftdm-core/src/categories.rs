//! IDM-style file categories: each has file extensions and its own download folder.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub extensions: Vec<String>,
    /// Folder name under the base download folder (or an absolute path).
    pub folder: String,
    /// Only files from these sites ("example.com" covers subdomains, * is a wildcard).
    /// Empty = any site.
    #[serde(default)]
    pub sites: Vec<String>,
}

pub fn defaults() -> Vec<Category> {
    let c = |name: &str, exts: &str| Category {
        name: name.into(),
        extensions: exts.split_whitespace().map(str::to_string).collect(),
        folder: name.into(),
        sites: Vec::new(),
    };
    vec![
        c("Compressed", "zip rar 7z gz bz2 xz tar tgz arj sit sitx sea ace lz lzh cab iso img"),
        c("Documents", "doc docx pdf ppt pptx pps xls xlsx odt ods odp rtf txt csv epub"),
        c("Music", "mp3 wav wma aac m4a flac ogg opus aif aiff mpa ra ram mid"),
        c("Programs", "exe msi msix appx apk dmg pkg deb rpm bat"),
        c("Video", "mp4 mkv avi mov wmv flv webm m4v mpg mpeg mpe asf qt rm ts m2ts ogv 3gp"),
    ]
}

/// The category for a file. A category limited to some sites wins over one that only matches
/// the file type; a site-limited category with no file types takes every file from its sites.
pub fn categorize<'a>(cats: &'a [Category], file_name: &str, host: Option<&str>) -> Option<&'a Category> {
    let ext = Path::new(file_name).extension().map(|e| e.to_string_lossy().to_lowercase());
    let type_ok = |c: &Category| match &ext {
        Some(ext) => c.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)),
        None => false,
    };
    let from_site = |c: &Category| !c.sites.is_empty() && host.is_some_and(|h| crate::wildcard::any_host(&c.sites, h));
    cats.iter()
        .find(|c| from_site(c) && (c.extensions.is_empty() || type_ok(c)))
        .or_else(|| cats.iter().find(|c| c.sites.is_empty() && type_ok(c)))
}

/// Folder for a category under the base folder ("General" = the base folder itself).
pub fn folder_for(base: &Path, cat: Option<&Category>) -> std::path::PathBuf {
    match cat {
        Some(c) if Path::new(&c.folder).is_absolute() => c.folder.clone().into(),
        Some(c) => base.join(&c.folder),
        None => base.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categorizes_by_extension() {
        let c = defaults();
        assert_eq!(categorize(&c, "setup.EXE", None).unwrap().name, "Programs");
        assert_eq!(categorize(&c, "a.tar.gz", None).unwrap().name, "Compressed");
        assert_eq!(categorize(&c, "movie.mkv", None).unwrap().name, "Video");
        assert!(categorize(&c, "noext", None).is_none());
        assert_eq!(folder_for(Path::new("C:\\D"), categorize(&c, "x.mp3", None)), Path::new("C:\\D\\Music"));
    }

    #[test]
    fn site_categories_win_and_only_take_their_sites() {
        let mut c = defaults();
        let site = |name: &str, exts: &str, sites: &str| Category {
            name: name.into(),
            extensions: exts.split_whitespace().map(str::to_string).collect(),
            folder: name.into(),
            sites: sites.split_whitespace().map(str::to_string).collect(),
        };
        c.insert(0, site("Lectures", "mp4 pdf", "uni.example.edu"));
        c.push(site("Mirror", "", "*.mirror.example.org"));
        assert_eq!(categorize(&c, "week1.mp4", Some("cdn.uni.example.edu")).unwrap().name, "Lectures");
        assert_eq!(categorize(&c, "week1.mp4", Some("example.com")).unwrap().name, "Video");
        assert_eq!(categorize(&c, "week1.mp4", None).unwrap().name, "Video");
        // the site category does not take other file types from its site
        assert_eq!(categorize(&c, "setup.exe", Some("uni.example.edu")).unwrap().name, "Programs");
        // no file types = everything from the site, even files without an extension
        assert_eq!(categorize(&c, "setup.exe", Some("dl.mirror.example.org")).unwrap().name, "Mirror");
        assert_eq!(categorize(&c, "README", Some("dl.mirror.example.org")).unwrap().name, "Mirror");
        assert!(categorize(&c, "README", Some("mirror.example.org")).is_none());
    }
}
