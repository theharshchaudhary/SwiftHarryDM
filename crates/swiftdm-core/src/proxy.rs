//! Proxy configuration: Windows system proxy (Internet Settings, including automatic
//! configuration scripts and automatic detection), a PAC script given in Options, none, or a
//! manual proxy. The choice is made per address, like a browser: PAC scripts and bypass
//! lists can send some sites direct and others through a proxy.
use crate::settings::{ProxyMode, Settings};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How the proxy for an address is found.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Direct,
    /// A WinINET-style list ("host:port" or "http=a:1;https=b:2") and a bypass list.
    Static { list: String, bypass: Vec<String> },
    /// Ask WinHTTP, which runs the PAC script (from `pac_url`, or found by automatic
    /// detection). `fallback` is used when the script cannot be run.
    Auto { pac_url: Option<String>, detect: bool, fallback: Box<Source> },
}

/// Chooses the proxy for each request. Cheap to clone; answers are cached per host.
#[derive(Clone)]
pub struct Resolver {
    source: Source,
    cache: Arc<Mutex<HashMap<String, (Instant, Option<String>)>>>,
}

const CACHE_FOR: Duration = Duration::from_secs(300);

impl Resolver {
    pub fn new(settings: &Settings) -> Self {
        let source = match settings.proxy_mode {
            ProxyMode::None => Source::Direct,
            ProxyMode::Manual if settings.proxy_url.trim().is_empty() => Source::Direct,
            ProxyMode::Manual => Source::Static { list: settings.proxy_url.trim().into(), bypass: settings.proxy_bypass.clone() },
            ProxyMode::Pac if settings.proxy_pac_url.trim().is_empty() => Source::Direct,
            ProxyMode::Pac => Source::Auto { pac_url: Some(settings.proxy_pac_url.trim().into()), detect: false, fallback: Box::new(Source::Direct) },
            ProxyMode::System => system_source(),
        };
        Resolver { source, cache: Arc::default() }
    }

    /// No proxy for any address (lets the client skip the per-request hook).
    pub fn is_direct(&self) -> bool {
        self.source == Source::Direct
    }

    /// The proxy URL for this address, or None to connect directly.
    pub fn proxy_for_url(&self, url: &url::Url) -> Option<String> {
        let key = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
        if let Some((at, p)) = self.cache.lock().unwrap().get(&key) {
            if at.elapsed() < CACHE_FOR {
                return p.clone();
            }
        }
        let p = resolve(&self.source, url);
        self.cache.lock().unwrap().insert(key, (Instant::now(), p.clone()));
        p
    }
}

fn resolve(source: &Source, url: &url::Url) -> Option<String> {
    let host = url.host_str().unwrap_or("");
    match source {
        Source::Direct => None,
        Source::Static { list, bypass } => {
            if bypassed(bypass, host) {
                None
            } else {
                pick_for_scheme(list, url.scheme())
            }
        }
        Source::Auto { pac_url, detect, fallback } => match winhttp_proxy_for(url.as_str(), pac_url.as_deref(), *detect) {
            Ok(list) => first_proxy(&list),
            Err(e) => {
                tracing::warn!("proxy script failed for {host}: {e}; using the fallback");
                resolve(fallback, url)
            }
        },
    }
}

/// The Windows proxy settings (what Edge and Chrome use), as a source.
fn system_source() -> Source {
    let c = match ie_config() {
        Some(c) => c,
        None => return Source::Direct,
    };
    let fixed = match &c.proxy {
        Some(list) => Source::Static { list: list.clone(), bypass: split_list(c.bypass.as_deref().unwrap_or("")) },
        None => Source::Direct,
    };
    if c.auto_config_url.is_some() || c.auto_detect {
        Source::Auto { pac_url: c.auto_config_url, detect: c.auto_detect, fallback: Box::new(fixed) }
    } else {
        fixed
    }
}

struct IeConfig {
    auto_detect: bool,
    auto_config_url: Option<String>,
    proxy: Option<String>,
    bypass: Option<String>,
}

fn take_wstr(p: windows::core::PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { p.to_string() }.ok();
    unsafe {
        let _ = windows::Win32::Foundation::GlobalFree(windows::Win32::Foundation::HGLOBAL(p.0 as _));
    }
    s.filter(|s| !s.trim().is_empty())
}

fn ie_config() -> Option<IeConfig> {
    use windows::Win32::Networking::WinHttp::{WinHttpGetIEProxyConfigForCurrentUser, WINHTTP_CURRENT_USER_IE_PROXY_CONFIG};
    let mut c = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut c) }.ok()?;
    Some(IeConfig {
        auto_detect: c.fAutoDetect.as_bool(),
        auto_config_url: take_wstr(c.lpszAutoConfigUrl),
        proxy: take_wstr(c.lpszProxy),
        bypass: take_wstr(c.lpszProxyBypass),
    })
}

