//! "Find" in the download list (Ctrl+F / F3): search file names, descriptions and addresses
//! (download link, final address after redirects, and the page the download came from).
use serde::{Deserialize, Serialize};

use crate::models::JobSnapshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindQuery {
    pub text: String,
    pub in_name: bool,
    pub in_description: bool,
    /// Download link, final address and referring page.
    pub in_address: bool,
    pub match_case: bool,
    /// The whole field must equal the text instead of containing it.
    pub whole: bool,
}

impl Default for FindQuery {
    fn default() -> Self {
        FindQuery { text: String::new(), in_name: true, in_description: true, in_address: false, match_case: false, whole: false }
    }
}

impl FindQuery {
    pub fn new(text: impl Into<String>) -> Self {
        FindQuery { text: text.into(), ..Default::default() }
    }

    fn field_matches(&self, field: &str) -> bool {
        let (f, t) = if self.match_case {
            (field.to_string(), self.text.clone())
        } else {
            (field.to_lowercase(), self.text.to_lowercase())
        };
        if self.whole {
            f == t
        } else {
            f.contains(&t)
        }
    }

    pub fn matches(&self, s: &JobSnapshot) -> bool {
        if self.text.is_empty() {
            return false;
        }
        let mut fields: Vec<&str> = vec![];
        if self.in_name {
            fields.push(&s.title);
            if let Some(n) = &s.file_name {
                fields.push(n);
            }
        }
        if self.in_description {
            if let Some(d) = &s.description {
                fields.push(d);
            }
        }
        if self.in_address {
            fields.push(&s.url);
            if let Some(u) = &s.final_url {
                fields.push(u);
            }
            if let Some(r) = &s.referer {
                fields.push(r);
            }
        }
        fields.into_iter().any(|f| self.field_matches(f))
    }
}

/// Next match after `after` in list order, wrapping around; `None` if nothing matches.
pub fn find_next<'a>(q: &FindQuery, list: &'a [JobSnapshot], after: Option<&str>) -> Option<&'a JobSnapshot> {
    let start = after.and_then(|id| list.iter().position(|s| s.id == id)).map(|i| i + 1).unwrap_or(0);
    let n = list.len();
    (0..n).map(|k| &list[(start + k) % n]).find(|s| q.matches(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Job, JobSpec};

    fn snap(url: &str, name: &str, desc: Option<&str>, referer: Option<&str>) -> JobSnapshot {
        let mut spec = JobSpec::new(url, "C:\\D");
        spec.filename = Some(name.into());
        spec.description = desc.map(Into::into);
        spec.referer = referer.map(Into::into);
        JobSnapshot::of(&Job::new(format!("id-{name}"), spec), None)
    }

    #[test]
    fn fields_and_options() {
        let s = snap("https://cdn.example.com/Setup.exe", "Setup.exe", Some("Installer v2"), Some("https://example.com/download"));
        assert!(FindQuery::new("setup").matches(&s));
        assert!(!FindQuery { match_case: true, ..FindQuery::new("setup") }.matches(&s));
        assert!(FindQuery::new("installer").matches(&s));
        assert!(!FindQuery { in_description: false, ..FindQuery::new("installer") }.matches(&s));
        assert!(!FindQuery::new("example.com/download").matches(&s));
        assert!(FindQuery { in_address: true, ..FindQuery::new("example.com/download") }.matches(&s));
        assert!(!FindQuery { whole: true, ..FindQuery::new("setup") }.matches(&s));
        assert!(FindQuery { whole: true, ..FindQuery::new("setup.exe") }.matches(&s));
        assert!(!FindQuery::new("").matches(&s));
    }

    #[test]
    fn next_wraps_around() {
        let list = vec![snap("http://a/1", "a.zip", None, None), snap("http://a/2", "b.iso", None, None), snap("http://a/3", "c.zip", None, None)];
        let q = FindQuery::new(".zip");
        assert_eq!(find_next(&q, &list, None).unwrap().id, "id-a.zip");
        assert_eq!(find_next(&q, &list, Some("id-a.zip")).unwrap().id, "id-c.zip");
        assert_eq!(find_next(&q, &list, Some("id-c.zip")).unwrap().id, "id-a.zip");
        assert!(find_next(&FindQuery::new("nothing"), &list, None).is_none());
    }
}
