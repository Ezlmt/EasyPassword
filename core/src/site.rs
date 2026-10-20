//! Site identifier normalization.

/// Turns whatever the user typed or pasted into a canonical site key:
///
/// - trims and lower-cases
/// - `https://user@www.GitHub.com:443/login?x#y` -> `github.com`
/// - `github.com/login` -> `github.com`
/// - strips a leading `www.` and trailing dots
///
/// Without a scheme, `@` is kept, so `work@github.com` can be used as a
/// separate account key.
pub fn normalize_site(input: &str) -> String {
    let mut s = input.trim().to_lowercase();

    if let Some(idx) = s.find("://") {
        let rest = &s[idx + 3..];
        let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let mut host = &rest[..host_end];
        if let Some(at) = host.rfind('@') {
            host = &host[at + 1..];
        }
        // Drop a port, but leave IPv6 literals like [::1] intact.
        if !host.starts_with('[') {
            if let Some(colon) = host.rfind(':') {
                if host[colon + 1..].chars().all(|c| c.is_ascii_digit()) {
                    host = &host[..colon];
                }
            }
        }
        s = host.to_string();
    } else if let Some(idx) = s.find(['/', '?', '#']) {
        s.truncate(idx);
    }

    let mut s = s.trim().trim_end_matches('.').to_string();
    if let Some(stripped) = s.strip_prefix("www.") {
        if !stripped.is_empty() {
            s = stripped.to_string();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::normalize_site as n;

    #[test]
    fn normalizes() {
        assert_eq!(n("github.com"), "github.com");
        assert_eq!(n("  GitHub.COM "), "github.com");
        assert_eq!(n("www.github.com"), "github.com");
        assert_eq!(n("https://www.github.com/login?next=/"), "github.com");
        assert_eq!(n("http://user:pw@example.com:8080/x"), "example.com");
        assert_eq!(n("https://[::1]:8443/"), "[::1]");
        assert_eq!(n("github.com/login"), "github.com");
        assert_eq!(n("github.com."), "github.com");
        assert_eq!(n("work@github.com"), "work@github.com");
        assert_eq!(n("My Bank"), "my bank");
        assert_eq!(n("www."), "www");
        assert_eq!(n(""), "");
        assert_eq!(n("工商银行"), "工商银行");
    }
}
