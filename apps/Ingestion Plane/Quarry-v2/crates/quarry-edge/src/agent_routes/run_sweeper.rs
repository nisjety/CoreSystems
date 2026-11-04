use super::*;

use super::lifecycle::{
    HUMAN_BROWSE_ABSOLUTE_TTL_S, HUMAN_BROWSE_IDLE_TTL_S, HUMAN_BROWSE_TAKEOVER_IDLE_TTL_S,
};

/// What one pass of the sweeper did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SweepReport {
    /// Runs released.
    pub expired: usize,
    /// Runs it would have released (dry run).
    pub would_expire: usize,
}

/// Start the periodic sweep. It does nothing when the sweeper is switched off
/// (`QUARRY_EDGE__RUN_SWEEPER=0`). Abort the returned handle on shutdown.
pub fn spawn_run_sweeper(state: AppState) -> tokio::task::JoinHandle<()> {
    let limits = state.agent_runs.limits().clone();
    // zdr-log-ok: a periodic background sweep started at boot; no request is in flight, and its events carry run ids, classes and numbers only
    tokio::spawn(async move {
        if !limits.sweeper {
            tracing::info!("run sweeper: off");
            return;
        }
        tracing::info!(
            interval_s = limits.sweep_interval_s,
            dry_run = limits.sweeper_dry_run,
            "run sweeper: on"
        );
        let mut ticker = tokio::time::interval(Duration::from_secs(limits.sweep_interval_s));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // the first tick is immediate; nothing is old yet
        loop {
            ticker.tick().await;
            sweep_once(&state, now_ms()).await;
        }
    })
}

// ---------------------------------------------------------------------------
// W3-E3.4-2/3 (F-t2-035): the durable side of a release.
//
// The idle and absolute TTLs are WS2.D's (`RunLive::verdict` on the run's two clocks,
// `RunLimits`) and W2-U39b's (the human-browse lifetimes below). A release always left a
// non-ZDR checkpoint `active`: a resume worked, but nothing durable said the run had ended.
// With `QUARRY_EDGE__RUN_TTL` on, a run the sweeper releases moves its checkpoint along the
// D8-10 arc: `expired` (or `suspended` when its profile was persisted), still resumable with
// `resume_run_id`, and back to `active` when the resume saves its first checkpoint
// (`start_run`). A ZDR run keeps no checkpoint, so it has no arc and is never resumable.
// Default off: the checkpoint stays `active`, as before.
// ---------------------------------------------------------------------------

/// The switch for the durable release arc. Unset, empty, `0`, `false` or `off` is off;
/// `1`, `true` or `on` is on; anything else is off (and logged).
pub const ENV_RUN_TTL: &str = "QUARRY_EDGE__RUN_TTL";

/// Parse [`ENV_RUN_TTL`] with the same words as the other switches.
pub fn parse_run_ttl(raw: Option<&str>) -> Result<bool, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("0") | Some("false") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("on") => Ok(true),
        Some(other) => Err(format!(
            "{ENV_RUN_TTL} must be 0, 1, on, off, true or false, got {other:?}"
        )),
    }
}

/// Whether a release marks the run's checkpoint now. Read at each pass (and by a resume,
/// for its log line); unreadable is off.
pub fn run_ttl_enabled() -> bool {
    match parse_run_ttl(std::env::var(ENV_RUN_TTL).ok().as_deref()) {
        Ok(on) => on,
        Err(_) => {
            // The value is not echoed: an unreadable switch is off, and the boot log says so.
            tracing::warn!("run TTL switch unreadable; off");
            false
        }
    }
}

