//! File-name selection and sanitization for Windows.
//! Preference: user override > Content-Disposition (RFC 6266 / 5987) > URL basename > "download".
//! Server-supplied names are reduced to a bare basename, so `filename="..\..\evil.exe"`
//! cannot escape the destination folder.
use percent_encoding::percent_decode_str;
use unicode_normalization::UnicodeNormalization;

/// Leave room for " (9999)" and the hidden temp suffix ".<id8>.sdmpart" within 255 UTF-16 units.
pub const MAX_NAME_UNITS: usize = 220;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    "LPT¹", "LPT²", "LPT³",
];

pub fn utf16_units(s: &str) -> usize {
    s.encode_utf16().count()
}

pub fn split_ext(name: &str) -> (&str, &str) {
    if let Some(i) = name.rfind('.') {
        if i > 0 {
            let ext = &name[i + 1..];
            if (1..=10).contains(&ext.chars().count()) && ext.chars().all(|c| c.is_alphanumeric()) {
                return (&name[..i], &name[i..]);
            }
        }
    }
    (name, "")
}

fn invalid(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 0x20 || c == '\x7f'
}

pub fn sanitize_component(name: &str) -> String {
    sanitize_with(name, MAX_NAME_UNITS)
}

pub fn sanitize_with(name: &str, max_units: usize) -> String {
    let n: String = name.nfc().map(|c| if invalid(c) { '_' } else { c }).collect();
    let mut n = n.trim().trim_end_matches(['.', ' ']).trim().to_string();
    if n.is_empty() || n == "." || n == ".." {
        n = "download".into();
    }
    let base = n.split('.').next().unwrap_or("").trim_end().to_uppercase();
    if RESERVED.contains(&base.as_str()) {
        n = format!("_{n}");
    }
    let (stem, ext) = split_ext(&n);
    let (mut stem, ext) = if utf16_units(ext) >= max_units / 2 {
        (n.clone(), String::new())
    } else {
        (stem.to_string(), ext.to_string())
    };
    if utf16_units(&stem) + utf16_units(&ext) > max_units {
        while utf16_units(&stem) + utf16_units(&ext) > max_units && !stem.is_empty() {
            stem.pop();
        }
        stem = stem.trim_end_matches(['.', ' ']).to_string();
    }
    if stem.is_empty() {
        stem = "download".into();
    }
    stem + &ext
}

fn parse_params(value: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut out: Vec<(String, String)> = Vec::new();
    while i < n && chars[i] != ';' {
        i += 1;
    }
    while i < n {
        i += 1; // ';'
        while i < n && (chars[i] == ' ' || chars[i] == '\t') {
            i += 1;
        }
        let start = i;
        while i < n && chars[i] != '=' && chars[i] != ';' {
            i += 1;
        }
        let key: String = chars[start..i].iter().collect::<String>().trim().to_lowercase();
        if i >= n || chars[i] == ';' {
            continue;
        }
        i += 1; // '='
        while i < n && (chars[i] == ' ' || chars[i] == '\t') {
            i += 1;
        }
        let val = if i < n && chars[i] == '"' {
            i += 1;
            let mut buf = String::new();
            while i < n && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < n {
                    i += 1;
                }
                buf.push(chars[i]);
                i += 1;
            }
            i += 1;
            while i < n && chars[i] != ';' {
                i += 1;
            }
            buf
        } else {
            let start = i;
            while i < n && chars[i] != ';' {
                i += 1;
            }
            chars[start..i].iter().collect::<String>().trim().to_string()
        };
        if !key.is_empty() && !out.iter().any(|(k, _)| *k == key) {
            out.push((key, val));
        }
    }
    out
}

