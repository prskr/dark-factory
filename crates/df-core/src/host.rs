//! The form hosts are stored and compared in. The allowlist form (`df-web`), the `network-access`
//! approval type (`df-db`) and the egress gateway's policy matching all use it, so a stored entry
//! and a request agree.

use std::net::IpAddr;

/// Port of an `http-host` entry or `network-access` request that names none.
pub const DEFAULT_HOST_PORT: u16 = 443;

/// Lower case without a trailing dot. Whitespace is not touched: a host that arrives over the
/// wire (SNI, `Host`, `CONNECT`) is not silently repaired; a form trims its input before it
/// calls this.
pub fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// `host` without the brackets of an IPv6 literal: exactly one matching pair is removed, anything
/// else is returned as it is.
pub fn strip_brackets(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host)
}

/// Whether `host` is an IPv4 or IPv6 address; an IPv6 address may be in brackets.
pub fn is_ip_literal(host: &str) -> bool {
    strip_brackets(host).parse::<IpAddr>().is_ok()
}

/// Whether `label` is a lower case DNS-1123 label.
pub fn is_dns_label(label: &str) -> bool {
    let edge = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    (1..=63).contains(&label.len())
        && label.starts_with(edge)
        && label.ends_with(edge)
        && label.chars().all(|c| edge(c) || c == '-')
}

/// The host of an allowlist entry as stored: a hostname or `*.suffix`, lower case, without a
/// trailing dot. IP literals, a bare `*` and wildcards anywhere but at the start are rejected.
pub fn validate_allowlist_host(raw: &str) -> Result<String, &'static str> {
    let host = normalize_host(raw.trim());
    if host.is_empty() {
        return Err("Host is required.");
    }
    if is_ip_literal(&host) {
        return Err("IP addresses are not allowed. Use a hostname.");
    }
    let name = host.strip_prefix("*.").unwrap_or(&host);
    if host == "*" || name.len() > 253 || !name.split('.').all(is_dns_label) {
        return Err(
            "Must be a hostname such as pypi.org, or *.example.org for all its subdomains.",
        );
    }
    Ok(host)
}

/// Like [`validate_allowlist_host`], but for a single host: a `*.suffix` wildcard is rejected.
pub fn validate_exact_host(raw: &str) -> Result<String, &'static str> {
    let host = validate_allowlist_host(raw)?;
    if host.starts_with("*.") {
        return Err("Must be a single hostname, not a wildcard.");
    }
    Ok(host)
}

/// Whether the allowlist `pattern` (a hostname or `*.suffix`) covers the normalized `host`.
/// `*.example.com` covers every name below `example.com`, not `example.com` itself.
pub fn pattern_matches(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => host
            .strip_suffix(suffix)
            .is_some_and(|label| label.len() > 1 && label.ends_with('.')),
        None => pattern == host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_lower_case_without_trailing_dots_and_not_trimmed() {
        assert_eq!(normalize_host("API.GitHub.com."), "api.github.com");
        assert_eq!(normalize_host("github.com.."), "github.com");
        assert_eq!(normalize_host(" github.com"), " github.com");
        assert_eq!(normalize_host(""), "");
    }

    #[test]
    fn exactly_one_pair_of_brackets_is_stripped() {
        assert_eq!(strip_brackets("[::1]"), "::1");
        assert_eq!(strip_brackets("::1"), "::1");
        for odd in ["[[::1]]", "[::1", "::1]", "[", "[]x"] {
            assert_ne!(strip_brackets(odd), "::1", "{odd}");
        }
        assert_eq!(strip_brackets("[[::1]]"), "[::1]");
    }

    #[test]
    fn ip_literals_are_recognized_with_or_without_brackets() {
        for host in ["10.0.0.1", "::1", "[::1]", "2606:4700::1111"] {
            assert!(is_ip_literal(host), "{host}");
        }
        for host in ["github.com", "", "[[::1]]", "[::1", "::1]", "[10.0.0.1"] {
            assert!(!is_ip_literal(host), "{host}");
        }
    }

    #[test]
    fn allowlist_hosts_are_normalized_and_ip_literals_rejected() {
        assert_eq!(
            validate_allowlist_host(" API.Anthropic.com. "),
            Ok("api.anthropic.com".into())
        );
        assert_eq!(
            validate_allowlist_host("*.crates.io"),
            Ok("*.crates.io".into())
        );
        assert_eq!(validate_allowlist_host("localhost"), Ok("localhost".into()));
        for ip in ["10.0.0.5", "1.1.1.1", "::1", "[::1]", "2606:4700::1111"] {
            assert!(
                validate_allowlist_host(ip).unwrap_err().contains("IP"),
                "{ip}"
            );
        }
        for bad in [
            "",
            "  ",
            "*",
            "*.",
            "a.*.b",
            "-a.com",
            "a..com",
            "a b.com",
            "exa_mple.com",
            "a.com/x",
            "*.*.com",
            "[[::1]]",
            "[::1",
            "::1]",
        ] {
            assert!(validate_allowlist_host(bad).is_err(), "{bad:?}");
        }
        assert!(validate_allowlist_host(&format!("{}.com", "a".repeat(64))).is_err());
    }

    #[test]
    fn exact_hosts_are_valid_allowlist_hosts_without_wildcards() {
        assert_eq!(
            validate_exact_host(" API.Anthropic.com. "),
            Ok("api.anthropic.com".into())
        );
        assert!(
            validate_exact_host("*.crates.io")
                .unwrap_err()
                .contains("wildcard")
        );
        for bad in ["", "*", "10.0.0.5", "[::1]", "a..com", "exa_mple.com"] {
            assert!(validate_exact_host(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn wildcard_patterns_cover_subdomains_only() {
        assert!(pattern_matches("pypi.org", "pypi.org"));
        assert!(!pattern_matches("pypi.org", "files.pypi.org"));
        assert!(pattern_matches("*.crates.io", "static.crates.io"));
        assert!(pattern_matches("*.crates.io", "a.b.crates.io"));
        assert!(!pattern_matches("*.crates.io", "crates.io"));
        assert!(!pattern_matches("*.crates.io", "evilcrates.io"));
        assert!(!pattern_matches("*.crates.io", ".crates.io"));
    }
}
