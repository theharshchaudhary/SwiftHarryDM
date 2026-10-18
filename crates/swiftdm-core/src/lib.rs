//! SwiftHarryDM core: download engine, persistence, scheduling and the local extension API.
//! Everything except the user interface lives here so it can be tested headlessly.

pub mod api;
pub mod appdata;
pub mod batch;
pub mod cancel;
pub mod capture;
pub mod categories;
pub mod engine;
pub mod errors;
pub mod events;
pub mod fs;
pub mod http;
pub mod licensing;
pub mod limiter;
pub mod logging;
pub mod manager;
pub mod media;
pub mod models;
pub mod proxy;
pub mod quota;
pub mod router;
pub mod scheduler;
pub mod search;
pub mod secrets;
pub mod settings;
pub mod states;
pub mod store;
pub mod wildcard;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
