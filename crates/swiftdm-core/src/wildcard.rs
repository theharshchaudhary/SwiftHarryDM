//! `*` wildcard patterns used by exception lists, per-server limits and batch downloads.
//! `*` matches any run of characters (including none); everything else is literal.
//! Matching is case-insensitive, as host names and Windows paths are.

pub fn matches(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.trim().to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    // Iterative glob match with single-star backtracking: O(len(p) * len(t)) worst case.
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// A host pattern matches the host itself; "example.com" also matches its subdomains, the way
/// users expect site lists to behave. "*.example.com" matches subdomains only.
pub fn host_matches(pattern: &str, host: &str) -> bool {
    let pat = pattern.trim().trim_end_matches('/').to_lowercase();
    let host = host.to_lowercase();
    if pat.is_empty() {
        return false;
    }
    if pat.contains('*') {
        return matches(&pat, &host);
    }
    host == pat || host.ends_with(&format!(".{pat}"))
}

/// Any pattern in a space- or newline-separated list.
pub fn any_host(patterns: &[String], host: &str) -> bool {
    patterns.iter().any(|p| host_matches(p, host))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glob() {
        assert!(matches("*", ""));
        assert!(matches("http://*.example.com/*.zip", "http://dl.example.com/a/b.ZIP"));
        assert!(!matches("http://*.example.com/*.zip", "http://example.org/b.zip"));
        assert!(matches("a*b*c", "aXXbYYc"));
        assert!(!matches("a*b*c", "aXXbYY"));
        assert!(matches("img??", "img??"));
        assert!(matches("*a*a*a*a*b", &"a".repeat(60).replace("aaaa", "aaab")));
    }
    #[test]
    fn hosts() {
        assert!(host_matches("example.com", "example.com"));
        assert!(host_matches("example.com", "cdn.example.com"));
        assert!(!host_matches("example.com", "badexample.com"));
        assert!(host_matches("*.example.com", "cdn.example.com"));
        assert!(!host_matches("*.example.com", "example.com"));
        assert!(host_matches("dl*.mirror.net", "dl3.mirror.net"));
        assert!(!host_matches("", "x.com"));
    }
}
