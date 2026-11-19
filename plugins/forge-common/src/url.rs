//! Percent-encoding for the URLs the plugins build by hand.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

/// Keeps only RFC 3986 unreserved characters (`A-Za-z0-9-._~`).
const VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Like [`VALUE`], but keeps `/`.
const PATH: &AsciiSet = &VALUE.remove(b'/');

/// A query value or a single path segment: everything but unreserved characters is encoded, `/`
/// included.
pub fn encode(value: &str) -> String {
    utf8_percent_encode(value, VALUE).to_string()
}

/// Like [`encode`], but keeps `/`: a branch name in the path of a ref endpoint.
pub fn encode_path(value: &str) -> String {
    utf8_percent_encode(value, PATH).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_characters_are_encoded() {
        assert_eq!(encode("renovate[bot]"), "renovate%5Bbot%5D");
        assert_eq!(encode("a/b c?d#e%"), "a%2Fb%20c%3Fd%23e%25");
        assert_eq!(encode("é"), "%C3%A9");
    }

    #[test]
    fn a_path_keeps_its_slashes() {
        assert_eq!(encode_path("feature/a b"), "feature/a%20b");
    }
}
