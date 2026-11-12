//! Five-field cron expressions of schedule triggers (spec §9.5), in UTC: minute, hour, day of month, month, day of
//! week. A field is `*`, a value, a range `a-b`, any of them with a step `/n`, or a comma list of those; months and
//! days of week also take three-letter English names, and day of week 7 is Sunday like 0.

const RANGES: [(u32, u32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const MINUTE_MS: i64 = 60_000;
const DAY_MINUTES: i64 = 1440;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cron {
    /// One bit per allowed value of each field, in field order.
    fields: [u64; 5],
    /// Day of month and day of week are both restricted: a day matches either, as in Vixie cron.
    either_day: bool,
}

impl Cron {
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(format!("`{text}` is not five cron fields"));
        }
        let mut fields = [0; 5];
        for (i, part) in parts.iter().enumerate() {
            fields[i] = field(part, i).ok_or_else(|| format!("`{text}`: bad cron field `{part}`"))?;
        }
        fields[4] |= fields[4] >> 7 & 1;
        let either_day = !parts[2].starts_with('*') && !parts[4].starts_with('*');
        Ok(Self { fields, either_day })
    }

    /// The first matching minute strictly after `after_ms` (milliseconds since the Unix epoch); `None` when none
    /// comes within 28 years, the period of the calendar (`0 0 30 2 *`).
    pub fn next_after(&self, after_ms: i64) -> Option<i64> {
        let first = after_ms.div_euclid(MINUTE_MS) + 1;
        let (day0, minute0) = (first.div_euclid(DAY_MINUTES), first.rem_euclid(DAY_MINUTES));
        (day0..day0 + 366 * 28)
            .filter(|day| self.day_matches(*day))
            .find_map(|day| {
                let from = if day == day0 { minute0 } else { 0 };
                let minute = (from..DAY_MINUTES).find(|m| self.has(0, m % 60) && self.has(1, m / 60))?;
                Some((day * DAY_MINUTES + minute) * MINUTE_MS)
            })
    }

    fn has(&self, field: usize, value: i64) -> bool {
        self.fields[field] >> value & 1 == 1
    }

    fn day_matches(&self, day: i64) -> bool {
        let (month, dom) = month_day(day);
        // 1970-01-01 was a Thursday.
        let (by_date, by_weekday) = (self.has(2, dom), self.has(4, (day + 4).rem_euclid(7)));
        self.has(3, month)
            && match self.either_day {
                true => by_date || by_weekday,
                false => by_date && by_weekday,
            }
    }
}

fn field(text: &str, i: usize) -> Option<u64> {
    let (lo, hi) = RANGES[i];
    let mut bits = 0;
    for item in text.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (range, Some(step.parse::<usize>().ok().filter(|s| *s > 0)?)),
            None => (item, None),
        };
        let (from, to) = match (range, range.split_once('-')) {
            ("*", _) => (lo, hi),
            (_, Some((a, b))) => (value(a, i)?, value(b, i)?),
            // `a/n` runs from `a` to the end of the field.
            _ => (value(range, i)?, if step.is_some() { hi } else { value(range, i)? }),
        };
        if from < lo || to > hi || from > to {
            return None;
        }
        bits |= (from..=to).step_by(step.unwrap_or(1)).fold(0_u64, |b, v| b | 1 << v);
    }
    Some(bits)
}

fn value(text: &str, i: usize) -> Option<u32> {
    let lower = text.to_ascii_lowercase();
    match i {
        3 => MONTHS.iter().position(|m| *m == lower).map(|p| p as u32 + 1),
        4 => DAYS.iter().position(|d| *d == lower).map(|p| p as u32),
        _ => None,
    }
    .or_else(|| text.parse().ok())
}

/// Month (1–12) and day of month of a day since the Unix epoch (H. Hinnant's `civil_from_days`).
fn month_day(days: i64) -> (i64, i64) {
    let doe = (days + 719_468).rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (month, doy - (153 * mp + 2) / 5 + 1)
}
