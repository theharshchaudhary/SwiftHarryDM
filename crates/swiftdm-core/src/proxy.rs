//! Proxy configuration: Windows system proxy (Internet Settings), none, or manual.
use crate::settings::{ProxyMode, Settings};

/// Read the per-user WinINET proxy (what browsers use). Returns e.g. "http://host:port".
pub fn system_proxy() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings");
    let mut enable: u32 = 0;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            w!("ProxyEnable"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut enable as *mut u32 as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    if enable == 0 {
        return None;
    }
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            w!("ProxyServer"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    let s = String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]);
    parse_proxy_server(&s)
}

/// "host:port" or "http=h:p;https=h2:p2" -> URL for HTTPS traffic (falls back to http= entry).
pub fn parse_proxy_server(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let pick = if s.contains('=') {
        let entries: Vec<(&str, &str)> = s.split(';').filter_map(|e| e.split_once('=')).collect();
        entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("https"))
            .or_else(|| entries.iter().find(|(k, _)| k.eq_ignore_ascii_case("http")))
            .map(|(_, v)| v.trim().to_string())?
    } else {
        s.to_string()
    };
    Some(if pick.contains("://") { pick } else { format!("http://{pick}") })
}

pub fn proxy_for(settings: &Settings) -> Option<String> {
    match settings.proxy_mode {
        ProxyMode::None => None,
        ProxyMode::Manual => (!settings.proxy_url.trim().is_empty()).then(|| settings.proxy_url.trim().to_string()),
        ProxyMode::System => system_proxy(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_wininet_formats() {
        assert_eq!(parse_proxy_server("proxy:8080").as_deref(), Some("http://proxy:8080"));
        assert_eq!(parse_proxy_server("http=a:1;https=b:2").as_deref(), Some("http://b:2"));
        assert_eq!(parse_proxy_server("http=a:1;ftp=c:3").as_deref(), Some("http://a:1"));
        assert_eq!(parse_proxy_server("socks5://s:1080").as_deref(), Some("socks5://s:1080"));
        assert_eq!(parse_proxy_server(""), None);
    }
}
