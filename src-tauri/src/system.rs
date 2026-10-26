//! Windows integration: opening files, power actions, sounds.
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONASTERISK, SW_SHOWNORMAL};

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

pub fn beep() {
    unsafe {
        let _ = MessageBeep(MB_ICONASTERISK);
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
