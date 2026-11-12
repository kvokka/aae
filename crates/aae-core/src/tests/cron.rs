use crate::cron::Cron;

const FRI_13_0259_30: i64 = 1_794_538_770_000;
const FRI_13_0300: i64 = 1_794_538_800_000;
const SAT_14_0300: i64 = 1_794_625_200_000;

fn next(expr: &str, after: i64) -> Option<i64> {
    Cron::parse(expr).unwrap().next_after(after)
}

#[test]
fn the_next_minute_is_strictly_after_the_given_time() {
    assert_eq!(next("0 3 * * *", FRI_13_0259_30), Some(FRI_13_0300));
    assert_eq!(next("0 3 * * *", FRI_13_0300), Some(SAT_14_0300));
    // 2026-11-13T10:07Z -> 10:15Z.
    assert_eq!(next("*/15 * * * *", 1_794_564_420_000), Some(1_794_564_900_000));
}

#[test]
fn days_of_month_and_week_follow_vixie_cron() {
    // From Saturday 2026-11-14T12:00Z the next weekday 09:00 is Monday 2026-11-16.
    assert_eq!(next("0 9 * * mon-fri", 1_794_657_600_000), Some(1_794_819_600_000));
    // Both restricted: the 13th or any Friday; from Friday 2026-11-13T00:00Z the next is Friday 2026-11-20.
    assert_eq!(next("0 0 13 * 5", 1_794_528_000_000), Some(1_795_132_800_000));
    // 7 is Sunday: 2026-11-14 (Saturday) -> Sunday 2026-11-15T00:00Z.
    assert_eq!(next("0 0 * * 7", SAT_14_0300), Some(SAT_14_0300 + 21 * 3_600_000));
    // 2026-03-01 -> the next 29 February is in 2028.
    assert_eq!(next("0 0 29 feb *", 1_772_323_200_000), Some(1_835_395_200_000));
    assert_eq!(next("0 0 30 2 *", 0), None);
}

#[test]
fn malformed_fields_are_rejected() {
    for bad in [
        "* * * *",
        "60 * * * *",
        "*/0 * * * *",
        "5-1 * * * *",
        "0 0 0 * *",
        "0 0 * foo *",
        "x * * * *",
    ] {
        assert!(Cron::parse(bad).is_err(), "{bad}");
    }
    assert!(Cron::parse("0,30 1-5/2 1 JAN-mar */2").is_ok());
}
