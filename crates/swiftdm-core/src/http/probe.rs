//! HTTP request layer: client construction (proxy, user agent, TLS verification on),
//! manual redirect policy, cookie/credential scoping, error mapping and the initial probe.
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, COOKIE, LOCATION, RANGE, REFERER, USER_AGENT};
use reqwest::{Client, Response, StatusCode};
use std::time::Duration;

use super::validators::parse_content_range;
use crate::cancel::{CancelToken, StopReason};
use crate::engine::EngineError;
use crate::errors::{ErrorCode, SdmError};
use crate::settings::Settings;

pub const MAX_REDIRECTS: usize = 10;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

pub fn build_client(settings: &Settings) -> Result<Client, SdmError> {
    let mut b = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .user_agent(settings.user_agent.clone())
        .pool_max_idle_per_host(32)
        .tcp_nodelay(true);
    b = match crate::proxy::proxy_for(settings) {
        Some(p) => b.proxy(
            reqwest::Proxy::all(&p)
                .map_err(|e| SdmError::new(ErrorCode::Network, "The proxy address is not valid.").detail(e.to_string()))?,
        ),
        None => b.no_proxy(),
    };
    b.build().map_err(|e| SdmError::new(ErrorCode::Internal, "Could not start the HTTP client.").detail(e.to_string()))
}

pub fn next_redirect(current: &str, location: Option<&str>) -> Result<String, SdmError> {
    let loc = location.ok_or_else(|| SdmError::new(ErrorCode::HttpStatus, "The server sent a redirect without a destination."))?;
    let base = url::Url::parse(current).map_err(|_| SdmError::new(ErrorCode::Network, "The address is not valid."))?;
    let next = base.join(loc).map_err(|_| SdmError::new(ErrorCode::HttpStatus, "The server sent an invalid redirect."))?;
    match next.scheme() {
        "http" | "https" => {}
        s => return Err(SdmError::new(ErrorCode::RedirectRefused, format!("Refused a redirect to an unsupported address ({s}:)."))),
    }
    if base.scheme() == "https" && next.scheme() == "http" {
        return Err(SdmError::new(
            ErrorCode::RedirectRefused,
            "Refused a redirect from a secure (https) to an insecure (http) address.",
        ));
    }
    Ok(next.to_string())
}

fn retry_after(h: &HeaderMap) -> Option<f64> {
    let v = h.get("retry-after")?.to_str().ok()?.trim();
    if let Ok(s) = v.parse::<f64>() {
        return Some(s.clamp(0.0, 600.0));
    }
    let t = chrono::DateTime::parse_from_rfc2822(v).ok()?;
    Some(((t.timestamp() as f64) - crate::now()).clamp(0.0, 600.0))
}

pub fn status_error(resp: &Response) -> SdmError {
    let code = resp.status().as_u16();
    let ra = retry_after(resp.headers());
    match code {
        401 | 403 => SdmError::new(
            ErrorCode::Forbidden,
            format!("The server denied access ({code}). The link may require a login or may have expired. Use \"Refresh download address\" with a new link."),
        )
        .status(code),
        404 | 410 => SdmError::new(ErrorCode::HttpStatus, format!("The file was not found on the server ({code}).")).status(code),
        429 => SdmError::new(ErrorCode::RateLimited, "The server is limiting requests (429). Retrying later.")
            .retryable()
            .status(code)
            .retry_after(ra),
        408 | 500..=599 => SdmError::new(ErrorCode::HttpStatus, format!("The server had a temporary problem ({code})."))
            .retryable()
            .status(code)
            .retry_after(ra),
        _ => SdmError::new(ErrorCode::HttpStatus, format!("The server returned an unexpected response ({code}).")).status(code),
    }
}

pub fn map_reqwest(e: &reqwest::Error) -> SdmError {
    let text = format!("{e:?}");
    if text.contains("certificate") || text.contains("InvalidCertificate") || text.contains("UnknownIssuer") {
        return SdmError::new(ErrorCode::Tls, "The secure connection could not be verified (certificate problem).").detail(text);
    }
    if e.is_timeout() {
        return SdmError::new(ErrorCode::Timeout, "The server stopped responding.").retryable().detail(text);
    }
    if e.is_builder() {
        return SdmError::new(ErrorCode::Network, "The address is not valid.").detail(text);
    }
    SdmError::new(ErrorCode::Network, "The connection to the server failed.").retryable().detail(text)
}

/// Per-request context for scoping secrets: cookies and site credentials are only sent to the
/// host the user downloaded from, never to a different host reached by a redirect.
#[derive(Clone, Default)]
pub struct RequestAuth {
    pub origin_host: String,
    pub cookies: Option<String>,
    pub basic: Option<(String, String)>,
    pub referer: Option<String>,
    /// The browser-like default user agent is in use: on 403, try once more as ourselves.
    pub plain_ua_fallback: bool,
}

impl RequestAuth {
    pub fn for_url(url: &str, settings: &Settings, cookies: Option<String>, referer: Option<String>) -> Self {
        let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_lowercase)).unwrap_or_default();
        let plain_ua_fallback = settings.user_agent == crate::settings::DEFAULT_UA;
        RequestAuth { basic: settings.login_for(&host), origin_host: host, cookies, referer, plain_ua_fallback }
    }
}

/// Some CDNs (Cloudflare, for one) refuse a request that claims to be a browser but does not
/// behave like one, while they let a program that names itself through.
pub fn plain_user_agent() -> String {
    format!("SwiftHarryDM/{}", env!("CARGO_PKG_VERSION"))
}

