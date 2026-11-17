//! The event envelope (docs/design/contracts.md 1.1): its schema version and the form of its timestamps.

use serde_json::{Value, json};

/// Version of the event envelope (docs/design/contracts.md 1.1).
pub const SCHEMA: u32 = 1;

pub fn envelope(seq: i64, ts_ms: i64, body: &str) -> Value {
    let mut event = json!({ "schema": SCHEMA, "seq": seq, "ts": rfc3339_ms(ts_ms) });
    if let (Some(out), Ok(Value::Object(fields))) = (event.as_object_mut(), serde_json::from_str::<Value>(body)) {
        out.extend(fields);
    }
    event
}

/// `2026-11-12T14:05:00.123Z` from milliseconds since the Unix epoch (days-to-civil, proleptic Gregorian).
pub fn rfc3339_ms(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s, milli) = (rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{milli:03}Z")
}

/// Milliseconds since the Unix epoch of a UTC time `YYYY-MM-DDTHH:MM:SS[.fff]Z`, the form events carry.
pub fn parse_rfc3339_ms(text: &str) -> Option<i64> {
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    let num = |s: &str| s.parse::<i64>().ok().filter(|_| s.bytes().all(|b| b.is_ascii_digit()));
    let mut d = date.splitn(3, '-').map(num);
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut t = hms.splitn(3, ':').map(num);
    let (h, min, s) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || min > 59 || s > 60 || frac.len() > 9 {
        return None;
    }
    let milli = match frac {
        "" => 0,
        f if f.bytes().all(|b| b.is_ascii_digit()) => num(&format!("{f:0<3}")[..3])?,
        _ => return None,
    };
    // Days from civil (proleptic Gregorian), the inverse of `rfc3339_ms`.
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(days * 86_400_000 + ((h * 60 + min) * 60 + s) * 1000 + milli)
}
