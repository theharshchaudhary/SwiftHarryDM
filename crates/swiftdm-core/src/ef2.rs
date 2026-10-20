//! IDM's export file format (`.ef2`): one block per download,
//!
//! ```text
//! <
//! https://example.com/file.zip
//! referer: https://example.com/page.html
//! User-Agent: Mozilla/5.0 ...
//! >
//! ```
//!
//! so lists move between IDM and SwiftHarryDM in both directions.

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ef2Entry {
    pub url: String,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

/// Read every block of an export file. Lines outside blocks and unknown fields are ignored.
pub fn parse(text: &str) -> Vec<Ef2Entry> {
    let mut out = vec![];
    let mut cur: Option<Ef2Entry> = None;
    for line in text.lines().map(str::trim) {
        match line {
            "<" => cur = Some(Ef2Entry::default()),
            ">" => {
                if let Some(e) = cur.take().filter(|e| e.url.starts_with("http://") || e.url.starts_with("https://")) {
                    out.push(e);
                }
            }
            _ => {
                let Some(e) = cur.as_mut() else { continue };
                if e.url.is_empty() {
                    e.url = line.to_string();
                } else if let Some((k, v)) = line.split_once(':') {
                    let v = v.trim().to_string();
                    match k.trim().to_ascii_lowercase().as_str() {
                        "referer" if !v.is_empty() => e.referer = Some(v),
                        "user-agent" if !v.is_empty() => e.user_agent = Some(v),
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

/// Write an export file IDM can import (Windows line endings).
pub fn write(entries: &[Ef2Entry]) -> String {
    let mut s = String::new();
    for e in entries {
        s.push_str("<\r\n");
        s.push_str(&e.url);
        s.push_str("\r\n");
        if let Some(r) = &e.referer {
            s.push_str(&format!("referer: {r}\r\n"));
        }
        if let Some(u) = &e.user_agent {
            s.push_str(&format!("User-Agent: {u}\r\n"));
        }
        s.push_str(">\r\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_tolerant_reading() {
        let list = vec![
            Ef2Entry { url: "https://a.example/x.zip".into(), referer: Some("https://a.example/".into()), user_agent: None },
            Ef2Entry { url: "http://b.example/y.iso".into(), referer: None, user_agent: Some("UA/1".into()) },
        ];
        assert_eq!(parse(&write(&list)), list);
        // IDM's own files: CRLF, a cookie line, and a non-web entry that is skipped.
        let idm = "<\r\nhttps://c.example/z.mp4\r\ncookie: a=b\r\nreferer: https://c.example/v\r\n>\r\n<\r\nftp://d.example/f\r\n>\r\n";
        assert_eq!(parse(idm), vec![Ef2Entry { url: "https://c.example/z.mp4".into(), referer: Some("https://c.example/v".into()), user_agent: None }]);
    }
}
