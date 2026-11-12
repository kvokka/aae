use tokio::time::Instant;

/// Wall-clock time in milliseconds since the Unix epoch, advanced by tokio's clock.
///
/// The core never reads the system time directly: under `tokio::time::pause()` this is the managed clock of the
/// tests (spec §5.1: timers are deadlines with an exact wake-up time, not polling).
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    base_ms: i64,
    base: Instant,
}

impl Clock {
    /// A clock that reads `now_ms` at this moment; the daemon passes the system time.
    pub fn new(now_ms: i64) -> Self {
        Self {
            base_ms: now_ms,
            base: Instant::now(),
        }
    }

    pub fn now_ms(&self) -> i64 {
        self.base_ms + i64::try_from(self.base.elapsed().as_millis()).unwrap_or(i64::MAX)
    }

    /// The tokio instant at which the wall time `at_ms` is reached (now, if it is already past).
    pub(crate) fn instant_at(&self, at_ms: i64) -> Instant {
        let ahead = u64::try_from(at_ms - self.now_ms()).unwrap_or(0);
        Instant::now() + std::time::Duration::from_millis(ahead)
    }
}
