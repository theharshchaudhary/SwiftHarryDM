//! Windows integration: opening files, power actions, sounds.
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Open a file with its default program (or a folder in Explorer).
pub fn open_path(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Err(format!("{path} no longer exists."));
    }
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // ShellExecute returns a value greater than 32 on success.
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("Windows could not open {path}."))
    }
}

/// Show the file selected in an Explorer window (or the folder if the file is gone).
pub fn reveal(path: &str) -> Result<(), String> {
    if Path::new(path).is_file() {
        Command::new("explorer.exe").raw_arg(format!("/select,\"{path}\"")).spawn().map(|_| ()).map_err(|e| e.to_string())
    } else {
        let dir = if Path::new(path).is_dir() { Path::new(path) } else { Path::new(path).parent().unwrap_or(Path::new(path)) };
        open_path(&dir.to_string_lossy())
    }
}

/// The Windows "Open with" chooser.
pub fn open_with(path: &str) -> Result<(), String> {
    if !Path::new(path).is_file() {
        return Err(format!("{path} no longer exists."));
    }
    Command::new("rundll32.exe")
        .raw_arg(format!("shell32.dll,OpenAs_RunDLL {path}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Running with administrator rights (Windows then blocks drag and drop from normal programs).
pub fn is_elevated() -> bool {
    unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
}

/// The sound for an event: the user's .wav file, or the Windows sound that fits.
pub fn play_sound(event: &str, file: &str) {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    let file = file.trim();
    unsafe {
        if !file.is_empty() && Path::new(file).is_file() {
            let _ = PlaySoundW(&HSTRING::from(file), None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
            return;
        }
        let alias = match event {
            "failed" => "SystemHand",
            "queue" => "SystemExclamation",
            _ => "SystemAsterisk",
        };
        let _ = PlaySoundW(&HSTRING::from(alias), None, SND_ALIAS | SND_ASYNC);
    }
}

/// Turn the computer off. `force` closes programs that would otherwise hold up the shutdown
/// ("force processes to close"); they may lose unsaved work.
pub fn shutdown(force: bool) -> Result<(), String> {
    let mut args = vec!["/s", "/t", "0"];
    if force {
        args.push("/f");
    }
    Command::new("shutdown.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn suspend(hibernate: bool) -> Result<(), String> {
    let ok = unsafe { SetSuspendState(hibernate, false, false) };
    if ok.as_bool() {
        Ok(())
    } else {
        Err(if hibernate { "Windows refused to hibernate (it may be turned off)." } else { "Windows refused to sleep." }.into())
    }
}

/// The tray icon with a progress bar along its bottom edge (`rgba` is the plain icon).
pub fn progress_icon(rgba: &[u8], w: u32, h: u32, pct: u8) -> Vec<u8> {
    let mut out = rgba.to_vec();
    let bar = (h / 6).max(2);
    let filled = (w as u64 * pct.min(100) as u64 / 100) as u32;
    for y in h - bar..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let px: [u8; 4] = if x < filled { [255, 255, 255, 255] } else { [23, 11, 51, 235] };
            out[i..i + 4].copy_from_slice(&px);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn progress_bar_on_the_icon() {
        let (w, h) = (32u32, 32u32);
        let plain = vec![10u8; (w * h * 4) as usize];
        let half = super::progress_icon(&plain, w, h, 50);
        let at = |x: u32, y: u32| half[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize].to_vec();
        assert_eq!(at(0, 31), [255, 255, 255, 255], "filled part");
        assert_eq!(at(31, 31), [23, 11, 51, 235], "track");
        assert_eq!(at(16, 0), [10, 10, 10, 10], "the rest of the icon is untouched");
        assert_eq!(super::progress_icon(&plain, w, h, 0)[((31 * w) * 4) as usize], 23);
    }
}
