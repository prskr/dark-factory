//! The form hosts are stored and compared in. The allowlist form (`df-web`) and the egress
//! gateway's policy matching both use it, so a stored entry and a request agree.

use std::net::IpAddr;

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
}
