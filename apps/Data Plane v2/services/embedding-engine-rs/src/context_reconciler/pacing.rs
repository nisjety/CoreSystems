//! Time and fairness for the reconciler, as small pure pieces: the clock it reads, the circuit
//! breaker, the per-unit cooldown of the work that has no stored schedule, the jitter, and the
//! rotation that makes the organizations take turns.
//!
//! None of it touches the database, the network or the wall clock directly (the clock is a
//! trait, and the tests move a manual one), so every rule here is tested with the real inputs
//! and without a sleep.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::batch::ladder_step;

/// The longest the reconciler waits because a provider asked it to: the same bound the
/// outcomes carry (`context::outcome::MAX_RETRY_AFTER`), so a unit and the loop that serves
/// it never disagree about how long five minutes is.
pub(crate) const MAX_WAIT: Duration = crate::context::outcome::MAX_RETRY_AFTER;

/// The pause after a pass that did work: long enough to leave the provider and the database
/// room, short enough that a backlog drains in minutes and not hours.
pub(crate) const WORKED_PAUSE: Duration = Duration::from_millis(750);

/// How far the pause between passes is jittered: up to a fifth either way. Two reconcilers
/// (a restart that overlaps its predecessor, a second replica) that started together do not
/// stay in step.
pub(crate) const JITTER: f64 = 0.2;

/// A monotonic clock the reconciler reads. A trait so that a test can move it by an hour
/// without sleeping for one.
pub(crate) trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

/// The real one.
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock a test moves by hand.
#[cfg(test)]
pub(crate) struct ManualClock {
    base: Instant,
    elapsed: std::sync::Mutex<Duration>,
}

#[cfg(test)]
impl ManualClock {
    pub(crate) fn new() -> Self {
        Self {
            base: Instant::now(),
            elapsed: std::sync::Mutex::new(Duration::ZERO),
        }
    }

    pub(crate) fn advance(&self, by: Duration) {
        *self
            .elapsed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += by;
    }
}

#[cfg(test)]
impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.base
            + *self
                .elapsed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// What a pass says about the provider, as far as the breaker is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PassHealth {
    /// The pass asked the provider for nothing (nothing to do, or only work that needs no
    /// model): it says nothing about the provider and changes nothing.
    Idle,
    /// The provider answered what it was asked.
    Healthy,
    /// The provider failed every request the pass made (it was throttled, down, timed out).
    ProviderFailed,
    /// A credential or the deployment was refused (`auth`): not a blip. The breaker opens at
    /// once, because asking again with the same credential only repeats the refusal.
    CredentialRefused,
}

/// The circuit breaker: after `threshold` passes in a row in which the provider failed
/// everything, the reconciler stops asking for `pause`, and then lets one pass through to find
/// out (half-open): if it fails the breaker opens again at once, and if it works the count
/// starts over.
pub(crate) struct Breaker {
    threshold: u32,
    pause: Duration,
    failures: u32,
    open_until: Option<Instant>,
}

impl Breaker {
    pub(crate) fn new(threshold: u32, pause: Duration) -> Self {
        Self {
            threshold: threshold.max(1),
            pause,
            failures: 0,
            open_until: None,
        }
    }

    /// How much longer the breaker is open, or `None` when a pass may run.
    pub(crate) fn open_for(&self, now: Instant) -> Option<Duration> {
        self.open_until
            .map(|until| until.saturating_duration_since(now))
            .filter(|left| !left.is_zero())
    }

