//! Destination-name reservation and no-clobber finalization. A name is reserved for a job as
//! soon as it is known, so concurrent jobs never pick the same file; the final move fails
//! instead of overwriting, and the next free "name (N).ext" is chosen.
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::long_path;
use super::names::split_ext;

pub fn candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = split_ext(name);
    std::iter::once(name.to_string()).chain((1..10000).map(move |i| format!("{stem} ({i}){ext}")))
}

fn key(dest: &str, name: &str) -> (String, String) {
    let abs = std::path::absolute(dest).unwrap_or_else(|_| PathBuf::from(dest));
    (abs.to_string_lossy().replace('/', "\\").to_lowercase(), name.to_lowercase())
}

#[derive(Default)]
pub struct Reservations {
    inner: Mutex<(HashMap<(String, String), String>, HashMap<String, (String, String)>)>,
}

impl Reservations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reserve(&self, job_id: &str, dest: &str, desired: &str, overwrite: bool) -> io::Result<String> {
        let mut g = self.inner.lock().unwrap();
        for cand in candidates(desired) {
            let k = key(dest, &cand);
            if let Some(owner) = g.0.get(&k) {
                if owner != job_id {
                    continue;
                }
            }
            // Checked even for the job's own reservation: a file may appear after reserving.
            if !overwrite && long_path(Path::new(dest).join(&cand)).symlink_metadata().is_ok() {
                continue;
            }
            if let Some(old) = g.1.remove(job_id) {
                if g.0.get(&old).map(String::as_str) == Some(job_id) {
                    g.0.remove(&old);
                }
            }
            g.0.insert(k.clone(), job_id.to_string());
            g.1.insert(job_id.to_string(), k);
            return Ok(cand);
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("no free name for {desired:?}")))
    }

    /// Re-register a persisted reservation at startup. False if someone else holds it.
    pub fn adopt(&self, job_id: &str, dest: &str, name: &str) -> bool {
        let mut g = self.inner.lock().unwrap();
        let k = key(dest, name);
        if g.0.get(&k).is_some_and(|o| o != job_id) {
            return false;
        }
        if let Some(old) = g.1.remove(job_id) {
            g.0.remove(&old);
        }
        g.0.insert(k.clone(), job_id.to_string());
        g.1.insert(job_id.to_string(), k);
        true
    }

    pub fn release(&self, job_id: &str) {
        let mut g = self.inner.lock().unwrap();
        if let Some(old) = g.1.remove(job_id) {
            if g.0.get(&old).map(String::as_str) == Some(job_id) {
                g.0.remove(&old);
            }
        }
    }
}

/// Rename that fails if `dst` exists (MoveFileExW without MOVEFILE_REPLACE_EXISTING).
pub fn rename_noclobber(src: &Path, dst: &Path) -> io::Result<()> {
    if long_path(dst).symlink_metadata().is_ok() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let s = HSTRING::from(long_path(src).as_os_str());
    let d = HSTRING::from(long_path(dst).as_os_str());
    unsafe { MoveFileExW(&s, &d, MOVEFILE_WRITE_THROUGH) }.map_err(|e| {
        let code = (e.code().0 & 0xFFFF) as i32;
        if code == 183 || code == 80 {
            io::Error::from(io::ErrorKind::AlreadyExists)
        } else {
            io::Error::from_raw_os_error(code)
        }
    })
}

/// Move a file or folder, also to another drive (copy, then delete the original). Fails if
/// `dst` exists.
pub fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    match rename_noclobber(src, dst) {
        Err(e) if e.raw_os_error() == Some(17) => {
            // ERROR_NOT_SAME_DEVICE: another drive.
            copy_all(src, dst)?;
            if long_path(src).is_dir() {
                std::fs::remove_dir_all(long_path(src))
            } else {
                std::fs::remove_file(long_path(src))
            }
        }
        r => r,
    }
}