/// Mark the checkpoint of a run the sweeper just released (`finish_run` saved it `active`
/// with the trace seq spent): `expired`, or `suspended` for a persisted profile. Nothing for
/// a ZDR run. Skipped when the id is live again or a resume of it is being committed, so a
/// resume that won the race keeps its `active` checkpoint; a race lost after this check is
/// healed by the resumed run's next save, which writes `active`. A `closed` checkpoint (a
/// concurrent `DELETE`) stays closed: the store never moves it. Returns the status written.
async fn mark_released_checkpoint(
    state: &AppState,
    run_id: &str,
    handle: &RunHandle,
) -> Option<AgentRunStatus> {
    let (org_id, rid, status) = {
        let entry = handle.entry.lock().await;
        let status = released_checkpoint_status(entry.zdr.is_active(), entry.lease.persist_profile)?;
        (entry.org_id.clone(), entry.run_id.clone(), status)
    };
    let resumed = state
        .agent_runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .is_live_or_resuming(run_id);
    if resumed {
        return None;
    }
    match state.receipts.mark_run_checkpoint(&org_id, &rid, status).await {
        Ok(()) => Some(status),
        Err(error) => {
            tracing::warn!(
                %run_id,
                error_class = crate::zdr_log::class(&error),
                "could not mark the checkpoint of a released run"
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// D2-060 (WS8.B / WS8.C): a person's human-browse run.
//
// Its lifetimes are the plan's (idle 900 s, takeover 1800 s, absolute 4 h; see
// `lifecycle.rs`), whatever class the run was given, and its broker grant is kept
// alive by the sweeper: each pass validates the grant, renews it shortly before it
// expires (never past its absolute cap) and closes the run with a typed reason when
// the grant was revoked, expired, reached its cap or its renewal was refused.
//
// The grant half is dark behind `QUARRY_EDGE__HUMAN_GRANT_RENEW` (default off), on
// top of `QUARRY_EDGE__HUMAN_BROWSE` (default off), without which no human-browse
// run exists at all.
// ---------------------------------------------------------------------------

/// The switch for the sweeper's grant checks on human-browse runs. Unset, empty,
/// `0`, `false` or `off` is off; `1`, `true` or `on` is on; anything else is off
/// (and logged), since this file cannot stop the edge at boot.
pub const ENV_HUMAN_GRANT_RENEW: &str = "QUARRY_EDGE__HUMAN_GRANT_RENEW";

/// Parse [`ENV_HUMAN_GRANT_RENEW`] with the same words as the other switches.
pub fn parse_human_grant_renew(raw: Option<&str>) -> Result<bool, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("0") | Some("false") | Some("off") => Ok(false),
        Some("1") | Some("true") | Some("on") => Ok(true),
        Some(other) => Err(format!(
            "{ENV_HUMAN_GRANT_RENEW} must be 0, 1, on, off, true or false, got {other:?}"
        )),
    }
}

/// Whether the sweeper checks and renews human-browse grants now. Read at each pass;
/// unreadable is off.
pub fn human_grant_renew_enabled() -> bool {
    match parse_human_grant_renew(std::env::var(ENV_HUMAN_GRANT_RENEW).ok().as_deref()) {
        Ok(on) => on,
        Err(error) => {
            tracing::warn!(error = %error, "human grant renewal switch unreadable; off");
            false
        }
    }
}

/// Why the sweeper ended a human-browse run on its grant. These are wire strings (the
/// sweeper's log line); the `agent.expired` event carries `max_age` for each of them
/// until `ExpireReason` gains a grant variant (see the unit's depth notes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanGrantEnd {
    /// The broker says the grant is no longer active (revoked, or unknown to it).
    Revoked,
    /// The grant's expiry passed and it was not renewed in time.
    Expired,
    /// The grant reached its absolute cap; no renewal can pass it.
    AbsoluteCap,
    /// The broker refused the renewal.
    RenewalDenied,
}

impl HumanGrantEnd {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Revoked => "grant_revoked",
            Self::Expired => "grant_expired",
            Self::AbsoluteCap => "grant_absolute_cap",
            Self::RenewalDenied => "grant_renewal_denied",
        }
    }
}

/// Why a renewal did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanGrantRenewRefusal {
    /// The broker answered and said no (at the cap, not renewable, not this caller's,
    /// revoked). The run is closed.
    Denied,
    /// The broker could not be reached, or no renewer is wired. The run is kept; the
    /// next pass tries again, and the grant's own expiry still ends it.
    Unavailable,
}

/// The renew half of the broker (U61 `RenewGrant`): extend a human-browse grant by one
/// broker TTL, never past its absolute cap. Returns the new expiry.
#[async_trait::async_trait]
pub trait HumanGrantRenewer: Send + Sync {
    async fn renew(
        &self,
        org_id: &str,
        grant_id: &str,
    ) -> Result<chrono::DateTime<chrono::Utc>, HumanGrantRenewRefusal>;
}

