//! Job logs are kept as their last [`MAX_BYTES`].

/// The most of a job log a plugin returns: 4 MiB.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

/// Keeps the last `max` bytes of `body`; true if bytes were dropped.
pub fn keep_tail(body: &mut Vec<u8>, max: usize) -> bool {
    let Some(excess) = body.len().checked_sub(max).filter(|e| *e > 0) else {
        return false;
    };
    body.drain(..excess);
    true
}

/// `bytes` as text. After a cut the first character may be incomplete: its remaining bytes are
/// skipped. Invalid UTF-8 elsewhere becomes U+FFFD.
pub fn text(bytes: &[u8], cut: bool) -> String {
    let start = if cut {
        bytes
            .iter()
            .position(|b| (b & 0b1100_0000) != 0b1000_0000)
            .unwrap_or(bytes.len())
    } else {
        0
    };
    String::from_utf8_lossy(bytes.get(start..).unwrap_or_default()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_body_is_kept_whole() {
        let mut body = b"abc".to_vec();
        assert!(!keep_tail(&mut body, 3));
        assert_eq!(body, b"abc");
    }

    #[test]
    fn a_long_body_keeps_its_end() {
        let mut body = b"abcdef".to_vec();
        assert!(keep_tail(&mut body, 2));
        assert_eq!(body, b"ef");
    }

    #[test]
    fn a_cut_inside_a_character_drops_the_rest_of_it() {
        let mut body = "aé!".as_bytes().to_vec(); // 61 C3 A9 21
        assert!(keep_tail(&mut body, 2)); // A9 21
        assert_eq!(text(&body, true), "!");
        assert_eq!(text("é!".as_bytes(), false), "é!");
    }
}
