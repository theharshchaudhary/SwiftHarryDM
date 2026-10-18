//! Black-box observation server: serves fixed scenarios on 127.0.0.1:<port> and keeps writing
//! every connection/request it receives to a JSON log, so any downloader (ours or another
//! product) can be measured from the outside without inspecting its code.
//! Usage: sdm-observe <port> <out_dir>
use std::time::Duration;
use swiftdm_testkit::{Body, Fault, Resource, TestServer, MIB};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args.get(1).and_then(|p| p.parse().ok()).unwrap_or(18080);
    let out = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&out).unwrap();
    let srv = TestServer::start_on(port).await;
    let mut expected = serde_json::Map::new();
    let mut add = |path: &str, res: Resource, alt: Option<&Body>| {
        let mut v = serde_json::json!({ "url": srv.add(path, res.clone()), "sha256": res.body.sha256(), "size": res.body.size() });
        if let Some(a) = alt {
            v["alt_sha256"] = a.sha256().into();
        }
        expected.insert(path.to_string(), v);
    };
    add("/s1/big64.bin", Resource::new(Body::gen("s1", 64 * MIB)).etag("\"s1\"").throttle(2 * MIB), None);
    add("/s2/norange.bin", Resource::new(Body::gen("s2", 16 * MIB)).no_ranges().throttle(4 * MIB), None);
    add(
        "/s3/cut.bin",
        Resource::new(Body::gen("s3", 24 * MIB)).etag("\"s3\"").throttle(2 * MIB).fault(Fault::cut(3 * MIB).when(|n, _| n == 1)),
        None,
    );
    add(
        "/s4/stall.bin",
        Resource::new(Body::gen("s4", 32 * MIB))
            .etag("\"s4\"")
            .throttle(2 * MIB)
            .fault(Fault::stall(MIB, Duration::from_secs(25)).when(|_, r| r.is_none_or(|r| r.starts_with("bytes=0-")))),
        None,
    );
    let v2 = Body::gen("s5-v2", 24 * MIB);
    let mut s5 = Resource::new(Body::gen("s5-v1", 24 * MIB)).etag("\"v1\"").throttle(MIB);
    s5.replace_after = Some(4);
    s5.replacement = Some(Box::new(Resource::new(v2.clone()).etag("\"v2\"").throttle(MIB)));
    add("/s5/change.bin", s5, Some(&v2));
    add(
        "/s6/get",
        Resource::new(Body::gen("s6", 300_000))
            .content_type("application/pdf")
            .disposition("attachment; filename=\"report 2026.pdf\""),
        None,
    );
    add("/s7/flaky.bin", Resource::new(Body::gen("s7", 4 * MIB)).etag("\"s7\"").statuses(&[503, 503]), None);
    std::fs::write(out.join("expected.json"), serde_json::to_string_pretty(&expected).unwrap()).unwrap();
    println!("ready {}", srv.base);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let log = serde_json::json!({ "connections": srv.connections(), "requests": srv.all_requests() });
        let _ = std::fs::write(out.join("log.json.tmp"), serde_json::to_vec(&log).unwrap());
        let _ = std::fs::rename(out.join("log.json.tmp"), out.join("log.json"));
    }
}
