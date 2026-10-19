//! swiftdm-cli: headless downloads with the SwiftHarryDM engine, plus the crash-recovery and
//! performance checks used before releases.
//!
//!   swiftdm-cli get <url> --dir <folder> [--home <folder>] [--connections N]
//!   swiftdm-cli kill-test [--runs N] [--size-mb N] [--conn-mbps N]
//!   swiftdm-cli bench [--size-mb N]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::Rng;
use sha2::{Digest, Sha256};
use swiftdm_core::engine::Engine;
use swiftdm_core::manager::{Manager, ManagerConfig, StartMode};
use swiftdm_core::models::{JobSpec, JobState};
use swiftdm_core::settings::{ProxyMode, Settings, SettingsStore};
use swiftdm_core::store::Store;
use swiftdm_testkit::{Body, Resource, TestServer, MIB};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn num(args: &[String], name: &str, default: u64) -> u64 {
    arg(args, name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn usage() -> ! {
    eprintln!(
        "usage:\n  swiftdm-cli get <url> --dir <folder> [--home <folder>] [--connections N]\n  swiftdm-cli kill-test [--runs N] [--size-mb N] [--conn-mbps N]\n  swiftdm-cli bench [--size-mb N]"
    );
    std::process::exit(2)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("get") => get(&args[1..]).await,
        Some("kill-test") => kill_test(&args[1..]).await,
        Some("bench") => bench(&args[1..]).await,
        _ => usage(),
    };
    std::process::exit(code);
}

// ---------------------------------------------------------------------- get

/// Download one address. Started again with the same --home, it continues the same download.
async fn get(args: &[String]) -> i32 {
    let Some(url) = args.first().filter(|a| !a.starts_with("--")).cloned() else { usage() };
    let Some(dir) = arg(args, "--dir") else { usage() };
    if let Some(home) = arg(args, "--home") {
        std::env::set_var("SWIFTDM_HOME", home);
    }
    let home = swiftdm_core::appdata::app_dir();
    let store = Arc::new(Store::open(home.join("state3.db")).expect("database"));
    let settings = SettingsStore::in_memory(Settings {
        download_dir: dir.clone(),
        max_segments: num(args, "--connections", 8) as u32,
        proxy_mode: ProxyMode::None,
        ..Settings::default()
    });
    let http = Arc::new(swiftdm_core::http::HttpEngine::new(store.clone())) as Arc<dyn Engine>;
    let media = Arc::new(swiftdm_core::media::MediaEngine::new(swiftdm_core::media::Tools::discover())) as Arc<dyn Engine>;
    let engines = HashMap::from([("http".to_string(), http), ("media".to_string(), media)]);
    let mgr = Manager::start(store, engines, settings, ManagerConfig::default()).expect("manager");
    mgr.sync().await;
    let id = match mgr.find_duplicates(&url).into_iter().find(|s| s.state != JobState::Completed) {
        Some(existing) => {
            mgr.resume(&existing.id);
            mgr.retry(&existing.id);
            existing.id
        }
        None => match mgr.submit(JobSpec::new(url.clone(), dir), StartMode::Now) {
            Ok(id) => id,
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        },
    };
    loop {
        let s = mgr.wait_for(&id, &[JobState::Completed, JobState::Failed, JobState::Cancelled], Duration::from_millis(500)).await;
        let Some(s) = s else { continue };
        match s.state {
            JobState::Completed => {
                println!("done {}", s.final_path.unwrap_or_default());
                mgr.shutdown(Duration::from_secs(5)).await;
                return 0;
            }
            JobState::Failed | JobState::Cancelled => {
                eprintln!("failed: {}", s.error.map(|e| e.message).unwrap_or_default());
                return 1;
            }
            _ => {
                if let Some(p) = mgr.progress(&id) {
                    eprint!("\r{:>6.1} MB of {}   ", p.downloaded as f64 / MIB as f64, p.total.map(|t| format!("{:.1} MB", t as f64 / MIB as f64)).unwrap_or("?".into()));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------- helpers

fn sha_file(p: &Path) -> String {
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(p).unwrap();
    std::io::copy(&mut f, &mut h).unwrap();
    hex::encode(h.finalize())
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("swiftdm-{tag}-{}", rand::thread_rng().gen::<u32>()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Peak working set of a child process (bytes), read before its handle is closed.
fn peak_memory(child: &tokio::process::Child) -> Option<u64> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    let h = HANDLE(child.raw_handle()?);
    let mut c = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
    unsafe { GetProcessMemoryInfo(h, &mut c, c.cb).ok()? };
    Some(c.PeakWorkingSetSize as u64)
}

fn me() -> PathBuf {
    std::env::current_exe().unwrap()
}

// ---------------------------------------------------------------------- kill-test

/// Kill the downloading process at random moments, start it again, and check the final file.
async fn kill_test(args: &[String]) -> i32 {
    let runs = num(args, "--runs", 20);
    let size = num(args, "--size-mb", 96) * MIB;
    // Slow enough per connection that downloads take several seconds and get killed mid-way.
    let rate = num(args, "--conn-mbps", 2) * MIB;
    let srv = TestServer::start().await;
    let mut failures = 0;
    let mut kills_total = 0;
    for run in 1..=runs {
        let body = Body::gen(&format!("kill-{run}"), size);
        let expected = body.sha256();
        let url = srv.add(&format!("/k{run}.bin"), Resource::new(body).etag(&format!("\"k{run}\"")).throttle(rate));
        let work = temp_dir("kill");
        let (home, dir) = (work.join("home"), work.join("dl"));
        let mut kills = 0;
        let ok = loop {
            let mut child = tokio::process::Command::new(me())
                .args(["get", &url, "--dir", &dir.to_string_lossy(), "--home", &home.to_string_lossy()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let wait = Duration::from_millis(rand::thread_rng().gen_range(300..2500));
            match tokio::time::timeout(wait, child.wait()).await {
                Ok(status) => break status.map(|s| s.success()).unwrap_or(false),
                Err(_) => {
                    // Hard kill (TerminateProcess): no chance to save anything.
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    kills += 1;
                    if kills > 60 {
                        break false;
                    }
                }
            }
        };
        let file = dir.join(format!("k{run}.bin"));
        let good = ok && file.is_file() && sha_file(&file) == expected;
        let leftovers = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
        println!("run {run:>3}: {:<4} after {kills:>2} kills, {} file(s) in the folder", if good { "ok" } else { "FAIL" }, leftovers);
        if !good || leftovers != 1 {
            failures += 1;
        }
        kills_total += kills;
        let _ = std::fs::remove_dir_all(&work);
    }
    println!("{runs} runs, {kills_total} hard kills, {failures} failure(s)");
    i32::from(failures > 0)
}

// ---------------------------------------------------------------------- bench

/// Throughput and peak memory with 1 and 8 connections against a fast local server.
async fn bench(args: &[String]) -> i32 {
    let size = num(args, "--size-mb", 1024) * MIB;
    let srv = TestServer::start().await;
    let body = Body::gen("bench", size);
    let expected = body.sha256();
    // A per-connection cap makes extra connections matter, as on real servers.
    let url = srv.add("/bench.bin", Resource::new(body).etag("\"b\"").throttle(64 * MIB));
    println!("{:.0} MB file, each connection limited to 64 MB/s by the test server", size as f64 / MIB as f64);
    for conns in [1u64, 8] {
        let work = temp_dir("bench");
        let t0 = Instant::now();
        let mut child = tokio::process::Command::new(me())
            .args(["get", &url, "--dir", &work.join("dl").to_string_lossy(), "--home", &work.join("home").to_string_lossy(), "--connections", &conns.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Poll so the peak can be read while the handle is still open.
        let mut peak = 0;
        let status = loop {
            if let Some(m) = peak_memory(&child) {
                peak = peak.max(m);
            }
            if let Ok(Some(s)) = child.try_wait() {
                break s;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let secs = t0.elapsed().as_secs_f64();
        let ok = status.success() && sha_file(&work.join("dl").join("bench.bin")) == expected;
        println!(
            "{conns} connection(s): {:>6.1} MB/s, {:>5.1} s, peak memory {:>5.1} MB, file {}",
            size as f64 / MIB as f64 / secs,
            secs,
            peak as f64 / MIB as f64,
            if ok { "verified" } else { "WRONG" }
        );
        let _ = std::fs::remove_dir_all(&work);
        if !ok {
            return 1;
        }
    }
    0
}
