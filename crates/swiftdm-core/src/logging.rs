//! Logging with secret redaction: URL query strings, userinfo, bearer tokens, cookies,
//! passwords and license keys never reach the log file.
use regex::Regex;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

fn url_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?i)\b(?:https?|ftp)://[^\s"'<>]+"#).unwrap())
}
fn bearer_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)(Bearer\s+)[A-Za-z0-9._~+/=-]+").unwrap())
}
fn kv_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?i)((?:token|license_key|password|cookie|authorization)"?\s*[:=]\s*"?)[^\s",}]+"#).unwrap()
    })
}

pub fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(u) => {
            let mut out = format!("{}://{}", u.scheme(), u.host_str().unwrap_or(""));
            if let Some(p) = u.port() {
                out.push_str(&format!(":{p}"));
            }
            out.push_str(u.path());
            if u.query().is_some() {
                out.push_str("?<redacted>");
            }
            out
        }
        Err(_) => "<invalid-url>".into(),
    }
}

pub fn redact(text: &str) -> String {
    let t = url_re().replace_all(text, |c: &regex::Captures| redact_url(&c[0]));
    let t = bearer_re().replace_all(&t, "${1}<redacted>");
    kv_re().replace_all(&t, "${1}<redacted>").into_owned()
}

/// A writer for tracing-subscriber that redacts every formatted line.
pub struct RedactingWriter {
    file: Mutex<std::fs::File>,
}

impl RedactingWriter {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > 4_000_000 {
                let _ = std::fs::rename(path, path.with_extension("log.1"));
            }
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file: Mutex::new(file) })
    }
}

impl Write for &RedactingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let line = redact(&String::from_utf8_lossy(buf));
        let mut f = self.file.lock().unwrap();
        f.write_all(line.as_bytes())?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.lock().unwrap().flush()
    }
}

pub fn init(log_dir: &Path) {
    use tracing_subscriber::fmt::MakeWriter;
    struct Make(&'static RedactingWriter);
    impl<'a> MakeWriter<'a> for Make {
        type Writer = &'static RedactingWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.0
        }
    }
    let _ = std::fs::create_dir_all(log_dir);
    let Ok(w) = RedactingWriter::open(&log_dir.join("swiftdm.log")) else { return };
    let w: &'static RedactingWriter = Box::leak(Box::new(w));
    let _ = tracing_subscriber::fmt()
        .with_writer(Make(w))
        .with_ansi(false)
        .with_thread_names(true)
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("SWIFTDM_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_urls_and_secrets() {
        assert_eq!(
            redact_url("https://user:pw@cdn.example.com:8443/v/a.mp4?sig=SECRET#t"),
            "https://cdn.example.com:8443/v/a.mp4?<redacted>"
        );
        let out = redact(r#"GET https://x.example/a?token=abc Authorization: Bearer abc.def license_key="K3Y" cookie=zzz"#);
        for s in ["abc.def", "K3Y", "zzz", "token=abc"] {
            assert!(!out.contains(s), "{out}");
        }
    }
}