/// The renewer the edge uses today. Quarry's vendored `browser.proto` does not carry
/// `RenewGrant` yet, and the broker accepts the call from a user principal only (WS8.C),
/// so a Quarry service token cannot renew a person's grant (see MP-ASKS). Every call is
/// [`HumanGrantRenewRefusal::Unavailable`]: the run is never closed on a renewal it could
/// not attempt, and it ends when the grant itself ends (the person's own keep-alive, WS8.D,
/// renews it through the gateway meanwhile).
#[derive(Debug, Clone, Copy, Default)]
pub struct UnwiredHumanGrantRenewer;

#[async_trait::async_trait]
impl HumanGrantRenewer for UnwiredHumanGrantRenewer {
    async fn renew(
        &self,
        _org_id: &str,
        _grant_id: &str,
    ) -> Result<chrono::DateTime<chrono::Utc>, HumanGrantRenewRefusal> {
        Err(HumanGrantRenewRefusal::Unavailable)
    }
}

/// What one grant check decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanGrantCheck {
    /// Nothing to do this pass.
    Keep,
    /// The grant was renewed.
    Renewed,
    /// Close the run.
    End(HumanGrantEnd),
}

/// How long before a grant's expiry the sweeper renews it: two sweep intervals, and
/// never less than a minute, so one missed pass still renews in time.
pub fn human_grant_renew_lead_s(sweep_interval_s: u64) -> u64 {
    sweep_interval_s.saturating_mul(2).max(60)
}

/// Check one human-browse grant at `now`. A broker that cannot be reached never ends a
/// person's session here (every step still revalidates and fails closed); a broker that
/// answers decides.
pub async fn check_human_grant(
    validator: &dyn quarry_runtime::GrantValidator,
    renewer: &dyn HumanGrantRenewer,
    org_id: &str,
    grant_id: &str,
    now: chrono::DateTime<chrono::Utc>,
    lead_s: u64,
) -> HumanGrantCheck {
    let grant = match validator.validate_for_org(org_id, grant_id).await {
        Ok(grant) => grant,
        Err(_) => return HumanGrantCheck::Keep,
    };
    if !grant.active {
        return HumanGrantCheck::End(HumanGrantEnd::Revoked);
    }
    if let Some(cap) = grant.absolute_expires_at {
        if now >= cap {
            return HumanGrantCheck::End(HumanGrantEnd::AbsoluteCap);
        }
    }
    let Some(expires_at) = grant.expires_at else {
        // A grant with no expiry needs no renewal.
        return HumanGrantCheck::Keep;
    };
    if now >= expires_at {
        return HumanGrantCheck::End(HumanGrantEnd::Expired);
    }
    let lead = chrono::Duration::seconds(i64::try_from(lead_s.min(86_400)).unwrap_or(86_400));
    if expires_at - now > lead {
        return HumanGrantCheck::Keep;
    }
    // Already at the cap: nothing to renew; the run ends when the cap is reached.
    if grant.absolute_expires_at.is_some_and(|cap| expires_at >= cap) {
        return HumanGrantCheck::Keep;
    }
    match renewer.renew(org_id, grant_id).await {
        Ok(_) => HumanGrantCheck::Renewed,
        Err(HumanGrantRenewRefusal::Denied) => HumanGrantCheck::End(HumanGrantEnd::RenewalDenied),
        Err(HumanGrantRenewRefusal::Unavailable) => HumanGrantCheck::Keep,
    }
}

/// The human-browse lifetimes, from what the run's clocks say. An action in flight is
/// never expired; the absolute cap applies even with a viewer open; a viewer blocks the
/// idle limits; a takeover has the longer idle limit.
pub fn human_browse_expiry(
    in_flight: bool,
    viewers: bool,
    takeover: bool,
    idle_s: u64,
    age_s: u64,
) -> Option<ExpireReason> {
    if in_flight {
        return None;
    }
    if age_s >= HUMAN_BROWSE_ABSOLUTE_TTL_S {
        return Some(ExpireReason::MaxAge);
    }
    if viewers {
        return None;
    }
    if takeover {
        (idle_s >= HUMAN_BROWSE_TAKEOVER_IDLE_TTL_S).then_some(ExpireReason::TakeoverIdle)
    } else {
        (idle_s >= HUMAN_BROWSE_IDLE_TTL_S).then_some(ExpireReason::Idle)
    }
}

