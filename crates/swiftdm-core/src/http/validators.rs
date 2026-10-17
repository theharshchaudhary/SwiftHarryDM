//! Response validation: Content-Range parsing and resource-identity checks for resume.

/// "bytes 0-99/1000" -> (0, 99, Some(1000)); total None for "*". None if malformed.
pub fn parse_content_range(v: Option<&str>) -> Option<(u64, u64, Option<u64>)> {
    let v = v?.trim();
    let rest = v.strip_prefix("bytes").or_else(|| v.strip_prefix("Bytes"))?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let (a, b) = range.trim().split_once('-')?;
    let start: u64 = a.trim().parse().ok()?;
    let end: u64 = b.trim().parse().ok()?;
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        t => {
            let t: u64 = t.parse().ok()?;
            if end >= t {
                return None;
            }
            Some(t)
        }
    };
    Some((start, end, total))
}

pub fn is_strong(etag: Option<&str>) -> bool {
    etag.is_some_and(|e| !e.trim().is_empty() && !e.trim().starts_with("W/"))
}

/// RFC 9110: If-Range requires a strong validator; fall back to Last-Modified.
pub fn if_range_value(etag: Option<&str>, last_modified: Option<&str>) -> Option<String> {
    if is_strong(etag) {
        return etag.map(str::to_string);
    }
    last_modified.filter(|l| !l.is_empty()).map(str::to_string)
}

/// Decide whether partially downloaded bytes may be kept. Returns Err(reason) if not.
pub fn same_resource(
    old_total: Option<u64>,
    old_etag: Option<&str>,
    old_lm: Option<&str>,
    new_total: Option<u64>,
    new_etag: Option<&str>,
    new_lm: Option<&str>,
) -> Result<(), String> {
    match (old_total, new_total) {
        (Some(a), Some(b)) if a != b => return Err(format!("size changed from {a} to {b} bytes")),
        (None, _) | (_, None) => return Err("the file size is unknown".into()),
        _ => {}
    }
    if let (Some(a), Some(b)) = (old_etag, new_etag) {
        if a != b {
            return Err("the server's ETag changed".into());
        }
    }
    if let (Some(a), Some(b)) = (old_lm, new_lm) {
        if a != b {
            return Err("the server's modification date changed".into());
        }
    }
    let etag_ok = old_etag.is_some() && old_etag == new_etag;
    let lm_ok = old_lm.is_some() && old_lm == new_lm;
    if (old_etag.is_some() || old_lm.is_some()) && !(etag_ok || lm_ok) {
        return Err("the server stopped sending its validator".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_range() {
        assert_eq!(parse_content_range(Some("bytes 0-99/1000")), Some((0, 99, Some(1000))));
        assert_eq!(parse_content_range(Some("bytes 5-5/*")), Some((5, 5, None)));
        for bad in ["bytes 9-1/10", "bytes 0-10/10", "items 0-1/2", "bytes x-1/2", ""] {
            assert_eq!(parse_content_range(Some(bad)), None, "{bad}");
        }
    }
    #[test]
    fn identity() {
        assert!(same_resource(Some(10), Some("\"a\""), None, Some(10), Some("\"a\""), None).is_ok());
        assert!(same_resource(Some(10), None, None, Some(10), None, None).is_ok());
        assert!(same_resource(Some(10), Some("\"a\""), None, Some(10), Some("\"b\""), None).is_err());
        assert!(same_resource(Some(10), None, None, Some(11), None, None).is_err());
        assert!(same_resource(Some(10), Some("\"a\""), None, Some(10), None, None).is_err());
        assert_eq!(if_range_value(Some("W/\"x\""), Some("lm")).as_deref(), Some("lm"));
        assert_eq!(if_range_value(Some("\"x\""), Some("lm")).as_deref(), Some("\"x\""));
    }
}
