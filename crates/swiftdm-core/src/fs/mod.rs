pub mod collisions;
pub mod names;

use std::path::{Path, PathBuf};

/// Prefix long Windows paths with \\?\ so paths over 260 characters still work.
pub fn long_path(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map(|c| c.join(p)).unwrap_or_else(|_| p.to_path_buf())
    };
    let s = abs.to_string_lossy().replace('/', "\\");
    if s.len() < 248 || s.starts_with(r"\\?\") {
        return PathBuf::from(s);
    }
    if let Some(unc) = s.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"\\?\UNC\{unc}"));
    }
    PathBuf::from(format!(r"\\?\{s}"))
}

/// Free bytes on the volume holding `dir`.
pub fn free_space(dir: &Path) -> std::io::Result<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut free = 0u64;
    let path = HSTRING::from(long_path(dir).as_os_str());
    unsafe { GetDiskFreeSpaceExW(&path, Some(&mut free), None, None) }
        .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))?;
    Ok(free)
}
