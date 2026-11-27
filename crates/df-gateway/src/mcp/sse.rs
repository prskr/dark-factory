//! Server-sent events as the Streamable HTTP transport uses them: only the `data` lines matter.

/// Splits a byte stream into the data of its events.
#[derive(Debug, Default)]
pub(crate) struct SseParser {
    pending: String,
}

impl SseParser {
    /// Adds `chunk` and returns the data of every event it completed. Carriage returns are
    /// dropped, so `\r\n` and `\n` line ends both work.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        self.pending.extend(
            String::from_utf8_lossy(chunk)
                .chars()
                .filter(|c| *c != '\r'),
        );
        let mut events = Vec::new();
        while let Some(end) = self.pending.find("\n\n") {
            let block: String = self.pending.drain(..end + 2).collect();
            let data: Vec<&str> = block
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
                .collect();
            if !data.is_empty() {
                events.push(data.join("\n"));
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_split_at_blank_lines_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b"event: message\nda").is_empty());
        assert_eq!(
            parser.feed(b"ta: {\"a\":1}\n\ndata: x\ndata: y\n\n: comment\n\nretry: 5\n\ndata: z"),
            ["{\"a\":1}", "x\ny"]
        );
        assert_eq!(parser.feed(b"\n\n"), ["z"]);
    }

    #[test]
    fn crlf_line_ends_work() {
        let mut parser = SseParser::default();
        assert_eq!(parser.feed(b"event: message\r\ndata: {}\r\n\r\n"), ["{}"]);
    }
}