fn human_browse_verdict(live: &RunLive, now_ms: u64) -> Option<ExpireVerdict> {
    let (idle_s, age_s) = live.idle_and_age_s(now_ms);
    human_browse_expiry(
        live.in_flight() > 0,
        live.viewers() > 0,
        live.snapshot().mode == BrowserControlMode::HumanTakeover,
        idle_s,
        age_s,
    )
    .map(|reason| ExpireVerdict { reason, idle_s, age_s })
}

/// A human-browse run's org and grant, read without waiting: a run whose entry is held
/// right now is in use, and is judged by its ordinary verdict this pass.
fn human_browse_run(handle: &RunHandle) -> Option<(String, Option<String>)> {
    let entry = handle.entry.try_lock().ok()?;
    if entry.closed || !entry.nav_policy.is_open_web() {
        return None;
    }
    Some((entry.org_id.clone(), entry.grant_id.clone()))
}

/// The broker half the sweeper uses for human-browse grants.
pub struct HumanGrantSweep<'a> {
    pub validator: &'a dyn quarry_runtime::GrantValidator,
    pub renewer: &'a dyn HumanGrantRenewer,
}

/// One pass at `now_ms`: release every run whose verdict says it has expired.
/// In a dry run it logs `would_expire` (run id and numbers only, never a URL) and
/// releases nothing, which is how the first deploy runs so a person's session is
/// never reaped by a number nobody has checked.
pub async fn sweep_once(state: &AppState, now_ms: u64) -> SweepReport {
    let renewer = UnwiredHumanGrantRenewer;
    let grants = human_grant_renew_enabled().then(|| HumanGrantSweep {
        validator: state.grant_validator.as_ref(),
        renewer: &renewer,
    });
    sweep_once_with(state, now_ms, grants.as_ref()).await
}

/// [`sweep_once`] with the grant half given explicitly (`None`: no grant checks).
pub async fn sweep_once_with(
    state: &AppState,
    now_ms: u64,
    grants: Option<&HumanGrantSweep<'_>>,
) -> SweepReport {
    sweep_once_ttl(state, now_ms, grants, run_ttl_enabled()).await
}

