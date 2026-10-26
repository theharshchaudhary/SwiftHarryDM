//! Locations of per-user application state: %LOCALAPPDATA%\Keepfetch unless
//! SWIFTDM_HOME is set (tests and portable installs).
use std::path::{Path, PathBuf};

/// The app's folder name before it was renamed to Keepfetch.
pub const PREVIOUS_NAME: &str = "SwiftHarryDM";

pub fn app_dir() -> PathBuf {
    let base = match std::env::var_os("SWIFTDM_HOME") {
        Some(p) => PathBuf::from(p),
        None => dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Keepfetch"),
    };
    let _ = std::fs::create_dir_all(&base);
    base
}

/// First start after the rename: bring the download list, settings, browser pairings,
/// window positions and updated tools over from the previous folder. The old folder is
/// left as it is (the old app may still be running). Returns what was copied.
pub fn migrate_previous(new_dir: &Path) -> Vec<String> {
    let Some(local) = dirs::data_local_dir() else { return vec![] };
    migrate_from(&local.join(PREVIOUS_NAME), new_dir)
}

pub fn migrate_from(old: &Path, new_dir: &Path) -> Vec<String> {
    let mut done = vec![];
    if !old.join("state3.db").is_file() || new_dir.join("state3.db").exists() {
        return done;
    }
    let _ = std::fs::create_dir_all(new_dir);
    // A consistent copy of the database even if the old app is writing to it.
    let db = (|| -> rusqlite::Result<()> {
        let c = rusqlite::Connection::open_with_flags(old.join("state3.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        c.execute("VACUUM INTO ?1", [new_dir.join("state3.db").to_string_lossy()])?;
        Ok(())
    })();
    match db {
        Ok(()) => done.push("state3.db".into()),
        Err(e) => {
            tracing::warn!("could not copy the previous download list: {e}");
            return done;
        }
    }
    for f in ["settings3.json", "pairings.json", "window.json"] {
        if old.join(f).is_file() && std::fs::copy(old.join(f), new_dir.join(f)).is_ok() {
            done.push(f.into());
        }
    }
    if let Ok(rd) = std::fs::read_dir(old.join("tools")) {
        let _ = std::fs::create_dir_all(new_dir.join("tools"));
        for e in rd.flatten() {
            if e.path().is_file() && std::fs::copy(e.path(), new_dir.join("tools").join(e.file_name())).is_ok() {
                done.push(format!("tools/{}", e.file_name().to_string_lossy()));
            }
        }
    }
    done
}

pub fn db_path() -> PathBuf {
    app_dir().join("state3.db")
}
pub fn settings_path() -> PathBuf {
    app_dir().join("settings3.json")
}
pub fn pairings_path() -> PathBuf {
    app_dir().join("pairings.json")
}
pub fn logs_dir() -> PathBuf {
    let d = app_dir().join("logs");
    let _ = std::fs::create_dir_all(&d);
    d
}
pub fn tools_dir() -> PathBuf {
    let d = app_dir().join("tools");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The user's Downloads folder (categories add Video, Music, ... below it).
pub fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_the_previous_folder_once() {
        let d = tempfile::tempdir().unwrap();
        let (old, new) = (d.path().join("old"), d.path().join("new"));
        std::fs::create_dir_all(old.join("tools")).unwrap();
        {
            let c = rusqlite::Connection::open(old.join("state3.db")).unwrap();
            c.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('kept');").unwrap();
            // Still open (like a running old app) while the copy is made.
            std::fs::write(old.join("settings3.json"), "{}").unwrap();
            std::fs::write(old.join("pairings.json"), "[]").unwrap();
            std::fs::write(old.join("tools").join("yt-dlp.exe"), "x").unwrap();
            let got = migrate_from(&old, &new);
            assert!(got.contains(&"state3.db".to_string()) && got.contains(&"tools/yt-dlp.exe".to_string()), "{got:?}");
        }
        let v: String = rusqlite::Connection::open(new.join("state3.db")).unwrap().query_row("SELECT v FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "kept");
        assert!(new.join("pairings.json").exists() && old.join("state3.db").exists(), "copied, not moved");
        assert!(migrate_from(&old, &new).is_empty(), "only once");
    }
}
