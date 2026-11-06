//! Warnings for the operator.

/// Warnings for the operator.
pub trait Log {
    fn warn(&self, message: &str);
}

/// Written before a warning on stderr; the host logs such lines at `warn`.
pub const WARNING_PREFIX: &str = "warning: ";

/// Writes warnings to stderr, which the host forwards to its log.
pub struct StderrLog;

impl Log for StderrLog {
    #[expect(clippy::print_stderr, reason = "the host logs plugin stderr")]
    fn warn(&self, message: &str) {
        eprintln!("{WARNING_PREFIX}{message}");
    }
}
