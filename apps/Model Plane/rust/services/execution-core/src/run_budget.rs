//! The time side of one governed run's budget, and the guard that keeps a run
//! on the ChatGPT subscription inside the lifetime of its own credentials.
//!
//! The round budget (`RunAgentRequest.max_rounds`, resolved on the gateway by
//! `run_budget::resolve_run_budget`) bounds how many inference rounds a run may
//! drive. It does not bound how long they take. On the subscription a round is a
//! Codex process spawn of tens of seconds, and the credentials a run carries are
//! short lived: the delegated bearers default to 300 s. A run that starts its
//! ninth round at second 290 has an `Unauthenticated` inference call ahead of
//! it and, worse, a `finalize` that can no longer write its own answer.
//!
//! [`RunDeadline`] is the single rule for that, resolved once per run by
//! [`RunDeadline::for_run`] and copied into every delegated child:
//!
//! * the wall clock: `EXECUTION_RUN_WALL_CLOCK_SECS` seconds from the start of
//!   the run. `0`, unset, empty and unparsable all mean off. It is an operator
//!   setting for every run this process serves, not a subscription-only one;
//! * the credential guard: for a run pinned to the subscription, the earliest
//!   `exp` of the bearers the run carries, less [`CREDENTIAL_MARGIN`]. It makes
//!   the safe wall clock follow the stack's real token lifetime instead of a
//!   number someone has to remember to keep below `PLANE_TOKEN_TTL_*`. It is not
//!   applied to an unpinned run, whose rounds take seconds and whose behaviour
//!   this change leaves alone.
//!
//! The tighter of the two wins. What the deadline means is the same either way:
//! no NEW round starts once it has passed. A round already in flight finishes,
//! tool calls included, and the run then ends honestly with what it has: it
//! reports that it stopped at its time limit (it does not claim an answer it
//! never produced) and finalizes through the ordinary path, so the thread and
//! the managed receipt are written while the credentials still work.
//!
//! Everything here is pure: the environment value, the bearers and the clock
//! arrive as arguments, so a test never touches the process environment or
//! sleeps to move time.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;

/// Seconds the run may take, from the start of the run. `0` or unset: off.
pub(crate) const WALL_CLOCK_ENV: &str = "EXECUTION_RUN_WALL_CLOCK_SECS";

/// Upper bound on [`WALL_CLOCK_ENV`]. A value past a day is a typo, and an
/// `Instant` that far out is also not guaranteed to be representable.
pub(crate) const MAX_WALL_CLOCK_SECS: u64 = 86_400;

/// How long before its earliest credential expires a pinned run stops starting
/// rounds. One round on the subscription is a Codex process spawn (about 35 s
/// measured), and the answer still has to be written afterwards with the same
/// credentials, so the margin has to cover a round and a finalize.
pub(crate) const CREDENTIAL_MARGIN: Duration = Duration::from_secs(60);

/// Which limit a [`RunDeadline`] is enforcing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeadlineCause {
    /// [`WALL_CLOCK_ENV`].
    WallClock,
    /// The earliest credential's expiry, less [`CREDENTIAL_MARGIN`].
    Credential,
}

impl DeadlineCause {
    /// A stable, content-free name for logs.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WallClock => "wall_clock",
            Self::Credential => "credential_expiry",
        }
    }
}

/// The instant after which a run starts no new round, if it has one. `Copy` so
/// every delegated child's context holds the parent's, not a second clock.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunDeadline {
    limit: Option<(Instant, DeadlineCause)>,
}

impl RunDeadline {
    /// No limit: what an environment with the wall clock off and a run that is
    /// not pinned means, and what every run had before this module. Production
    /// never builds it directly ([`RunDeadline::for_run`] yields it); tests that
    /// drive the loop without a budget do.
    #[cfg(test)]
    pub(crate) const NONE: Self = Self { limit: None };