fn decode_ext_value(v: &str) -> Option<String> {
    let mut parts = v.splitn(3, '\'');
    let charset = parts.next()?.trim().to_lowercase();
    let _lang = parts.next()?;
    let data = parts.next()?;
    let bytes: Vec<u8> = percent_decode_str(data).collect();
    match charset.as_str() {
        "utf-8" | "" => String::from_utf8(bytes).ok(),
        "iso-8859-1" | "us-ascii" => Some(bytes.iter().map(|&b| b as char).collect()),
        _ => None,
    }
}

fn basename(v: &str) -> Option<String> {
    let last = v.trim().rsplit(['\\', '/']).next().unwrap_or("").trim().to_string();
    (!last.is_empty() && last != "." && last != "..").then_some(last)
}

pub fn parse_content_disposition(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.is_empty() {
        return None;
    }
    let params = parse_params(value);
    let get = |k: &str| params.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    if let Some(ext) = get("filename*") {
        if let Some(d) = decode_ext_value(&ext) {
            if let Some(b) = basename(&d) {
                return Some(b);
            }
        }
    }
    if let Some(raw) = get("filename") {
        // Many servers send raw UTF-8 bytes, which arrive as latin-1 characters.
        let fixed = if raw.chars().all(|c| (c as u32) < 256) {
            let bytes: Vec<u8> = raw.chars().map(|c| c as u8).collect();
            String::from_utf8(bytes).unwrap_or(raw)
        } else {
            raw
        };
        return basename(&fixed);
    }
    None
}

pub fn name_from_url(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let last = u.path().rsplit('/').next()?;
    let decoded = percent_decode_str(last).decode_utf8_lossy().into_owned();
    basename(&decoded)
}

pub fn ext_for_type(content_type: Option<&str>) -> &'static str {
    let ct = content_type.unwrap_or("").split(';').next().unwrap_or("").trim().to_lowercase();
    match ct.as_str() {
        "application/zip" | "application/x-zip-compressed" => ".zip",
        "application/pdf" => ".pdf",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => ".exe",
        "application/x-msi" | "application/x-ms-installer" => ".msi",
        "application/x-7z-compressed" => ".7z",
        "application/vnd.rar" | "application/x-rar-compressed" => ".rar",
        "application/gzip" | "application/x-gzip" => ".gz",
        "application/x-tar" => ".tar",
        "application/x-iso9660-image" => ".iso",
        "application/json" => ".json",
        "text/plain" => ".txt",
        "text/csv" => ".csv",
        "text/html" => ".html",
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        "video/x-matroska" => ".mkv",
        "audio/mpeg" => ".mp3",
        "audio/mp4" => ".m4a",
        "audio/ogg" => ".ogg",
        "audio/wav" | "audio/x-wav" => ".wav",
        "application/vnd.android.package-archive" => ".apk",
        _ => "",
    }
}

