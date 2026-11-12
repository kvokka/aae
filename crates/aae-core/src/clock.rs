use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

/// Wall-clock time of the core in milliseconds since the Unix epoch.
///
/// `system()` reads the system time on every call, so time spent with the machine asleep and NTP corrections are
/// seen by the next `now_ms`. `managed(start)` starts at `start` and advances with tokio's clock: under
/// `tokio::time::pause()` that is the managed clock of the deterministic checks (spec §15, 5b). Deadlines sleep on
/// tokio's monotonic timer either way; one that overslept a suspend is caught by the periodic resync (spec §5.1).
#[derive(Clone, Copy, Debug)]
pub struct Clock(Mode);

#[derive(Clone, Copy, Debug)]
enum Mode {
    System,
    Managed { start_ms: i64, start: Instant },
}

impl Clock {
    pub fn system() -> Self {
        Self(Mode::System)
    }

    pub fn managed(start_ms: i64) -> Self {
        Self(Mode::Managed {
            start_ms,
            start: Instant::now(),
        })
    }

    pub fn now_ms(&self) -> i64 {
        match self.0 {
            Mode::System => match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(since) => millis(since),
                Err(before) => -millis(before.duration()),
            },
            Mode::Managed { start_ms, start } => start_ms + millis(start.elapsed()),
        }
    }

    /// The tokio instant at which the wall time `at_ms` is reached (now, if it is already past).
    pub(crate) fn instant_at(&self, at_ms: i64) -> Instant {
        let ahead = u64::try_from(at_ms - self.now_ms()).unwrap_or(0);
        Instant::now() + Duration::from_millis(ahead)
    }
}

fn millis(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}
