//! License rules against a local mock license server (same rules as v2).
use std::sync::{Arc, Mutex};

use axum::routing::post;
use axum::{Json, Router};
use chrono::NaiveDateTime;
use serde_json::{json, Value};
use swiftdm_core::licensing::LicenseService;

#[derive(Clone, Default)]
struct Mock {
    verify: Arc<Mutex<Value>>,
    trial: Arc<Mutex<Value>>,
}

async fn server(m: Mock) -> String {
    let (v, t) = (m.verify.clone(), m.trial.clone());
    let app = Router::new()
        .route("/verify", post(move || { let v = v.clone(); async move { Json(v.lock().unwrap().clone()) } }))
        .route("/trial", post(move || { let t = t.clone(); async move { Json(t.lock().unwrap().clone()) } }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

fn at(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
}

fn svc(dir: &tempfile::TempDir, server: &str) -> LicenseService {
    LicenseService::with(server, dir.path().join("token.json"), dir.path().join("machine"), "3.0.0")
}

const KEY: &str = "ABCD-EFGH-IJKL-MNOP-QRST-UVWX";

#[tokio::test]
async fn trial_then_license_and_offline_grace() {
    let dir = tempfile::tempdir().unwrap();
    let m = Mock::default();
    let base = server(m.clone()).await;
    *m.trial.lock().unwrap() = json!({ "valid": true, "days_left": 7, "trial_started": "x" });
    let s = svc(&dir, &base);

    // First start: a trial begins and is stored.
    let st = s.check().await;
    assert!(st.allowed && st.action.as_deref() == Some("trial_started"), "{st:?}");
    assert!(s.load_token().unwrap().contains_key("trial_start"));
    assert_eq!(s.machine_id().len(), 64);

    // Trial over on the server.
    *m.trial.lock().unwrap() = json!({ "valid": false });
    let st = s.check().await;
    assert!(!st.allowed && st.action.as_deref() == Some("ask_key"));

    // Activation: bad format, rejected key, then a good key.
    assert_eq!(s.activate("short").await.text, "License: Invalid format");
    *m.verify.lock().unwrap() = json!({ "valid": false, "type": "invalid" });
    assert!(!s.activate(KEY).await.allowed);
    *m.verify.lock().unwrap() = json!({ "valid": true, "type": "lifetime" });
    let st = s.activate(KEY).await;
    assert!(st.allowed, "{st:?}");
    assert_eq!(s.check().await.text, "License: Active ✓");
    assert!(!s.info().to_string().contains("EFGH"), "the key is masked in the information window");

    // Server unreachable: allowed for 60 days after the last successful check.
    let mut off = svc(&dir, "http://127.0.0.1:9");
    let last = s.load_token().unwrap()["last_verified"].as_str().unwrap().to_string();
    let verified = NaiveDateTime::parse_from_str(&last, "%Y-%m-%dT%H:%M:%S%.f").unwrap();
    fn plus_10_days() -> NaiveDateTime {
        chrono::Local::now().naive_local() + chrono::Duration::days(10)
    }
    fn plus_61_days() -> NaiveDateTime {
        chrono::Local::now().naive_local() + chrono::Duration::days(61)
    }
    off.now = plus_10_days;
    let st = off.check().await;
    assert!(st.allowed && st.text.contains("Offline"), "{st:?} (verified {verified})");
    off.now = plus_61_days;
    let st = off.check().await;
    assert!(!st.allowed && st.action.as_deref() == Some("ask_key"), "{st:?}");

    // Key used on another computer.
    *m.verify.lock().unwrap() = json!({ "valid": false, "type": "machine_mismatch" });
    assert_eq!(s.check().await.action.as_deref(), Some("machine_mismatch"));
}

#[tokio::test]
async fn hardware_change_and_offline_trial() {
    let dir = tempfile::tempdir().unwrap();
    let s = svc(&dir, "http://127.0.0.1:9");
    // No server and no token: the check fails closed with an explanation.
    let st = s.check().await;
    assert!(!st.allowed && st.action.as_deref() == Some("error"));
    // A trial started two days ago keeps working offline.
    fn day2() -> NaiveDateTime {
        at("2026-01-03T10:00:00")
    }
    let mut s2 = svc(&dir, "http://127.0.0.1:9");
    s2.now = day2;
    std::fs::write(dir.path().join("token.json"), json!({ "trial_start": "2026-01-01T09:00:00", "machine_id": s2.machine_id() }).to_string()).unwrap();
    let st = s2.check().await;
    assert!(st.allowed && st.text.contains("Offline"), "{st:?}");
    // The token belongs to a different machine.
    std::fs::write(dir.path().join("token.json"), json!({ "trial_start": "2026-01-01T09:00:00", "machine_id": "other" }).to_string()).unwrap();
    assert_eq!(s2.check().await.action.as_deref(), Some("hardware_changed"));
}
