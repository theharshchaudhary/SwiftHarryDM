//! IDM-style file categories: each has file extensions and its own download folder.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub extensions: Vec<String>,
    /// Folder name under the base download folder (or an absolute path).
    pub folder: String,
}

pub fn defaults() -> Vec<Category> {
    let c = |name: &str, exts: &str| Category {
        name: name.into(),
        extensions: exts.split_whitespace().map(str::to_string).collect(),
        folder: name.into(),
    };
    vec![
        c("Compressed", "zip rar 7z gz bz2 xz tar tgz arj sit sitx sea ace lz lzh cab iso img"),
        c("Documents", "doc docx pdf ppt pptx pps xls xlsx odt ods odp rtf txt csv epub"),
        c("Music", "mp3 wav wma aac m4a flac ogg opus aif aiff mpa ra ram mid"),
        c("Programs", "exe msi msix appx apk dmg pkg deb rpm bat"),
        c("Video", "mp4 mkv avi mov wmv flv webm m4v mpg mpeg mpe asf qt rm ts m2ts ogv 3gp"),
    ]
}

pub fn categorize<'a>(cats: &'a [Category], file_name: &str) -> Option<&'a Category> {
    let ext = Path::new(file_name).extension()?.to_string_lossy().to_lowercase();
    cats.iter().find(|c| c.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)))
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
        assert_eq!(categorize(&c, "setup.EXE").unwrap().name, "Programs");
        assert_eq!(categorize(&c, "a.tar.gz").unwrap().name, "Compressed");
        assert_eq!(categorize(&c, "movie.mkv").unwrap().name, "Video");
        assert!(categorize(&c, "noext").is_none());
        assert_eq!(folder_for(Path::new("C:\\D"), categorize(&c, "x.mp3")), Path::new("C:\\D\\Music"));
    }
}