/// [`sweep_once_with`] with the W3-E3.4-2/3 switch given explicitly (`run_ttl`: mark each
/// released run's checkpoint `expired`/`suspended`).
pub async fn sweep_once_ttl(
    state: &AppState,
    now_ms: u64,
    grants: Option<&HumanGrantSweep<'_>>,
    run_ttl: bool,
) -> SweepReport {
    let limits = state.agent_runs.limits().clone();
    let runs = state
        .agent_runs
        .lock()
        .expect("agent_runs mutex poisoned")
        .snapshot();
    let lead_s = human_grant_renew_lead_s(limits.sweep_interval_s);
    let mut report = SweepReport::default();
    for (run_id, handle) in runs {
        let human = human_browse_run(&handle);
        let mut grant_end: Option<HumanGrantEnd> = None;
        let verdict = if human.is_some() {
            human_browse_verdict(&handle.live, now_ms)
        } else {
            handle.live.verdict(now_ms, &limits)
        };
        let verdict = match verdict {
            Some(verdict) => verdict,
            None => {
                let (Some(grants), Some((org_id, Some(grant_id)))) = (grants, human.as_ref()) else {
                    continue;
                };
                if handle.live.in_flight() > 0 {
                    continue;
                }
                let now = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
                    i64::try_from(now_ms).unwrap_or(i64::MAX),
                )
                .unwrap_or_else(chrono::Utc::now);
                match check_human_grant(grants.validator, grants.renewer, org_id, grant_id, now, lead_s)
                    .await
                {
                    HumanGrantCheck::Keep => continue,
                    HumanGrantCheck::Renewed => {
                        tracing::info!(%run_id, "human-browse grant renewed");
                        continue;
                    }
                    HumanGrantCheck::End(end) => {
                        grant_end = Some(end);
                        let (idle_s, age_s) = handle.live.idle_and_age_s(now_ms);
                        ExpireVerdict { reason: ExpireReason::MaxAge, idle_s, age_s }
                    }
                }
            }
        };
        let class = handle.live.class().label();
        let reason = grant_end.map_or(verdict.reason.as_str(), HumanGrantEnd::as_str);
        if limits.sweeper_dry_run {
            report.would_expire += 1;
            tracing::warn!(
                %run_id,
                class,
                reason,
                idle_s = verdict.idle_s,
                age_s = verdict.age_s,
                "would_expire: run sweeper is in dry run"
            );
            continue;
        }
        tracing::info!(
            %run_id,
            class,
            reason,
            idle_s = verdict.idle_s,
            age_s = verdict.age_s,
            "run expired"
        );
        let released = run_ttl.then(|| handle.clone());
        match finish_run(state, &run_id, handle, CloseReason::Expired(verdict)).await {
            Ok(_) => {
                report.expired += 1;
                crate::metrics::record_run_expired(reason, 1);
                if let Some(handle) = released {
                    if let Some(status) = mark_released_checkpoint(state, &run_id, &handle).await {
                        tracing::info!(
                            %run_id,
                            checkpoint_status = status.as_str(),
                            "released run is resumable"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(
                %run_id,
                error_class = crate::zdr_log::class(&error),
                "run sweeper could not release a run"
            ),
        }
    }
    report
}

#[cfg(test)]
mod human_grant_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use chrono::{DateTime, Duration as ChronoDuration, Utc};
    use quarry_runtime::{GrantValidation, GrantValidator, NoopGrantValidator};

    fn now() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap()
    }

    async fn grant(
        active: bool,
        expires_in_s: Option<i64>,
        cap_in_s: Option<i64>,
    ) -> GrantValidation {
        let mut grant = NoopGrantValidator.validate("g-human").await.unwrap();
        grant.active = active;
        grant.kind = "human_browse".into();
        grant.policy_mode = "open_web".into();
        grant.expires_at = expires_in_s.map(|s| now() + ChronoDuration::seconds(s));
        grant.absolute_expires_at = cap_in_s.map(|s| now() + ChronoDuration::seconds(s));
        grant
    }

    /// A validator that answers one scripted grant, or an outage.
    struct FakeValidator {
        answer: Option<GrantValidation>,
        calls: AtomicUsize,
    }

    impl FakeValidator {
        fn answering(answer: Option<GrantValidation>) -> Self {
            Self { answer, calls: AtomicUsize::new(0) }
        }
    }

    #[async_trait::async_trait]
    impl GrantValidator for FakeValidator {
        async fn validate(&self, _grant_id: &str) -> QuarryResult<GrantValidation> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.answer
                .clone()
                .ok_or_else(|| QuarryError::new(ErrorCode::Timeout, "broker down"))
        }
    }

    /// A renewer that answers one scripted result and counts its calls.
    struct FakeRenewer {
        answer: Result<(), HumanGrantRenewRefusal>,
        calls: AtomicUsize,
    }

    impl FakeRenewer {
        fn answering(answer: Result<(), HumanGrantRenewRefusal>) -> Self {
            Self { answer, calls: AtomicUsize::new(0) }
        }
        fn calls(&self) -> usize {
            self.calls.load(AtomicOrdering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl HumanGrantRenewer for FakeRenewer {
        async fn renew(
            &self,
            _org_id: &str,
            _grant_id: &str,
        ) -> Result<DateTime<Utc>, HumanGrantRenewRefusal> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            self.answer.map(|()| now() + ChronoDuration::seconds(900))
        }
    }

    async fn check(validator: &FakeValidator, renewer: &FakeRenewer) -> HumanGrantCheck {
        check_human_grant(validator, renewer, "org", "g-human", now(), 60).await
    }

    #[tokio::test]
    async fn a_grant_far_from_expiry_is_left_alone() {
        let validator = FakeValidator::answering(Some(grant(true, Some(600), Some(14_400)).await));
        let renewer = FakeRenewer::answering(Ok(()));
        assert_eq!(check(&validator, &renewer).await, HumanGrantCheck::Keep);
        assert_eq!(renewer.calls(), 0);
    }

    #[tokio::test]
    async fn a_grant_near_expiry_is_renewed() {
        let validator = FakeValidator::answering(Some(grant(true, Some(30), Some(14_400)).await));
        let renewer = FakeRenewer::answering(Ok(()));
        assert_eq!(check(&validator, &renewer).await, HumanGrantCheck::Renewed);
        assert_eq!(renewer.calls(), 1);
    }

    #[tokio::test]
    async fn a_denied_renewal_ends_the_run_with_a_typed_reason() {
        let validator = FakeValidator::answering(Some(grant(true, Some(30), Some(14_400)).await));
        let renewer = FakeRenewer::answering(Err(HumanGrantRenewRefusal::Denied));
        let decided = check(&validator, &renewer).await;
        assert_eq!(decided, HumanGrantCheck::End(HumanGrantEnd::RenewalDenied));
        assert_eq!(HumanGrantEnd::RenewalDenied.as_str(), "grant_renewal_denied");
    }

    #[tokio::test]
    async fn an_unreachable_renewer_keeps_the_run() {
        let validator = FakeValidator::answering(Some(grant(true, Some(30), Some(14_400)).await));
        let renewer = FakeRenewer::answering(Err(HumanGrantRenewRefusal::Unavailable));
        assert_eq!(check(&validator, &renewer).await, HumanGrantCheck::Keep);
        assert_eq!(renewer.calls(), 1);
    }

    #[tokio::test]
    async fn the_unwired_renewer_never_closes_a_run() {
        let validator = FakeValidator::answering(Some(grant(true, Some(30), Some(14_400)).await));
        let decided =
            check_human_grant(&validator, &UnwiredHumanGrantRenewer, "org", "g-human", now(), 60).await;
        assert_eq!(decided, HumanGrantCheck::Keep);
    }

    #[tokio::test]
    async fn renewal_never_goes_past_the_absolute_cap() {
        // The expiry already equals the cap: nothing to renew, the renewer is not called.
        let validator = FakeValidator::answering(Some(grant(true, Some(30), Some(30)).await));
        let renewer = FakeRenewer::answering(Ok(()));
        assert_eq!(check(&validator, &renewer).await, HumanGrantCheck::Keep);
        assert_eq!(renewer.calls(), 0);
        // At the cap the run ends.
        let validator = FakeValidator::answering(Some(grant(true, Some(30), Some(0)).await));
        assert_eq!(
            check(&validator, &renewer).await,
            HumanGrantCheck::End(HumanGrantEnd::AbsoluteCap)
        );
        assert_eq!(renewer.calls(), 0);
    }

    #[tokio::test]
    async fn a_revoked_or_expired_grant_ends_the_run() {
        let renewer = FakeRenewer::answering(Ok(()));
        let validator = FakeValidator::answering(Some(grant(false, Some(600), Some(14_400)).await));
        assert_eq!(check(&validator, &renewer).await, HumanGrantCheck::End(HumanGrantEnd::Revoked));
        let validator = FakeValidator::answering(Some(grant(true, Some(-1), Some(14_400)).await));
        assert_eq!(check(&validator, &renewer).await, HumanGrantCheck::End(HumanGrantEnd::Expired));
        assert_eq!(renewer.calls(), 0);
    }

    #[tokio::test]
    async fn a_broker_outage_keeps_the_run() {
        let validator = FakeValidator::answering(None);
        let renewer = FakeRenewer::answering(Err(HumanGrantRenewRefusal::Denied));
        assert_eq!(check(&validator, &renewer).await, HumanGrantCheck::Keep);
        assert_eq!(validator.calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(renewer.calls(), 0);
    }

    #[test]
    fn the_renew_lead_is_two_passes_and_at_least_a_minute() {
        assert_eq!(human_grant_renew_lead_s(15), 60);
        assert_eq!(human_grant_renew_lead_s(45), 90);
    }

    #[test]
    fn human_browse_lifetimes_are_the_plan_values() {
        assert_eq!(HUMAN_BROWSE_IDLE_TTL_S, 900);
        assert_eq!(HUMAN_BROWSE_TAKEOVER_IDLE_TTL_S, 1800);
        assert_eq!(HUMAN_BROWSE_ABSOLUTE_TTL_S, 14_400);
        // Idle: 900 s without a viewer.
        assert_eq!(human_browse_expiry(false, false, false, 899, 899), None);
        assert_eq!(human_browse_expiry(false, false, false, 900, 900), Some(ExpireReason::Idle));
        // Takeover: 1800 s.
        assert_eq!(human_browse_expiry(false, false, true, 1_799, 1_799), None);
        assert_eq!(
            human_browse_expiry(false, false, true, 1_800, 1_800),
            Some(ExpireReason::TakeoverIdle)
        );
        // A viewer blocks idle but not the 4 h cap; an action in flight blocks both.
        assert_eq!(human_browse_expiry(false, true, false, 5_000, 5_000), None);
        assert_eq!(
            human_browse_expiry(false, true, false, 0, 14_400),
            Some(ExpireReason::MaxAge)
        );
        assert_eq!(human_browse_expiry(true, false, false, 99_999, 99_999), None);
    }

    #[test]
    fn the_renew_switch_defaults_off_and_rejects_typos() {
        assert_eq!(parse_human_grant_renew(None), Ok(false));
        assert_eq!(parse_human_grant_renew(Some("")), Ok(false));
        assert_eq!(parse_human_grant_renew(Some("on")), Ok(true));
        assert!(parse_human_grant_renew(Some("yes please")).is_err());
    }
}

#[cfg(test)]
mod run_ttl_tests {
    use super::*;

    use crate::budget::RunClass;
    use crate::limits::RunCapOverride;
    use crate::test_support::{claims_for_org, insert_live_run, test_state, StubDriver};

    /// Far past the default interactive idle limit (900 s) of a run inserted now.
    fn later() -> u64 {
        now_ms() + 3_600_000
    }

    async fn ids(handle: &RunHandle) -> (String, RunKind) {
        let entry = handle.entry.lock().await;
        (entry.org_id.clone(), entry.run_id.clone())
    }

    async fn status_of(state: &AppState, handle: &RunHandle) -> Option<AgentRunStatus> {
        let (org_id, rid) = ids(handle).await;
        state
            .receipts
            .load_run_checkpoint(&org_id, &rid)
            .await
            .expect("the in-memory store reads")
            .map(|checkpoint| checkpoint.status)
    }

    #[test]
    fn the_run_ttl_switch_defaults_off_and_rejects_typos() {
        assert_eq!(parse_run_ttl(None), Ok(false));
        assert_eq!(parse_run_ttl(Some("")), Ok(false));
        assert_eq!(parse_run_ttl(Some(" OFF ")), Ok(false));
        assert_eq!(parse_run_ttl(Some("on")), Ok(true));
        assert_eq!(parse_run_ttl(Some("1")), Ok(true));
        assert!(parse_run_ttl(Some("sometimes")).is_err());
    }

    #[tokio::test]
    async fn with_the_switch_off_a_released_run_keeps_an_active_checkpoint() {
        let state = test_state(StubDriver::ok());
        let handle = insert_live_run(&state, &claims_for_org("org"));
        let report = sweep_once_ttl(&state, later(), None, false).await;
        assert_eq!(report.expired, 1);
        assert_eq!(status_of(&state, &handle).await, Some(AgentRunStatus::Active));
    }

    #[tokio::test]
    async fn with_the_switch_on_a_released_run_is_expired_and_still_resumable() {
        let state = test_state(StubDriver::ok());
        let handle = insert_live_run(&state, &claims_for_org("org"));
        // Not idle long enough: nothing is released and nothing is marked.
        assert_eq!(sweep_once_ttl(&state, now_ms(), None, true).await.expired, 0);
        let report = sweep_once_ttl(&state, later(), None, true).await;
        assert_eq!(report.expired, 1);
        assert_eq!(state.agent_runs.stats().live, 0);
        assert_eq!(status_of(&state, &handle).await, Some(AgentRunStatus::Expired));
        let (org_id, rid) = ids(&handle).await;
        let checkpoint = state
            .receipts
            .load_run_checkpoint(&org_id, &rid)
            .await
            .unwrap()
            .expect("the checkpoint is kept");
        assert!(checkpoint.check_resumable().is_ok(), "expired is resumable");
    }

    #[tokio::test]
    async fn a_dry_run_marks_nothing() {
        let mut state = test_state(StubDriver::ok());
        state.agent_runs = new_runs_with(crate::run_limits::RunLimits {
            sweeper_dry_run: true,
            ..crate::run_limits::RunLimits::unlimited()
        });
        let handle = insert_live_run(&state, &claims_for_org("org"));
        let report = sweep_once_ttl(&state, later(), None, true).await;
        assert_eq!((report.expired, report.would_expire), (0, 1));
        assert_eq!(status_of(&state, &handle).await, None, "a live run saved nothing yet");
    }

    #[tokio::test]
    async fn a_run_with_a_persisted_profile_is_suspended() {
        let state = test_state(StubDriver::ok());
        let handle = insert_live_run(&state, &claims_for_org("org"));
        handle.entry.lock().await.lease.persist_profile = true;
        assert_eq!(sweep_once_ttl(&state, later(), None, true).await.expired, 1);
        assert_eq!(status_of(&state, &handle).await, Some(AgentRunStatus::Suspended));
    }

    #[tokio::test]
    async fn a_zdr_run_keeps_no_checkpoint_and_has_no_arc() {
        let state = test_state(StubDriver::ok());
        let handle = insert_live_run(&state, &claims_for_org("org"));
        handle.entry.lock().await.zdr = ZdrMode::On;
        assert_eq!(sweep_once_ttl(&state, later(), None, true).await.expired, 1);
        assert_eq!(status_of(&state, &handle).await, None);
        let (_, rid) = ids(&handle).await;
        assert_eq!(mark_released_checkpoint(&state, &rid.to_string(), &handle).await, None);
    }

    #[tokio::test]
    async fn a_resume_that_won_the_race_keeps_its_active_checkpoint() {
        let state = test_state(StubDriver::ok());
        let handle = insert_live_run(&state, &claims_for_org("org"));
        let run_id = {
            let entry = handle.entry.lock().await;
            state
                .receipts
                .save_run_checkpoint(checkpoint_for_entry(&entry, &handle.live))
                .await
                .unwrap();
            entry.run_id.to_string()
        };
        // Live: not marked.
        assert_eq!(mark_released_checkpoint(&state, &run_id, &handle).await, None);
        // Released, and a resume of the id is being committed: not marked either.
        state.agent_runs.lock().unwrap().remove(&run_id);
        let slot = state
            .agent_runs
            .try_reserve_for("org", RunClass::Agent, RunCapOverride::NONE, Some(&run_id))
            .expect("the resume claims the id");
        assert_eq!(mark_released_checkpoint(&state, &run_id, &handle).await, None);
        assert_eq!(status_of(&state, &handle).await, Some(AgentRunStatus::Active));
        // The resume failed and gave the id back: now the release is marked.
        drop(slot);
        assert_eq!(
            mark_released_checkpoint(&state, &run_id, &handle).await,
            Some(AgentRunStatus::Expired)
        );
        assert_eq!(status_of(&state, &handle).await, Some(AgentRunStatus::Expired));
    }

    #[tokio::test]
    async fn a_closed_checkpoint_is_never_reopened_by_the_mark() {
        let state = test_state(StubDriver::ok());
        let handle = insert_live_run(&state, &claims_for_org("org"));
        let (org_id, rid) = ids(&handle).await;
        {
            let entry = handle.entry.lock().await;
            state
                .receipts
                .save_run_checkpoint(checkpoint_for_entry(&entry, &handle.live))
                .await
                .unwrap();
        }
        state.receipts.close_run_checkpoint(&org_id, &rid).await.unwrap();
        state.agent_runs.lock().unwrap().remove(&rid.to_string());
        mark_released_checkpoint(&state, &rid.to_string(), &handle).await;
        assert_eq!(status_of(&state, &handle).await, Some(AgentRunStatus::Closed));
    }
}
