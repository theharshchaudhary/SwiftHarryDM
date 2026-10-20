//! IDM's "Zip preview": list the files in a ZIP archive on a server before downloading it.
//! A ZIP keeps its table of contents (the central directory) at the end, so two range
//! requests are enough: the last bytes (end-of-directory record), then the directory itself.
use reqwest::header::{HeaderMap, HeaderValue, RANGE};
use serde::Serialize;

use super::probe::{build_client, open_url, RequestAuth};
use crate::cancel::CancelToken;
use crate::engine::EngineError;
use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZipEntry {
    pub name: String,
    pub size: u64,
    pub packed: u64,
    pub is_dir: bool,
}

/// Bytes read from the end to find the directory (the record is 22 bytes plus a comment of
/// up to 64 KiB).
const TAIL: u64 = 66 * 1024;
/// Directories larger than this are not fetched (archives with very many files).
const MAX_DIR: u64 = 16 * 1024 * 1024;

fn u16_at(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as u64)
}
fn u32_at(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as u64)
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap()))
}

/// Where the central directory is (offset, size), from the end of the archive. `tail` holds
/// the archive's last bytes, starting at `tail_start`.
pub fn find_directory(tail: &[u8], tail_start: u64) -> Result<(u64, u64), String> {
    let eocd = (0..tail.len().saturating_sub(21)).rev().find(|&i| tail[i..].starts_with(&[0x50, 0x4b, 0x05, 0x06])).ok_or("This is not a ZIP archive.")?;
    let (mut size, mut offset) = (u32_at(tail, eocd + 12).unwrap_or(0), u32_at(tail, eocd + 16).unwrap_or(0));
    // ZIP64: the record before points to a 64-bit end-of-directory record.
    if (size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF) && eocd >= 20 && tail[eocd - 20..].starts_with(&[0x50, 0x4b, 0x06, 0x07]) {
        let rec = u64_at(tail, eocd - 20 + 8).ok_or("Damaged ZIP64 record.")?;
        let at = rec.checked_sub(tail_start).ok_or("The ZIP64 record is outside the bytes read.")? as usize;
        if !tail.get(at..).is_some_and(|t| t.starts_with(&[0x50, 0x4b, 0x06, 0x06])) {
            return Err("Damaged ZIP64 record.".into());
        }
        size = u64_at(tail, at + 40).ok_or("Damaged ZIP64 record.")?;
        offset = u64_at(tail, at + 48).ok_or("Damaged ZIP64 record.")?;
    }
    Ok((offset, size))
}

/// The entries of a central directory.
pub fn parse_directory(dir: &[u8]) -> Vec<ZipEntry> {
    let mut out = vec![];
    let mut o = 0;
    while dir.get(o..o + 4) == Some(&[0x50, 0x4b, 0x01, 0x02]) {
        let (Some(packed), Some(size), Some(nlen), Some(xlen), Some(clen)) =
            (u32_at(dir, o + 20), u32_at(dir, o + 24), u16_at(dir, o + 28), u16_at(dir, o + 30), u16_at(dir, o + 32))
        else {
            break;
        };
        let (nlen, xlen, clen) = (nlen as usize, xlen as usize, clen as usize);
        let Some(raw) = dir.get(o + 46..o + 46 + nlen) else { break };
        let name = String::from_utf8_lossy(raw).replace('\\', "/");
        let (mut size, mut packed) = (size, packed);
        // ZIP64 sizes live in the extra field (tag 1).
        if size == 0xFFFF_FFFF || packed == 0xFFFF_FFFF {
            let extra = dir.get(o + 46 + nlen..o + 46 + nlen + xlen).unwrap_or(&[]);
            let mut e = 0;
            while e + 4 <= extra.len() {
                let (tag, len) = (u16_at(extra, e).unwrap_or(0), u16_at(extra, e + 2).unwrap_or(0) as usize);
                if tag == 1 {
                    let mut f = e + 4;
                    if size == 0xFFFF_FFFF {
                        size = u64_at(extra, f).unwrap_or(size);
                        f += 8;
                    }
                    if packed == 0xFFFF_FFFF {
                        packed = u64_at(extra, f).unwrap_or(packed);
                    }
                }
                e += 4 + len;
            }
        }
        out.push(ZipEntry { is_dir: name.ends_with('/'), name, size, packed });
        o += 46 + nlen + xlen + clen;
    }
    out
}

async fn range(client: &reqwest::Client, url: &str, auth: &RequestAuth, from: u64, len: u64) -> Result<(Vec<u8>, Option<u64>), String> {
    let mut h = HeaderMap::new();
    h.insert(RANGE, HeaderValue::from_str(&format!("bytes={from}-{}", from + len - 1)).unwrap());
    let (resp, _) = open_url(client, url, &h, auth, &CancelToken::new()).await.map_err(|e| match e {
        EngineError::Fail(e) => e.0.message,
        EngineError::Stop(_) => "Cancelled.".into(),
    })?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err("The server does not send parts of files, so the archive can't be previewed.".into());
    }
    let total = resp
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|t| t.parse().ok());
    let body = resp.bytes().await.map_err(|e| format!("The connection failed: {e}"))?;
    Ok((body.to_vec(), total))
}

/// List the files in the ZIP archive at `url` (size `total`, if known).
pub async fn preview(url: &str, settings: &Settings, cookies: Option<String>, total: u64) -> Result<Vec<ZipEntry>, String> {
    let client = build_client(settings).map_err(|e| e.0.message)?;
    let auth = RequestAuth::for_url(url, settings, cookies, None);
    let start = total.saturating_sub(TAIL);
    let (tail, _) = range(&client, url, &auth, start, total - start).await?;
    let (offset, size) = find_directory(&tail, start)?;
    if size > MAX_DIR {
        return Err("The archive lists too many files to preview.".into());
    }
    // Usually the directory is already in the bytes read.
    let dir = if offset >= start && offset + size <= total {
        tail[(offset - start) as usize..(offset - start + size) as usize].to_vec()
    } else {
        range(&client, url, &auth, offset, size.max(1)).await?.0
    };
    Ok(parse_directory(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swiftdm_testkit::zip_archive as make_zip;

    #[test]
    fn lists_the_files() {
        let z = make_zip(&[("readme.txt", b"hello"), ("docs/", b""), ("docs/a.pdf", &[7u8; 300])]);
        let (off, size) = find_directory(&z, 0).unwrap();
        let e = parse_directory(&z[off as usize..(off + size) as usize]);
        assert_eq!(e.iter().map(|x| (x.name.as_str(), x.size, x.is_dir)).collect::<Vec<_>>(), vec![("readme.txt", 5, false), ("docs/", 0, true), ("docs/a.pdf", 300, false)]);
        // The last 22 bytes (the end record alone) are enough to find the directory.
        let start = z.len() - 22;
        assert_eq!(find_directory(&z[start..], start as u64).unwrap(), (off, size));
        assert!(find_directory(b"not a zip at all, just some text", 0).is_err());
    }
}
