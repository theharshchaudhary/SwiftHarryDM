//! Stand-in for yt-dlp.exe used by the media engine tests. It understands the arguments the
//! engine passes and behaves according to the URL path:
//!   /ok        a 2 MB "video" (format 137) + "audio" (140), merged
//!   /slow      like /ok but slow, so pause and resume can be tested (resumes from .part)
//!   /private   fails like a private video
//!   /drm       reports a DRM-protected format
//!   /playlist  reports a playlist
//!   /live      reports a live stream
//!   /flaky     fails with HTTP 503 on the first run in a temp folder, then succeeds
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

fn arg_after<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.windows(2).filter(|w| w[0] == flag).map(|w| w[1].as_str()).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("2026.01.01-fake");
        return;
    }
    let url = args.last().cloned().unwrap_or_default();
    let path = url.split("://").nth(1).and_then(|r| r.split_once('/')).map(|(_, p)| format!("/{p}")).unwrap_or_default();
    let probe = args.iter().any(|a| a == "-J");
    match path.as_str() {
        "/private" => fail("ERROR: [fake] abc: Private video. Sign in if you've been granted access"),
        // The proxy chosen in Options must reach yt-dlp.
        "/needs-proxy" if arg_after(&args, "--proxy") != ["http://px.test:3128"] => {
            fail(&format!("ERROR: [fake] expected --proxy http://px.test:3128, got {:?}", arg_after(&args, "--proxy")))
        }
        "/playlist" if probe => {
            println!(r#"{{"_type":"playlist","id":"pl","title":"A playlist","entries":[]}}"#);
            return;
        }
        "/live" if probe => {
            println!(r#"{{"id":"lv","title":"Live","is_live":true,"live_status":"is_live","format_id":"1"}}"#);
            return;
        }
        _ => {}
    }
    if probe {
        let drm = path == "/drm";
        println!(
            r#"{{"id":"vid1","title":"Fake Video","extractor_key":"Fake","duration":10.0,"format_id":"137+140","requested_formats":[
                {{"format_id":"137","ext":"mp4","height":720,"vcodec":"avc1","acodec":"none","filesize":1500000,"has_drm":{drm}}},
                {{"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a","filesize":500000}}],
               "formats":[{{"format_id":"137","ext":"mp4","height":720,"vcodec":"avc1","acodec":"none","filesize":1500000}},
                          {{"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a","filesize":500000}},
                          {{"format_id":"18","ext":"mp4","height":360,"vcodec":"avc1","acodec":"mp4a","filesize":900000}}]}}"#
        );
        return;
    }
    // Download mode.
    let home = arg_after(&args, "--paths").into_iter().find_map(|p| p.strip_prefix("home:")).map(PathBuf::from).expect("--paths home:");
    std::fs::create_dir_all(&home).unwrap();
    if path == "/flaky" {
        let marker = home.join("flaky.marker");
        if !marker.exists() {
            std::fs::write(&marker, b"1").unwrap();
            fail("ERROR: unable to download video data: HTTP Error 503: Service Unavailable");
        }
    }
    let slow = path == "/slow";
    let mut out = std::io::stdout();
    for (fid, size) in [("137", 1_500_000u64), ("140", 500_000u64)] {
        let part = home.join(format!("Fake Video.f{fid}.part"));
        let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&part).unwrap();
        while have < size {
            let n = (size - have).min(100_000);
            f.write_all(&vec![fid.as_bytes()[0]; n as usize]).unwrap();
            f.flush().unwrap();
            have += n;
            writeln!(out, "SDMP {fid}|downloading|{have}|{size}|NA|1000000|1").unwrap();
            out.flush().unwrap();
            if slow {
                std::thread::sleep(Duration::from_millis(150));
            }
        }
        writeln!(out, "SDMP {fid}|finished|{size}|{size}|NA|NA|NA").unwrap();
        std::fs::rename(&part, home.join(format!("Fake Video.f{fid}.done"))).unwrap();
    }
    writeln!(out, "[Merger] Merging formats into \"Fake Video.mp4\"").unwrap();
    let final_path = home.join("Fake Video.mp4");
    let mut merged = std::fs::read(home.join("Fake Video.f137.done")).unwrap();
    merged.extend(std::fs::read(home.join("Fake Video.f140.done")).unwrap());
    std::fs::write(&final_path, merged).unwrap();
    for fid in ["137", "140"] {
        let _ = std::fs::remove_file(home.join(format!("Fake Video.f{fid}.done")));
    }
    writeln!(out, "SDMF {}", serde_json::to_string(&final_path.to_string_lossy()).unwrap()).unwrap();
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}