    pub(crate) fn record(&mut self, now: Instant, health: PassHealth) {
        match health {
            PassHealth::Idle => {}
            PassHealth::Healthy => {
                self.failures = 0;
                self.open_until = None;
            }
            PassHealth::ProviderFailed => {
                self.failures = self.failures.saturating_add(1);
                if self.failures >= self.threshold {
                    self.open_until = Some(now + self.pause);
                }
            }
            PassHealth::CredentialRefused => {
                self.failures = self.threshold;
                self.open_until = Some(now + self.pause);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn failures(&self) -> u32 {
        self.failures
    }
}

/// How long a unit that has no stored schedule waits after a failure.
///
/// The units that fail to be generated carry their schedule in the database (the pipeline
/// writes `context_next_attempt_at` and `context_attempts`). The two kinds of work that are
/// only a re-embed (a `legacy` unit's verification, a vector that does not match its stored
/// context) have nothing to store a failure in without writing the unit again after it was
/// embedded, which would make the stale-embedding detector report a change that is not one. So
/// their backoff is kept here, in the same ladder the database schedule uses (a minute,
/// five, thirty, two hours, half a day), and a restart forgets it, which costs one more try.
pub(crate) struct Cooldown {
    entries: HashMap<String, Entry>,
    capacity: usize,
}

struct Entry {
    attempts: u32,
    until: Instant,
}

impl Cooldown {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    /// `key` failed. It is not asked again before `wait` (the provider's own, when it named one,
    /// else the next step of the ladder). Returns how many times it has failed in a row.
    pub(crate) fn failed(&mut self, key: &str, now: Instant, wait: Option<Duration>) -> u32 {
        let attempts = self
            .entries
            .get(key)
            .map_or(1, |entry| entry.attempts.saturating_add(1));
        // The provider's own wish is capped by the caller (five minutes); the ladder's steps
        // are what they are.
        let wait = wait.unwrap_or_else(|| ladder_step(attempts));
        self.entries.insert(
            key.to_owned(),
            Entry {
                attempts,
                until: now + wait,
            },
        );
        if self.entries.len() > self.capacity {
            // Bounded: the entries that are free again soonest go first, and forgetting one
            // only costs it one more try.
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.until)
                .map(|(key, _)| key.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        attempts
    }

    /// `key` worked.
    pub(crate) fn cleared(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// The keys that must not be asked now.
    pub(crate) fn cooling(&self, now: Instant) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.until > now)
            .map(|(key, _)| key.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn attempts(&self, key: &str) -> u32 {
        self.entries.get(key).map_or(0, |entry| entry.attempts)
    }
}

/// `base`, moved by up to `fraction` either way. `unit` is a number in `[0, 1)` (the caller's
/// source of randomness: a parameter, so that the function is reproducible).
pub(crate) fn jittered(base: Duration, fraction: f64, unit: f64) -> Duration {
    let unit = unit.clamp(0.0, 1.0);
    let factor = 1.0 + fraction.clamp(0.0, 1.0) * (2.0 * unit - 1.0);
    base.mul_f64(factor.max(0.0))
}

/// A number in `[0, 1)` that is different every time it is asked for. The reconciler needs
/// jitter, not secrecy: a random UUID is the entropy the crate already depends on.
pub(crate) fn unit_noise() -> f64 {
    // The top 53 bits of a random 128-bit value, scaled: exact in an f64.
    let bits = (uuid::Uuid::new_v4().as_u128() >> 75) as u64;
    bits as f64 / (1u64 << 53) as f64
}

/// How long to wait before the next pass.
///
/// * A pass that did work is followed by the next after a short pause, so a backlog drains.
/// * A pass that found nothing waits the interval.
/// * Either waits at least as long as a provider asked it to (`retry_after`, capped at five
///   minutes), so a throttled provider is not asked again at once for the next organization's
///   units.
/// * Every wait is jittered.
pub(crate) fn next_pause(
    worked: bool,
    interval: Duration,
    retry_after: Option<Duration>,
    unit: f64,
) -> Duration {
    let base = if worked { WORKED_PAUSE } else { interval };
    let asked = retry_after.unwrap_or_default().min(MAX_WAIT);
    jittered(base.max(asked), JITTER, unit)
}

/// `items` starting at `cursor` (modulo the length) and wrapping round: the organizations take
/// turns at being first, so that with a cap on a pass the same organization is not always the
/// one that is left out.
pub(crate) fn rotated<T: Clone>(items: &[T], cursor: usize) -> Vec<T> {
    if items.is_empty() {
        return Vec::new();
    }
    let start = cursor % items.len();
    items[start..]
        .iter()
        .chain(items[..start].iter())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn the_breaker_opens_after_a_run_of_failed_passes_and_half_opens_after_the_pause() {
        let clock = ManualClock::new();
        let mut breaker = Breaker::new(3, secs(300));
        for _ in 0..2 {
            breaker.record(clock.now(), PassHealth::ProviderFailed);
            assert_eq!(
                breaker.open_for(clock.now()),
                None,
                "two failures are not three"
            );
        }
        breaker.record(clock.now(), PassHealth::ProviderFailed);
        assert_eq!(breaker.open_for(clock.now()), Some(secs(300)));
        clock.advance(secs(299));
        assert_eq!(breaker.open_for(clock.now()), Some(secs(1)));
        clock.advance(secs(2));
        assert_eq!(
            breaker.open_for(clock.now()),
            None,
            "half open: one pass may run"
        );

        // The probe fails: open again at once, without counting to three again.
        breaker.record(clock.now(), PassHealth::ProviderFailed);
        assert_eq!(breaker.open_for(clock.now()), Some(secs(300)));
        clock.advance(secs(301));
        // The probe works: the count starts over.
        breaker.record(clock.now(), PassHealth::Healthy);
        assert_eq!(breaker.failures(), 0);
        breaker.record(clock.now(), PassHealth::ProviderFailed);
        assert_eq!(breaker.open_for(clock.now()), None);
    }

    #[test]
    fn a_pass_that_asked_the_provider_nothing_changes_nothing() {
        let clock = ManualClock::new();
        let mut breaker = Breaker::new(2, secs(60));
        breaker.record(clock.now(), PassHealth::ProviderFailed);
        breaker.record(clock.now(), PassHealth::Idle);
        breaker.record(clock.now(), PassHealth::Idle);
        assert_eq!(
            breaker.failures(),
            1,
            "an idle pass neither resets nor adds"
        );
        breaker.record(clock.now(), PassHealth::ProviderFailed);
        assert_eq!(breaker.open_for(clock.now()), Some(secs(60)));
        breaker.record(clock.now(), PassHealth::Idle);
        assert_eq!(
            breaker.open_for(clock.now()),
            Some(secs(60)),
            "an idle pass does not close it"
        );
    }

    #[test]
    fn a_refused_credential_opens_the_breaker_at_once() {
        let clock = ManualClock::new();
        let mut breaker = Breaker::new(5, secs(120));
        breaker.record(clock.now(), PassHealth::CredentialRefused);
        assert_eq!(breaker.open_for(clock.now()), Some(secs(120)));
        clock.advance(secs(121));
        assert_eq!(breaker.open_for(clock.now()), None);
        // After the pause, one more refusal is enough: the credential did not heal.
        breaker.record(clock.now(), PassHealth::CredentialRefused);
        assert_eq!(breaker.open_for(clock.now()), Some(secs(120)));
    }

    #[test]
    fn a_threshold_of_zero_is_one() {
        let clock = ManualClock::new();
        let mut breaker = Breaker::new(0, secs(10));
        breaker.record(clock.now(), PassHealth::ProviderFailed);
        assert!(breaker.open_for(clock.now()).is_some());
    }

    #[test]
    fn a_failing_unit_climbs_the_same_ladder_the_database_schedule_uses() {
        let clock = ManualClock::new();
        let mut cooldown = Cooldown::new(10);
        let mut waited = Vec::new();
        for _ in 0..7 {
            let attempts = cooldown.failed("kid", clock.now(), None);
            let until = cooldown.entries["kid"].until;
            waited.push((attempts, until.duration_since(clock.now()).as_secs()));
            clock.advance(secs(50_000));
        }
        assert_eq!(
            waited,
            [
                (1, 60),
                (2, 300),
                (3, 1_800),
                (4, 7_200),
                (5, 43_200),
                (6, 43_200),
                (7, 43_200)
            ]
        );
    }

    #[test]
    fn a_unit_that_cools_is_excluded_until_its_wait_is_over_and_a_success_forgets_it() {
        let clock = ManualClock::new();
        let mut cooldown = Cooldown::new(10);
        cooldown.failed("a", clock.now(), Some(secs(30)));
        cooldown.failed("b", clock.now(), None);
        let mut cooling = cooldown.cooling(clock.now());
        cooling.sort();
        assert_eq!(cooling, ["a", "b"]);
        clock.advance(secs(31));
        assert_eq!(cooldown.cooling(clock.now()), ["b"]);
        assert_eq!(
            cooldown.attempts("a"),
            1,
            "the history survives the wait, so the next failure is the second"
        );
        assert_eq!(cooldown.failed("a", clock.now(), None), 2);
        cooldown.cleared("a");
        assert_eq!(cooldown.attempts("a"), 0);
        assert_eq!(cooldown.cooling(clock.now()), ["b"]);
    }

    #[test]
    fn the_cooldown_is_bounded_and_forgets_the_entry_that_is_free_soonest() {
        let clock = ManualClock::new();
        let mut cooldown = Cooldown::new(2);
        cooldown.failed("soon", clock.now(), Some(secs(10)));
        cooldown.failed("later", clock.now(), Some(secs(1_000)));
        cooldown.failed("latest", clock.now(), Some(secs(2_000)));
        let mut cooling = cooldown.cooling(clock.now());
        cooling.sort();
        assert_eq!(cooling, ["later", "latest"]);
    }

    #[test]
    fn jitter_stays_inside_its_fraction_and_is_reproducible() {
        let base = secs(100);
        assert_eq!(jittered(base, 0.2, 0.5), base);
        assert_eq!(jittered(base, 0.2, 0.0), secs(80));
        assert_eq!(jittered(base, 0.2, 1.0), secs(120));
        assert_eq!(jittered(base, 0.0, 0.9), base);
        // Out-of-range inputs are clamped, never a panic or a negative wait.
        assert_eq!(jittered(base, 5.0, 0.0), Duration::ZERO);
        assert_eq!(jittered(base, 0.2, 7.0), secs(120));
        for _ in 0..1_000 {
            let noise = unit_noise();
            assert!((0.0..1.0).contains(&noise), "{noise}");
        }
    }

    #[test]
    fn the_pause_between_passes_follows_the_work_and_what_the_provider_asked() {
        let interval = secs(30);
        let exact = |worked, asked| next_pause(worked, interval, asked, 0.5);
        assert_eq!(exact(false, None), interval);
        assert_eq!(exact(true, None), WORKED_PAUSE);
        // A provider that asked for 20 seconds is not asked again sooner, work or no work.
        assert_eq!(exact(true, Some(secs(20))), secs(20));
        // And never for more than five minutes, whatever the header said.
        assert_eq!(exact(true, Some(secs(52_260))), MAX_WAIT);
        assert_eq!(MAX_WAIT, secs(300));
        // Jittered inside a fifth.
        let low = next_pause(false, interval, None, 0.0);
        let high = next_pause(false, interval, None, 1.0);
        assert_eq!((low, high), (secs(24), secs(36)));
    }

    #[test]
    fn organizations_take_turns_at_being_first() {
        let orgs = ["a", "b", "c"];
        assert_eq!(rotated(&orgs, 0), ["a", "b", "c"]);
        assert_eq!(rotated(&orgs, 1), ["b", "c", "a"]);
        assert_eq!(rotated(&orgs, 2), ["c", "a", "b"]);
        assert_eq!(rotated(&orgs, 3), ["a", "b", "c"], "the cursor wraps");
        assert_eq!(rotated(&orgs, 100), ["b", "c", "a"]);
        assert!(rotated::<&str>(&[], 5).is_empty());
    }
}