    /// The deadline of a run that started at `started`.
    ///
    /// * `wall_clock_raw`: the value of [`WALL_CLOCK_ENV`], if set;
    /// * `guard_credentials`: whether the credential guard applies (the run is
    ///   pinned to the subscription);
    /// * `bearers`: the compact JWTs the run carries (inference, session, data
    ///   plane, sandbox); an absent or unreadable one constrains nothing;
    /// * `now_epoch_secs`: the wall-clock time, to compare against a bearer's
    ///   `exp` (see [`now_epoch_secs`]).
    #[must_use]
    pub(crate) fn for_run(
        started: Instant,
        wall_clock_raw: Option<&str>,
        guard_credentials: bool,
        bearers: &[Option<&str>],
        now_epoch_secs: u64,
    ) -> Self {
        let wall_clock = parse_wall_clock(wall_clock_raw)
            .and_then(|limit| started.checked_add(limit))
            .map(|at| (at, DeadlineCause::WallClock));
        let credential = guard_credentials
            .then(|| credential_headroom(earliest_bearer_expiry(bearers), now_epoch_secs))
            .flatten()
            .and_then(|headroom| started.checked_add(headroom))
            .map(|at| (at, DeadlineCause::Credential));
        // The tighter limit wins; on a tie the wall clock, the operator's own
        // setting, is the one named.
        let limit = match (wall_clock, credential) {
            (Some(wall), Some(cred)) => Some(if cred.0 < wall.0 { cred } else { wall }),
            (one, other) => one.or(other),
        };
        Self { limit }
    }

    /// A wall-clock deadline `limit` after `started`, for a loop test that needs
    /// a limit shorter than the whole seconds the variable takes.
    #[cfg(test)]
    pub(crate) fn wall_clock_after(started: Instant, limit: Duration) -> Self {
        Self {
            limit: Some((started + limit, DeadlineCause::WallClock)),
        }
    }

    /// Which limit has been reached at `now`, if any.
    #[must_use]
    pub(crate) fn expired_at(self, now: Instant) -> Option<DeadlineCause> {
        self.limit
            .and_then(|(at, cause)| (now >= at).then_some(cause))
    }

    /// Which limit has been reached right now, if any.
    #[must_use]
    pub(crate) fn expired(self) -> Option<DeadlineCause> {
        self.expired_at(Instant::now())
    }

    /// Time left at `now`, `None` when the run has no limit. For the run's
    /// start-of-run log line.
    #[must_use]
    pub(crate) fn remaining_at(self, now: Instant) -> Option<Duration> {
        self.limit.map(|(at, _)| at.saturating_duration_since(now))
    }
}

/// The wall-clock limit from the raw value of [`WALL_CLOCK_ENV`]. `None` is off:
/// unset, empty, `0` and anything that is not a whole number of seconds.
/// Clamped to [`MAX_WALL_CLOCK_SECS`].
#[must_use]
pub(crate) fn parse_wall_clock(raw: Option<&str>) -> Option<Duration> {
    raw.map(str::trim)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|&secs| secs > 0)
        .map(|secs| Duration::from_secs(secs.min(MAX_WALL_CLOCK_SECS)))
}

/// The `exp` claim (seconds since the epoch) of a compact JWT, read without
/// verifying anything: every bearer a run carries was verified at the gRPC
/// boundary, and this only reads WHEN it stops being good. `None` for anything
/// that is not a JWT with a numeric `exp`.
#[must_use]
pub(crate) fn bearer_expiry(bearer: &str) -> Option<u64> {
    let payload = bearer.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()?
        .get("exp")?
        .as_u64()
}

/// The earliest `exp` among the bearers that carry a readable one: the
/// credential that will fail first is the one that bounds the run.
#[must_use]
pub(crate) fn earliest_bearer_expiry(bearers: &[Option<&str>]) -> Option<u64> {
    bearers
        .iter()
        .flatten()
        .filter_map(|bearer| bearer_expiry(bearer))
        .min()
}

/// How long, from `now_epoch_secs`, a run may keep starting rounds before the
/// credential guard stops it: until [`CREDENTIAL_MARGIN`] before `earliest_exp`.
/// Zero when that moment has passed. `None` when no bearer had a readable
/// expiry (nothing to guard).
#[must_use]
pub(crate) fn credential_headroom(earliest_exp: Option<u64>, now_epoch_secs: u64) -> Option<Duration> {
    earliest_exp.map(|exp| {
        Duration::from_secs(exp.saturating_sub(now_epoch_secs)).saturating_sub(CREDENTIAL_MARGIN)
    })
}

