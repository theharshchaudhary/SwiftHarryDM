//! Batch downloads: expand one address with a `*` placeholder into a numbered series
//! (img001.jpg ... img100.jpg, or a.jpg ... z.jpg), and pull every web address out of a
//! block of text (clipboard, imported text files).
use serde::{Deserialize, Serialize};

/// Hard cap so a typo cannot add millions of downloads.
pub const MAX_BATCH: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Series {
    /// `from..=to`, left-padded with zeros to `width` digits (0 or 1 = no padding).
    Numbers { from: u64, to: u64, width: usize },
    /// Single letters `from..=to`, e.g. 'a'..='z'.
    Letters { from: char, to: char },
}

pub fn expand(pattern: &str, series: &Series) -> Result<Vec<String>, String> {
    let pattern = pattern.trim();
    let stars = pattern.matches('*').count();
    if stars != 1 {
        return Err("Put exactly one * in the address where the number or letter goes.".into());
    }
    crate::manager::validate_url(&pattern.replace('*', "1"))?;
    let items: Vec<String> = match series {
        Series::Numbers { from, to, width } => {
            if from > to {
                return Err("The first number must not be larger than the last one.".into());
            }
            if to - from >= MAX_BATCH as u64 {
                return Err(format!("A batch can have at most {MAX_BATCH} files."));
            }
            (*from..=*to).map(|n| format!("{n:0w$}", w = (*width).min(20))).collect()
        }
        Series::Letters { from, to } => {
            if !from.is_ascii_alphabetic() || !to.is_ascii_alphabetic() || from.is_ascii_lowercase() != to.is_ascii_lowercase() {
                return Err("Use two letters of the same case, for example a and z.".into());
            }
            if from > to {
                return Err("The first letter must come before the last one.".into());
            }
            (*from..=*to).map(|c| c.to_string()).collect()
        }
    };
    Ok(items.into_iter().map(|i| pattern.replacen('*', &i, 1)).collect())
}

/// Every distinct http(s) address in `text`, in order of appearance.
pub fn extract_urls(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in text.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`')) {
        let Some(start) = raw.find("http://").or_else(|| raw.find("https://")) else { continue };
        let cand = raw[start..].trim_end_matches(|c: char| matches!(c, ')' | ']' | '}' | ',' | ';' | '.' | '!' | '?'));
        if let Ok(u) = crate::manager::validate_url(cand) {
            if !out.contains(&u) {
                out.push(u);
            }
        }
        if out.len() >= MAX_BATCH {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers_and_letters() {
        let v = expand("http://x.com/img*.jpg", &Series::Numbers { from: 8, to: 11, width: 3 }).unwrap();
        assert_eq!(v, vec!["http://x.com/img008.jpg", "http://x.com/img009.jpg", "http://x.com/img010.jpg", "http://x.com/img011.jpg"]);
        let v = expand("http://x.com/*.jpg", &Series::Letters { from: 'a', to: 'c' }).unwrap();
        assert_eq!(v, vec!["http://x.com/a.jpg", "http://x.com/b.jpg", "http://x.com/c.jpg"]);
        assert!(expand("http://x.com/a.jpg", &Series::Numbers { from: 1, to: 2, width: 0 }).is_err());
        assert!(expand("http://x.com/**.jpg", &Series::Numbers { from: 1, to: 2, width: 0 }).is_err());
        assert!(expand("ftp://x.com/*.jpg", &Series::Numbers { from: 1, to: 2, width: 0 }).is_err());
        assert!(expand("http://x.com/*.jpg", &Series::Numbers { from: 0, to: 20_000, width: 0 }).is_err());
        assert!(expand("http://x.com/*.jpg", &Series::Letters { from: 'a', to: 'Z' }).is_err());
    }
    #[test]
    fn urls_from_text() {
        let t = "See https://a.com/x.zip, and (http://b.org/y.iso).\nDup: https://a.com/x.zip\n<a href=\"https://c.net/z?q=1\">z</a> ftp://no";
        assert_eq!(extract_urls(t), vec!["https://a.com/x.zip", "http://b.org/y.iso", "https://c.net/z?q=1"]);
        assert!(extract_urls("nothing here").is_empty());
    }
}
