//! Conversions between [`jiff::Timestamp`] and the protobuf `Timestamp` of `pbjson-types`.
//!
//! `pbjson-types` converts to and from `chrono` only, and neither type is local to this crate, so
//! the conversions are functions.

use jiff::Timestamp;
use pbjson_types::Timestamp as ProtoTimestamp;

const NANOS_PER_SECOND: i32 = 1_000_000_000;

/// The protobuf form of `time`. Protobuf requires `nanos` in `0..1_000_000_000`, `jiff` reports a
/// negative sub-second part before 1970: that is carried into the seconds.
pub fn to_proto(time: Timestamp) -> ProtoTimestamp {
    let (seconds, nanos) = (time.as_second(), time.subsec_nanosecond());
    if nanos < 0 {
        ProtoTimestamp {
            seconds: seconds - 1,
            nanos: nanos + NANOS_PER_SECOND,
        }
    } else {
        ProtoTimestamp { seconds, nanos }
    }
}

/// The time of `proto`; `None` for `nanos` outside `0..1_000_000_000` or seconds out of range.
pub fn from_proto(proto: &ProtoTimestamp) -> Option<Timestamp> {
    if !(0..NANOS_PER_SECOND).contains(&proto.nanos) {
        return None;
    }
    Timestamp::new(proto.seconds, proto.nanos).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_with_nanoseconds_survives_a_round_trip() {
        let time = Timestamp::new(1_700_000_000, 123_456_789).unwrap();
        let proto = to_proto(time);
        assert_eq!((proto.seconds, proto.nanos), (1_700_000_000, 123_456_789));
        assert_eq!(from_proto(&proto), Some(time));
    }

    #[test]
    fn the_unix_epoch_is_zero_seconds_and_nanos() {
        let proto = to_proto(Timestamp::UNIX_EPOCH);
        assert_eq!((proto.seconds, proto.nanos), (0, 0));
        assert_eq!(from_proto(&proto), Some(Timestamp::UNIX_EPOCH));
    }

    #[test]
    fn a_time_before_the_epoch_has_non_negative_nanos() {
        let time = Timestamp::new(-1, -500_000_000).unwrap();
        let proto = to_proto(time);
        assert_eq!((proto.seconds, proto.nanos), (-2, 500_000_000));
        assert_eq!(from_proto(&proto), Some(time));
    }

    #[test]
    fn nanos_out_of_range_give_none() {
        for nanos in [-1, 1_000_000_000] {
            let proto = ProtoTimestamp { seconds: 0, nanos };
            assert_eq!(from_proto(&proto), None, "nanos {nanos}");
        }
    }

    #[test]
    fn seconds_out_of_range_give_none() {
        let proto = ProtoTimestamp {
            seconds: i64::MAX,
            nanos: 0,
        };
        assert_eq!(from_proto(&proto), None);
    }
}
