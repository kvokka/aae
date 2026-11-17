use crate::{parse_rfc3339_ms as parse, rfc3339_ms};

const T0: i64 = 1_800_000_000_000;

#[test]
fn timestamps_are_rfc3339_utc_with_milliseconds() {
    assert_eq!(rfc3339_ms(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(rfc3339_ms(T0), "2027-01-15T08:00:00.000Z");
    assert_eq!(rfc3339_ms(951_782_400_123), "2000-02-29T00:00:00.123Z");
    assert_eq!(rfc3339_ms(-1), "1969-12-31T23:59:59.999Z");
    for ms in [0, T0, 951_782_400_123, -1, 4_102_444_799_999] {
        assert_eq!(parse(&rfc3339_ms(ms)), Some(ms));
    }
    assert_eq!(parse("2027-01-15T08:00:00Z"), Some(T0));
    assert_eq!(parse("2027-01-15T08:00:00.5Z"), Some(T0 + 500));
    for bad in [
        "2027-01-15T08:00:00",
        "2027-13-15T08:00:00Z",
        "2027-01-15T08:00:00.x1Z",
        "2027-01-15 08:00:00Z",
        "+027-01-15T08:00:00Z",
    ] {
        assert_eq!(parse(bad), None, "{bad}");
    }
}