/// GET with manual redirects. Returns (response, final_url). Stops early on cancellation.
pub async fn open_url(
    client: &Client,
    url: &str,
    extra: &HeaderMap,
    auth: &RequestAuth,
    cancel: &CancelToken,
) -> Result<(Response, String), EngineError> {
    let mut current = url.to_string();
    let mut used_basic = false;
    let mut plain_ua = false;
    for _ in 0..=MAX_REDIRECTS {
        if cancel.is_set() {
            return Err(EngineError::Stop(cancel.reason().unwrap_or(StopReason::Pause)));
        }
        let host = url::Url::parse(&current).ok().and_then(|u| u.host_str().map(str::to_lowercase)).unwrap_or_default();
        let same_host = host == auth.origin_host;
        let mut req = client.get(&current).headers(extra.clone()).header("Accept-Encoding", "identity");
        if plain_ua {
            req = req.header(USER_AGENT, plain_user_agent());
        }
        if let Some(r) = &auth.referer {
            if let Ok(v) = HeaderValue::from_str(r) {
                req = req.header(REFERER, v);
            }
        }
        if same_host {
            if let Some(c) = &auth.cookies {
                if let Ok(v) = HeaderValue::from_str(c) {
                    req = req.header(COOKIE, v);
                }
            }
            if used_basic {
                if let Some((u, p)) = &auth.basic {
                    req = req.basic_auth(u, Some(p));
                }
            }
        }
        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(EngineError::Stop(cancel.reason().unwrap_or(StopReason::Pause))),
            r = req.send() => r.map_err(|e| EngineError::Fail(map_reqwest(&e)))?,
        };
        let st = resp.status();
        if st.is_redirection() && st != StatusCode::NOT_MODIFIED {
            let loc = resp.headers().get(LOCATION).and_then(|v| v.to_str().ok()).map(str::to_string);
            current = next_redirect(&current, loc.as_deref())?;
            continue;
        }
        if st == StatusCode::UNAUTHORIZED && same_host && auth.basic.is_some() && !used_basic {
            used_basic = true;
            continue;
        }
        if st == StatusCode::FORBIDDEN && auth.plain_ua_fallback && !plain_ua {
            plain_ua = true;
            continue;
        }
        return Ok((resp, current));
    }
    Err(EngineError::Fail(SdmError::new(ErrorCode::HttpStatus, format!("Too many redirects (more than {MAX_REDIRECTS})."))))
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpProbe {
    pub final_url: String,
    pub total: Option<u64>,
    pub ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
}

impl HttpProbe {
    pub fn is_html(&self) -> bool {
        let ct = self.content_type.as_deref().unwrap_or("").split(';').next().unwrap_or("").trim().to_lowercase();
        ct == "text/html" || ct == "application/xhtml+xml"
    }
}

fn header(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(HeaderName::from_bytes(name.as_bytes()).ok()?)?.to_str().ok().map(|s| s.trim().to_string())
}

pub async fn probe(client: &Client, url: &str, auth: &RequestAuth, cancel: &CancelToken) -> Result<HttpProbe, EngineError> {
    let mut extra = HeaderMap::new();
    extra.insert(RANGE, HeaderValue::from_static("bytes=0-0"));
    let (resp, final_url) = open_url(client, url, &extra, auth, cancel).await?;
    let h = resp.headers().clone();
    let encoding = header(&h, "content-encoding").map(|e| e.to_lowercase()).filter(|e| e != "identity" && !e.is_empty());
    let (total, ranges) = match resp.status() {
        StatusCode::PARTIAL_CONTENT => match parse_content_range(header(&h, "content-range").as_deref()) {
            Some((0, 0, total)) => (total, total.is_some() && encoding.is_none()),
            // Broken range support: fall back to one plain stream of unknown length.
            _ => (None, false),
        },
        StatusCode::OK => {
            let cl = header(&h, "content-length").and_then(|v| v.parse::<u64>().ok());
            (if encoding.is_none() { cl } else { None }, false)
        }
        StatusCode::RANGE_NOT_SATISFIABLE => {
            if header(&h, "content-range").is_some_and(|c| c.ends_with("/0")) {
                (Some(0), true)
            } else {
                return Err(status_error(&resp).into());
            }
        }
        _ => return Err(status_error(&resp).into()),
    };
    drop(resp);
    Ok(HttpProbe {
        final_url,
        total,
        ranges,
        etag: header(&h, "etag"),
        last_modified: header(&h, "last-modified"),
        content_type: header(&h, "content-type"),
        content_disposition: header(&h, "content-disposition"),
        content_encoding: encoding,
    })
}

/// Probe a URL with fresh settings (used by "Refresh download address" and the info dialog).
pub async fn probe_url(url: &str, settings: &Settings, cancel: &CancelToken, cookies: Option<String>) -> Result<HttpProbe, SdmError> {
    let client = build_client(settings)?;
    let auth = RequestAuth::for_url(url, settings, cookies, None);
    probe(&client, url, &auth, cancel).await.map_err(|e| match e {
        EngineError::Fail(e) => e,
        EngineError::Stop(_) => SdmError::new(ErrorCode::Network, "Cancelled."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirect_policy() {
        for (cur, loc, ok) in [
            ("https://a/x", "http://b/y", false),
            ("https://a/x", "https://b/y", true),
            ("http://a/x", "https://b/y", true),
            ("http://a/x", "/rel", true),
            ("http://a/x", "file:///C:/x", false),
            ("http://a/x", "ftp://h/f", false),
        ] {
            let r = next_redirect(cur, Some(loc));
            assert_eq!(r.is_ok(), ok, "{cur} -> {loc}");
            if !ok {
                assert_eq!(r.unwrap_err().code(), ErrorCode::RedirectRefused);
            }
        }
    }
}
