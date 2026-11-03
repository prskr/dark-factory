//! RFC 3339 timestamps as comparable instants. Forgejo writes them in the server's time zone, so
//! strings can't be compared directly.

/// Seconds and nanoseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Instant(i64, u32);

/// Parses `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`; `T` and `Z` may be lowercase.
pub(crate) fn parse(s: &str) -> Option<Instant> {
    let year = digits(s, 0, 4)?;
    let month = digits(s, 5, 2)?;
    let day = digits(s, 8, 2)?;
    let hour = digits(s, 11, 2)?;
    let minute = digits(s, 14, 2)?;
    let second = digits(s, 17, 2)?;
    let separators = [(4, "-"), (7, "-"), (10, "T"), (13, ":"), (16, ":")];
    if separators.iter().any(|(at, sep)| {
        !s.get(*at..=*at)
            .is_some_and(|c| c.eq_ignore_ascii_case(sep))
    }) || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let (nanos, zone) = fraction(s.get(19..)?)?;
    let offset = offset_seconds(zone)?;
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(Instant(seconds - offset, nanos))
}

/// Whether one of `timestamps` is at or after `since`; empty and unreadable ones are skipped.
pub(crate) fn touched_since(timestamps: &[&str], since: Instant) -> bool {
    timestamps
        .iter()
        .filter_map(|t| parse(t))
        .any(|t| t >= since)
}

/// The unsigned decimal number of `len` digits at `at`.
fn digits(s: &str, at: usize, len: usize) -> Option<i64> {
    let part = s.get(at..at + len)?;
    if !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// Nanoseconds of an optional `.fraction` and the rest of `s`.
fn fraction(s: &str) -> Option<(u32, &str)> {
    let Some(rest) = s.strip_prefix('.') else {
        return Some((0, s));
    };
    let len = rest.bytes().take_while(u8::is_ascii_digit).count();
    if len == 0 {
        return None;
    }
    let (fraction, zone) = rest.split_at(len);
    let nanos = format!("{:0<9}", fraction.get(..9).unwrap_or(fraction));
    Some((nanos.parse().ok()?, zone))
}

fn offset_seconds(zone: &str) -> Option<i64> {
    if zone.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let sign = match zone.get(..1)? {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let (hours, minutes) = (digits(zone, 1, 2)?, digits(zone, 4, 2)?);
    if zone.len() != 6 || zone.get(3..4)? != ":" || hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3_600 + minutes * 60))
}

/// Days since 1970-01-01 in the proleptic Gregorian calendar (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_timestamps_count_from_the_epoch() {
        assert_eq!(parse("1970-01-01T00:00:00Z"), Some(Instant(0, 0)));
        assert_eq!(parse("2000-03-01T00:00:00Z"), Some(Instant(951_868_800, 0)));
        assert_eq!(
            parse("2026-11-03t10:20:30.5z"),
            Some(Instant(1_793_701_230, 500_000_000))
        );
    }

    #[test]
    fn offsets_name_the_same_instant() {
        assert_eq!(
            parse("2026-11-01T12:30:00+01:00"),
            parse("2026-11-01T11:30:00Z")
        );
        assert_eq!(
            parse("2026-10-31T23:30:00-01:00"),
            parse("2026-11-01T00:30:00Z")
        );
    }

    #[test]
    fn fractions_order_within_a_second() {
        assert!(parse("2026-11-01T11:00:00.1Z") < parse("2026-11-01T11:00:00.123456789123Z"));
        assert!(parse("2026-11-01T11:00:00Z") < parse("2026-11-01T11:00:00.000000001Z"));
    }

    #[test]
    fn rejects_other_formats() {
        for s in [
            "",
            "yesterday",
            "2026-11-01",
            "2026-11-01T11:00:00",
            "2026-11-01 11:00:00Z",
            "2026-13-01T11:00:00Z",
            "2026-11-01T24:00:00Z",
            "2026-11-01T11:00:00.Z",
            "2026-11-01T11:00:00+0100",
            "2026-11-01T11:00:00+01:00:00",
            "+026-11-01T11:00:00Z",
        ] {
            assert_eq!(parse(s), None, "{s:?}");
        }
    }
}