pub fn choose_filename(
    override_name: Option<&str>,
    content_disposition: Option<&str>,
    final_url: Option<&str>,
    content_type: Option<&str>,
) -> String {
    let raw = override_name
        .map(str::to_string)
        .or_else(|| parse_content_disposition(content_disposition))
        .or_else(|| final_url.and_then(name_from_url))
        .unwrap_or_else(|| "download".into());
    let mut name = sanitize_component(&raw);
    if split_ext(&name).1.is_empty() && override_name.is_none() {
        let ext = ext_for_type(content_type);
        if !ext.is_empty() {
            name = sanitize_component(&format!("{name}{ext}"));
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_vectors() {
        let cases: &[(&str, Option<&str>)] = &[
            (r#"attachment; filename="foo.html""#, Some("foo.html")),
            (r#"inline; filename="foo.pdf""#, Some("foo.pdf")),
            ("attachment; filename=foo.html", Some("foo.html")),
            (r#"attachment; filename="f\oo.html""#, Some("foo.html")),
            (r#"attachment; filename="\"quoting\" tested.html""#, Some("\"quoting\" tested.html")),
            ("attachment; filename=\"Here's a semicolon;.html\"", Some("Here's a semicolon;.html")),
            (r#"attachment; foo="bar"; filename="foo.html""#, Some("foo.html")),
            (r#"attachment; FILENAME="foo.html""#, Some("foo.html")),
            ("attachment; filename*=UTF-8''foo-%c3%a4-%e2%82%ac.html", Some("foo-ä-€.html")),
            ("attachment; filename*=iso-8859-1''foo-%E4.html", Some("foo-ä.html")),
            ("attachment; filename=\"foo-ae.html\"; filename*=UTF-8''foo-%c3%a4.html", Some("foo-ä.html")),
            ("attachment; filename*=UTF-8''foo-%c3%a4.html; filename=\"foo-ae.html\"", Some("foo-ä.html")),
            ("attachment; filename*=bogus''x.html; filename=\"fallback.html\"", Some("fallback.html")),
            ("attachment; filename=\"fooÃ¤.html\"", Some("fooä.html")),
            (r#"attachment; filename="/foo.html""#, Some("foo.html")),
            (r#"attachment; filename="../../evil.exe""#, Some("evil.exe")),
            (r#"attachment; filename="..\\..\\evil.exe""#, Some("evil.exe")),
            ("attachment; filename*=UTF-8''..%2F..%2Fevil.exe", Some("evil.exe")),
            (r#"attachment; filename=".""#, None),
            (r#"attachment; filename="""#, None),
            ("attachment", None),
            ("", None),
        ];
        for (h, want) in cases {
            assert_eq!(parse_content_disposition(Some(h)).as_deref(), *want, "{h}");
        }
    }

    #[test]
    fn sanitize_vectors() {
        for (raw, want) in [
            ("CON", "_CON"),
            ("con.txt", "_con.txt"),
            ("LPT1.tar.gz", "_LPT1.tar.gz"),
            ("COM¹.txt", "_COM¹.txt"),
            ("nul .txt", "_nul .txt"),
            ("CONSOLE.txt", "CONSOLE.txt"),
            (r#"a<b>c:d"e/f\g|h?i*j.txt"#, "a_b_c_d_e_f_g_h_i_j.txt"),
            ("name. . ", "name"),
            ("  spaced.txt ", "spaced.txt"),
            ("", "download"),
            ("...", "download"),
            ("tab\there\x01.bin", "tab_here_.bin"),
            ("e\u{301}.txt", "\u{e9}.txt"),
        ] {
            assert_eq!(sanitize_component(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn sanitize_length_limits() {
        for raw in [
            format!("{}.mp4", "é".repeat(300)),
            format!("{}.txt", "😀".repeat(200)),
            "x".repeat(1000),
            format!("a.{}{}", "b".repeat(9), "c".repeat(400)),
        ] {
            let out = sanitize_component(&raw);
            assert!(utf16_units(&out) <= MAX_NAME_UNITS && !out.is_empty());
            if raw.ends_with(".mp4") || raw.ends_with(".txt") {
                assert!(out.ends_with(&raw[raw.len() - 4..]));
            }
        }
    }

    #[test]
    fn choose_filename_vectors() {
        for (ov, cd, url, ct, want) in [
            (None, None, "http://h/file.bin", Some("application/octet-stream"), "file.bin"),
            (None, None, "http://h/download?id=1", Some("application/zip"), "download.zip"),
            (None, None, "http://h/", Some("text/plain"), "download.txt"),
            (None, None, "http://h/", None, "download"),
            (None, Some(r#"attachment; filename="report.pdf""#), "http://h/get?id=9", Some("application/pdf"), "report.pdf"),
            (Some("my name.iso"), Some(r#"attachment; filename="x.bin""#), "http://h/y", None, "my name.iso"),
            (None, None, "http://h/my%20file%20%E2%82%AC.pdf", None, "my file €.pdf"),
            (None, None, "http://h/a/CON", Some("application/x-msdownload"), "_CON.exe"),
        ] {
            assert_eq!(choose_filename(ov, cd, Some(url), ct), want);
        }
    }
}