/// Seconds since the Unix epoch, for comparing against a bearer's `exp`.
#[must_use]
pub(crate) fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A compact JWT whose payload is `payload`. The header and signature are
/// placeholders: nothing in this module verifies them, and nothing that accepts
/// one on a real boundary would accept this one.
#[cfg(test)]
pub(crate) fn unsigned_test_token(payload: &serde_json::Value) -> String {
    let enc = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    format!(
        "{}.{}.{}",
        enc(br#"{"alg":"RS256"}"#),
        enc(payload.to_string().as_bytes()),
        enc(b"signature")
    )
}

/// [`unsigned_test_token`] expiring at `exp` (seconds since the epoch).
#[cfg(test)]
pub(crate) fn unsigned_test_token_expiring_at(exp: u64) -> String {
    unsigned_test_token(&serde_json::json!({ "sub": "user-1", "exp": exp }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_with(payload: &serde_json::Value) -> String {
        unsigned_test_token(payload)
    }

    fn token_expiring_at(exp: u64) -> String {
        unsigned_test_token_expiring_at(exp)
    }

    const NOW: u64 = 1_800_000_000;

    #[test]
    fn the_wall_clock_is_off_unless_it_is_a_positive_whole_number_of_seconds() {
        for off in [
            None,
            Some(""),
            Some("  "),
            Some("0"),
            Some("-5"),
            Some("1.5"),
            Some("abc"),
            Some("240s"),
        ] {
            assert_eq!(parse_wall_clock(off), None, "{off:?} must leave the clock off");
        }
        assert_eq!(parse_wall_clock(Some("240")), Some(Duration::from_secs(240)));
        assert_eq!(parse_wall_clock(Some(" 240 ")), Some(Duration::from_secs(240)));
        assert_eq!(parse_wall_clock(Some("1")), Some(Duration::from_secs(1)));
    }

    #[test]
    fn an_absurd_wall_clock_is_clamped_not_overflowed() {
        assert_eq!(
            parse_wall_clock(Some("99999999999")),
            Some(Duration::from_secs(MAX_WALL_CLOCK_SECS))
        );
        // And an Instant that far out is representable, so `for_run` keeps it.
        let deadline =
            RunDeadline::for_run(Instant::now(), Some("18446744073709551615"), false, &[], NOW);
        assert!(deadline.expired().is_none());
        assert!(deadline.remaining_at(Instant::now()).is_some());
    }

    #[test]
    fn a_bearers_expiry_is_read_from_its_payload() {
        assert_eq!(bearer_expiry(&token_expiring_at(NOW + 300)), Some(NOW + 300));
        // Padding is tolerated, as some issuers leave it in.
        let padded = {
            let token = token_expiring_at(NOW + 300);
            let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
            let padding = (4 - parts[1].len() % 4) % 4;
            parts[1].push_str(&"=".repeat(padding));
            parts.join(".")
        };
        assert_eq!(bearer_expiry(&padded), Some(NOW + 300));
    }

    #[test]
    fn anything_that_is_not_a_jwt_with_a_numeric_exp_has_no_expiry() {
        for not_a_token in [
            "",
            "inference-token",
            "a.b",
            "a.%%%.c",
            "a.bm90LWpzb24.c",
        ] {
            assert_eq!(bearer_expiry(not_a_token), None, "{not_a_token:?}");
        }
        let string_exp = token_with(&serde_json::json!({ "exp": "1800000300" }));
        assert_eq!(bearer_expiry(&string_exp), None, "exp must be a number");
        let no_exp = token_with(&serde_json::json!({ "sub": "user-1" }));
        assert_eq!(bearer_expiry(&no_exp), None);
    }

    #[test]
    fn the_credential_that_fails_first_bounds_the_run() {
        let early = token_expiring_at(NOW + 100);
        let late = token_expiring_at(NOW + 300);
        assert_eq!(
            earliest_bearer_expiry(&[Some(&late), Some(&early), None, Some("opaque")]),
            Some(NOW + 100)
        );
        assert_eq!(earliest_bearer_expiry(&[None, Some("opaque")]), None);
        assert_eq!(earliest_bearer_expiry(&[]), None);
    }

    #[test]
    fn the_credential_headroom_stops_a_margin_before_expiry() {
        assert_eq!(
            credential_headroom(Some(NOW + 300), NOW),
            Some(Duration::from_secs(240)),
            "a default 300 s bearer leaves 240 s to start rounds in"
        );
        assert_eq!(
            credential_headroom(Some(NOW + 30), NOW),
            Some(Duration::ZERO),
            "inside the margin already: no round may start"
        );
        assert_eq!(
            credential_headroom(Some(NOW - 5), NOW),
            Some(Duration::ZERO),
            "an expired credential saturates at zero"
        );
        assert_eq!(credential_headroom(None, NOW), None);
    }

    #[test]
    fn with_the_wall_clock_off_and_nothing_to_guard_a_run_has_no_deadline() {
        let started = Instant::now();
        let far_later = started + Duration::from_secs(10_000_000);
        for deadline in [
            RunDeadline::NONE,
            RunDeadline::for_run(started, None, false, &[], NOW),
            RunDeadline::for_run(started, Some("0"), true, &[Some("opaque")], NOW),
            // A pinned run whose bearers are not readable JWTs has nothing to guard.
            RunDeadline::for_run(started, None, true, &[None, Some("inference-token")], NOW),
        ] {
            assert_eq!(deadline.expired_at(far_later), None);
            assert!(deadline.remaining_at(started).is_none());
        }
    }

    #[test]
    fn the_wall_clock_expires_at_its_limit_and_not_before() {
        let started = Instant::now();
        let deadline = RunDeadline::for_run(started, Some("240"), false, &[], NOW);
        assert_eq!(deadline.expired_at(started), None);
        assert_eq!(deadline.expired_at(started + Duration::from_secs(239)), None);
        assert_eq!(
            deadline.expired_at(started + Duration::from_secs(240)),
            Some(DeadlineCause::WallClock)
        );
        assert_eq!(
            deadline.expired_at(started + Duration::from_secs(900)),
            Some(DeadlineCause::WallClock)
        );
        assert_eq!(
            deadline.remaining_at(started + Duration::from_secs(100)),
            Some(Duration::from_secs(140))
        );
    }

    #[test]
    fn the_wall_clock_applies_to_a_run_that_is_not_pinned() {
        // The operator set it for the process; it is not a subscription-only knob.
        let started = Instant::now();
        let deadline = RunDeadline::for_run(started, Some("240"), false, &[], NOW);
        assert_eq!(
            deadline.expired_at(started + Duration::from_secs(300)),
            Some(DeadlineCause::WallClock)
        );
    }

    #[test]
    fn the_credential_guard_follows_the_real_token_lifetime() {
        // A stack with a 120 s token override: no wall clock configured, and the
        // guard still stops new rounds 60 s into the run.
        let started = Instant::now();
        let token = token_expiring_at(NOW + 120);
        let deadline = RunDeadline::for_run(started, None, true, &[Some(&token)], NOW);
        assert_eq!(deadline.expired_at(started + Duration::from_secs(59)), None);
        assert_eq!(
            deadline.expired_at(started + Duration::from_secs(60)),
            Some(DeadlineCause::Credential)
        );
    }

    #[test]
    fn the_credential_guard_is_not_applied_to_a_run_that_is_not_pinned() {
        let started = Instant::now();
        let token = token_expiring_at(NOW + 30);
        let deadline = RunDeadline::for_run(started, None, false, &[Some(&token)], NOW);
        assert_eq!(
            deadline.expired_at(started + Duration::from_secs(1000)),
            None,
            "an unpinned run's behaviour is unchanged by the guard"
        );
    }

    #[test]
    fn the_tighter_limit_wins_and_is_the_one_named() {
        let started = Instant::now();
        let token = token_expiring_at(NOW + 300); // credential limit: 240 s
        let at = |secs| started + Duration::from_secs(secs);

        // Wall clock tighter than the credential.
        let wall_first = RunDeadline::for_run(started, Some("100"), true, &[Some(&token)], NOW);
        assert_eq!(wall_first.expired_at(at(100)), Some(DeadlineCause::WallClock));
        assert_eq!(wall_first.expired_at(at(99)), None);

        // Credential tighter than the wall clock (a 600 s wall clock against a
        // 300 s token is the misconfiguration the guard exists for).
        let credential_first =
            RunDeadline::for_run(started, Some("600"), true, &[Some(&token)], NOW);
        assert_eq!(credential_first.expired_at(at(239)), None);
        assert_eq!(
            credential_first.expired_at(at(240)),
            Some(DeadlineCause::Credential)
        );

        // A tie names the operator's wall clock.
        let tie = RunDeadline::for_run(started, Some("240"), true, &[Some(&token)], NOW);
        assert_eq!(tie.expired_at(at(240)), Some(DeadlineCause::WallClock));
    }

    #[test]
    fn a_pinned_run_whose_credential_is_inside_the_margin_never_starts_a_round() {
        let started = Instant::now();
        let token = token_expiring_at(NOW + 45);
        let deadline = RunDeadline::for_run(started, None, true, &[Some(&token)], NOW);
        assert_eq!(deadline.expired_at(started), Some(DeadlineCause::Credential));
    }

    #[test]
    fn the_cause_names_are_stable() {
        assert_eq!(DeadlineCause::WallClock.as_str(), "wall_clock");
        assert_eq!(DeadlineCause::Credential.as_str(), "credential_expiry");
        assert_eq!(WALL_CLOCK_ENV, "EXECUTION_RUN_WALL_CLOCK_SECS");
    }
}
