//! Time text shared by the views. All times are UTC.

use jiff::Timestamp;

/// Month, day and clock of `time`, for example `Oct 03 04:05`.
pub fn short_time(time: Timestamp) -> String {
    time.strftime("%b %d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_time_shows_month_day_and_utc_clock() {
        let at: Timestamp = "2026-10-03T04:05:06Z".parse().unwrap();
        assert_eq!(short_time(at), "Oct 03 04:05");
    }
}
