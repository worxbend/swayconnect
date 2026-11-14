//! The continuous cap of keep-awake (`docs/PROTOCOL.md` 17.8 rule 5). Pure: every method
//! takes the time.
//!
//! On-time counts across off periods shorter than `cooldown`; only an off period of at least
//! `cooldown` resets the count. A replace keeps the running on-period, so it resets nothing.

use std::time::Duration;

use tokio::time::Instant;

#[derive(Debug, Clone)]
pub(crate) struct Budget {
    max_continuous: Duration,
    cooldown: Duration,
    /// On-time of the ended on-periods since the last break.
    counted: Duration,
    /// Start of the running on-period.
    on_since: Option<Instant>,
    /// End of the last on-period; `None` before the first one.
    off_since: Option<Instant>,
}

impl Budget {
    pub(crate) fn new(max_continuous: Duration, cooldown: Duration) -> Budget {
        Budget {
            max_continuous,
            cooldown,
            counted: Duration::ZERO,
            on_since: None,
            off_since: None,
        }
    }

    /// Whole seconds of on-time left before the cap. A sleep child takes whole seconds, so
    /// less than one second left is no budget.
    pub(crate) fn seconds_left(&self, now: Instant) -> u64 {
        self.left(now).as_secs()
    }

    /// While the cap applies (no whole second left): the time until `on` is allowed again.
    pub(crate) fn retry_after(&self, now: Instant) -> Option<Duration> {
        if self.seconds_left(now) > 0 {
            return None;
        }
        match (self.on_since, self.off_since) {
            (Some(_), _) => Some(self.left(now) + self.cooldown),
            (None, Some(off_since)) => Some(
                self.cooldown
                    .saturating_sub(now.saturating_duration_since(off_since)),
            ),
            // Never on: the whole budget is left, so this is unreachable for a valid config.
            (None, None) => None,
        }
    }

    /// An on-period starts, unless one runs (a replace).
    pub(crate) fn start(&mut self, now: Instant) {
        if self.on_since.is_none() {
            self.counted = self.count(now);
            self.on_since = Some(now);
        }
    }

    /// The running on-period ends.
    pub(crate) fn stop(&mut self, now: Instant) {
        if let Some(on_since) = self.on_since.take() {
            self.counted += now.saturating_duration_since(on_since);
            self.off_since = Some(now);
        }
    }

    /// On-time since the last break.
    fn count(&self, now: Instant) -> Duration {
        match (self.on_since, self.off_since) {
            (Some(on_since), _) => self.counted + now.saturating_duration_since(on_since),
            (None, Some(off_since))
                if now.saturating_duration_since(off_since) >= self.cooldown =>
            {
                Duration::ZERO
            }
            (None, _) => self.counted,
        }
    }

    fn left(&self, now: Instant) -> Duration {
        self.max_continuous.saturating_sub(self.count(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Duration = Duration::from_secs(14_400);
    const COOLDOWN: Duration = Duration::from_secs(900);

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn a_fresh_budget_is_the_whole_cap() {
        let now = Instant::now();
        let budget = Budget::new(CAP, COOLDOWN);
        assert_eq!(budget.seconds_left(now), 14_400);
        assert_eq!(budget.retry_after(now), None);
    }

    #[test]
    fn on_time_counts_across_a_short_break_and_a_replace() {
        let t0 = Instant::now();
        let mut budget = Budget::new(CAP, COOLDOWN);
        budget.start(t0);
        budget.start(t0 + secs(100));
        budget.stop(t0 + secs(1_000));
        assert_eq!(budget.seconds_left(t0 + secs(1_899)), 13_400);
        budget.start(t0 + secs(1_899));
        assert_eq!(budget.seconds_left(t0 + secs(1_999)), 13_300);
    }

    #[test]
    fn a_break_of_the_cooldown_resets_the_count() {
        let t0 = Instant::now();
        let mut budget = Budget::new(CAP, COOLDOWN);
        budget.start(t0);
        budget.stop(t0 + CAP);
        assert_eq!(budget.seconds_left(t0 + CAP), 0);
        assert_eq!(budget.retry_after(t0 + CAP + secs(60)), Some(secs(840)));
        let after_break = t0 + CAP + COOLDOWN;
        assert_eq!(budget.seconds_left(after_break), 14_400);
        assert_eq!(budget.retry_after(after_break), None);
        budget.start(after_break);
        assert_eq!(budget.seconds_left(after_break + secs(10)), 14_390);
    }

    #[test]
    fn under_one_second_left_is_no_budget() {
        let t0 = Instant::now();
        let mut budget = Budget::new(CAP, COOLDOWN);
        budget.start(t0);
        let almost = t0 + CAP - Duration::from_millis(400);
        assert_eq!(budget.seconds_left(almost), 0);
        assert_eq!(
            budget.retry_after(almost),
            Some(Duration::from_millis(400) + COOLDOWN)
        );
    }
}