/// Run the PAC script for one address with WinHTTP. Returns WinHTTP's proxy list ("" =
/// direct). Blocking; WinHTTP downloads and caches the script itself.
fn winhttp_proxy_for(url: &str, pac_url: Option<&str>, detect: bool) -> Result<String, String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;
    let session = unsafe { WinHttpOpen(&HSTRING::from("SwiftHarryDM"), WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0) };
    if session.is_null() {
        return Err("WinHTTP is not available".into());
    }
    let _ = unsafe { WinHttpSetTimeouts(session, 5000, 5000, 5000, 5000) };
    let pac = pac_url.map(HSTRING::from);
    let mut opts = WINHTTP_AUTOPROXY_OPTIONS::default();
    if let Some(p) = &pac {
        opts.dwFlags |= WINHTTP_AUTOPROXY_CONFIG_URL;
        opts.lpszAutoConfigUrl = PCWSTR(p.as_ptr());
    }
    if detect {
        opts.dwFlags |= WINHTTP_AUTOPROXY_AUTO_DETECT;
        opts.dwAutoDetectFlags = WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A;
    }
    opts.fAutoLogonIfChallenged = true.into();
    let mut info = WINHTTP_PROXY_INFO::default();
    let r = unsafe { WinHttpGetProxyForUrl(session, &HSTRING::from(url), &mut opts, &mut info) };
    let _ = unsafe { WinHttpCloseHandle(session) };
    r.map_err(|e| e.message())?;
    let list = take_wstr(info.lpszProxy).unwrap_or_default();
    let _ = take_wstr(info.lpszProxyBypass);
    Ok(if info.dwAccessType == WINHTTP_ACCESS_TYPE_NO_PROXY { String::new() } else { list })
}

/// The first proxy of a WinHTTP list ("a:1; b:2" or "a:1 b:2"); None for an empty list.
pub fn first_proxy(list: &str) -> Option<String> {
    let first = list.split(|c: char| c == ';' || c.is_whitespace()).map(str::trim).find(|s| !s.is_empty())?;
    Some(with_scheme(first))
}

fn with_scheme(p: &str) -> String {
    if p.contains("://") {
        p.to_string()
    } else {
        format!("http://{p}")
    }
}

/// "host:port" (every protocol) or "http=a:1;https=b:2;socks=c:3": the proxy for a scheme.
/// An https address without an https= entry uses the http= one; a socks= entry is used
/// when there is nothing more specific.
pub fn pick_for_scheme(list: &str, scheme: &str) -> Option<String> {
    let list = list.trim();
    if list.is_empty() {
        return None;
    }
    if !list.contains('=') {
        return Some(with_scheme(list));
    }
    let entries: Vec<(String, String)> = list
        .split(';')
        .filter_map(|e| e.split_once('='))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let get = |k: &str| entries.iter().find(|(e, _)| e == k).map(|(_, v)| v.clone());
    let pick = match scheme {
        "https" => get("https").or_else(|| get("http")),
        s => get(s),
    };
    match pick {
        Some(p) => Some(with_scheme(&p)),
        None => get("socks").map(|p| if p.contains("://") { p } else { format!("socks5://{p}") }),
    }
}

/// Split a bypass list written with semicolons, commas, spaces or new lines.
pub fn split_list(s: &str) -> Vec<String> {
    s.split(|c: char| c == ';' || c == ',' || c.is_whitespace()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Whether a host skips the proxy. Entries are host patterns with * ("*.corp.example",
/// "10.*"), plain names (cover subdomains), or "<local>" for names without a dot.
pub fn bypassed(bypass: &[String], host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']').to_lowercase();
    bypass.iter().any(|b| {
        let b = b.trim().to_lowercase();
        if b == "<local>" {
            return !host.contains('.') && !host.contains(':');
        }
        let b = b.split("://").last().unwrap_or(&b).to_string();
        crate::wildcard::host_matches(&b, &host)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_proxy_for_each_protocol() {
        assert_eq!(pick_for_scheme("proxy:8080", "https").as_deref(), Some("http://proxy:8080"));
        assert_eq!(pick_for_scheme("http=a:1;https=b:2", "http").as_deref(), Some("http://a:1"));
        assert_eq!(pick_for_scheme("http=a:1;https=b:2", "https").as_deref(), Some("http://b:2"));
        assert_eq!(pick_for_scheme("http=a:1;ftp=c:3", "https").as_deref(), Some("http://a:1"));
        assert_eq!(pick_for_scheme("ftp=c:3", "http"), None);
        assert_eq!(pick_for_scheme("ftp=c:3;socks=s:1080", "http").as_deref(), Some("socks5://s:1080"));
        assert_eq!(pick_for_scheme("socks5://u:p@s:1080", "https").as_deref(), Some("socks5://u:p@s:1080"));
        assert_eq!(pick_for_scheme("", "http"), None);
    }

    #[test]
    fn bypass_lists() {
        let b = split_list("<local>; *.corp.example, 10.* intranet.example");
        assert!(bypassed(&b, "printer"));
        assert!(bypassed(&b, "wiki.corp.example"));
        assert!(!bypassed(&b, "corp.example.com"));
        assert!(bypassed(&b, "10.1.2.3"));
        assert!(!bypassed(&b, "110.1.2.3"));
        assert!(bypassed(&b, "files.intranet.example"));
        assert!(!bypassed(&b, "example.com"));
    }

    #[test]
    fn winhttp_lists() {
        assert_eq!(first_proxy("a:1; b:2").as_deref(), Some("http://a:1"));
        assert_eq!(first_proxy("  a:1 b:2").as_deref(), Some("http://a:1"));
        assert_eq!(first_proxy(""), None);
    }

    #[test]
    fn manual_proxy_and_bypass() {
        let s = Settings {
            proxy_mode: ProxyMode::Manual,
            proxy_url: "http=h:8080;https=s:8443".into(),
            proxy_bypass: vec!["*.local.test".into()],
            ..Settings::default()
        };
        let r = Resolver::new(&s);
        let u = |x: &str| url::Url::parse(x).unwrap();
        assert_eq!(r.proxy_for_url(&u("http://a.example/f")).as_deref(), Some("http://h:8080"));
        assert_eq!(r.proxy_for_url(&u("https://a.example/f")).as_deref(), Some("http://s:8443"));
        assert_eq!(r.proxy_for_url(&u("https://nas.local.test/f")), None);
        assert!(Resolver::new(&Settings { proxy_mode: ProxyMode::None, ..Settings::default() }).is_direct());
    }
}
