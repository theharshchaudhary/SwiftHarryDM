// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // A browser starts the app as its native messaging host with the extension's origin.
    if let Some(origin) = std::env::args().nth(1).filter(|a| a.starts_with("chrome-extension://")) {
        std::process::exit(swiftharrydm_lib::native_host(&origin));
    }
    swiftharrydm_lib::run();
}