fn copy_all(src: &Path, dst: &Path) -> io::Result<()> {
    if long_path(src).is_dir() {
        std::fs::create_dir_all(long_path(dst))?;
        for e in std::fs::read_dir(long_path(src))? {
            let e = e?;
            copy_all(&src.join(e.file_name()), &dst.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(long_path(src), long_path(dst)).map(|_| ())
    }
}

/// Move a finished temp artifact to its final name and return the final path.
pub fn finalize_move(
    src: &Path,
    dest: &str,
    desired: &str,
    job_id: &str,
    res: &Reservations,
    overwrite: bool,
) -> io::Result<PathBuf> {
    std::fs::create_dir_all(long_path(dest))?;
    let mut name = res.reserve(job_id, dest, desired, overwrite)?;
    for _ in 0..10000 {
        let dst = Path::new(dest).join(&name);
        let r = if overwrite {
            if long_path(&dst).is_dir() {
                return Err(io::Error::new(io::ErrorKind::IsADirectory, dst.display().to_string()));
            }
            std::fs::rename(long_path(src), long_path(&dst))
        } else {
            rename_noclobber(src, &dst)
        };
        match r {
            Ok(()) => return Ok(dst),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // Someone created the file after we reserved the name: pick the next free one.
                name = res.reserve(job_id, dest, desired, false)?;
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn write(p: &Path, data: &[u8]) {
        std::fs::write(long_path(p), data).unwrap();
    }

    #[test]
    fn unique_and_case_insensitive() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().to_str().unwrap();
        let r = Reservations::new();
        assert_eq!(r.reserve("j1", dest, "File.bin", false).unwrap(), "File.bin");
        assert_eq!(r.reserve("j2", dest, "file.BIN", false).unwrap(), "file (1).BIN");
        assert_eq!(r.reserve("j3", dest, "file.bin", false).unwrap(), "file (2).bin");
        assert_eq!(r.reserve("j1", dest, "File.bin", false).unwrap(), "File.bin");
        r.release("j2");
        assert_eq!(r.reserve("j4", dest, "file.bin", false).unwrap(), "file (1).bin");
    }

    #[test]
    fn skips_existing_files() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("a.zip"), b"x");
        write(&d.path().join("a (1).zip"), b"x");
        assert_eq!(Reservations::new().reserve("j", d.path().to_str().unwrap(), "a.zip", false).unwrap(), "a (2).zip");
    }

    #[test]
    fn parallel_reservations_never_collide() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().to_str().unwrap().to_string();
        let r = Arc::new(Reservations::new());
        let hs: Vec<_> = (0..50)
            .map(|i| {
                let r = r.clone();
                let dest = dest.clone();
                std::thread::spawn(move || r.reserve(&format!("j{i}"), &dest, "same.bin", false).unwrap().to_lowercase())
            })
            .collect();
        let mut names: Vec<String> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 50);
    }

    #[test]
    fn finalize_race_and_readonly() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().to_str().unwrap();
        let r = Reservations::new();
        let src = d.path().join(".tmp.part");
        write(&src, b"new");
        r.reserve("j", dest, "out.bin", false).unwrap();
        write(&d.path().join("out.bin"), b"other process");
        let f = finalize_move(&src, dest, "out.bin", "j", &r, false).unwrap();
        assert!(f.ends_with("out (1).bin"));
        assert_eq!(std::fs::read(d.path().join("out.bin")).unwrap(), b"other process");
        let ro = d.path().join("ro.bin");
        write(&ro, b"keep");
        let mut perm = std::fs::metadata(&ro).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&ro, perm).unwrap();
        let src2 = d.path().join(".t2");
        write(&src2, b"new");
        let f2 = finalize_move(&src2, dest, "ro.bin", "k", &r, false).unwrap();
        assert!(f2.ends_with("ro (1).bin"));
        assert_eq!(std::fs::read(&ro).unwrap(), b"keep");
    }

    #[test]
    fn overwrite_policy() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join(".t");
        write(&src, b"new");
        write(&d.path().join("o.bin"), b"old");
        finalize_move(&src, d.path().to_str().unwrap(), "o.bin", "j", &Reservations::new(), true).unwrap();
        assert_eq!(std::fs::read(d.path().join("o.bin")).unwrap(), b"new");
    }

    #[test]
    fn long_paths_work() {
        let d = tempfile::tempdir().unwrap();
        let mut deep = d.path().to_path_buf();
        for i in 0..12 {
            deep = deep.join(format!("{}{i}", "d".repeat(25)));
        }
        std::fs::create_dir_all(long_path(&deep)).unwrap();
        let src = deep.join(".t");
        write(&src, b"ok");
        let f = finalize_move(&src, deep.to_str().unwrap(), &format!("{}.bin", "f".repeat(60)), "j", &Reservations::new(), false)
            .unwrap();
        assert!(f.to_string_lossy().len() > 300);
        assert_eq!(std::fs::read(long_path(&f)).unwrap(), b"ok");
    }
}
