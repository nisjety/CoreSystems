//! Skill version pins for Do runs (EP-11 S3, the Do path).
//!
//! `fetch_skill_context` injects skills from session-core's current rows, and
//! `reattach_skill` recovers one from the current row too. A skill an operator
//! edits mid-run would therefore steer the run's first rounds with one text and
//! its recovery with another. The Ask path (model-gateway `run_context.rs`,
//! `chat_turn_registry.rs`, `skills.rs`) already pins what it injected; this
//! module is the same mechanism for `RunAgent`, kept as an independent copy
//! because the two live in separate crates and the rule is a few lines.
//!
//! * [`record`] pins the skills a run injected: in the process-local
//!   [`RunPinRegistry`] (recovery inside the run reads it) and, best-effort, on
//!   the run record through `RecordRunContext` with a skills-only status.
//! * [`recover`] answers `reattach_skill` from the version the run pinned when
//!   the current row's text has moved since, never from the moved text.
//!
//! Off unless `EXECUTION_CORE_SKILL_PINS` is the literal `true`: off, nothing is
//! recorded and recovery reads the current row, byte for byte as before. A
//! zero-retention or waived run is never pinned and never reads a pin: the run
//! record is a durable write and session-core refuses it for such a run anyway.
//! A pin holds ids, numbers and a hash, never skill text.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use mp_contracts::model_plane::v1::{
    self as pb, session_core_client::SessionCoreClient,
    skill_version_service_client::SkillVersionServiceClient,
};
use tonic::transport::Channel;

/// Environment switch for Do-run skill pins.
pub const SWITCH_ENV: &str = "EXECUTION_CORE_SKILL_PINS";

/// Counter bumped when a pin could not be stamped on the run record.
const RECORD_FAILED_METRIC: &str = "skill_pin_record_failed_total";

/// Only the literal `true` enables the switch, as every switch of this service reads it.
#[must_use]
pub fn switch_on(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether the deployment pins Do-run skills, read once per process so a run cannot see it change.
#[must_use]
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| switch_on(std::env::var(SWITCH_ENV).ok().as_deref()))
}

/// Whether a run is pinned: the switch is on and the run keeps state (not
/// zero-retention, not waived).
#[must_use]
pub fn applies(on: bool, zdr: bool, waived: bool) -> bool {
    on && !zdr && !waived
}

/// How long a run's pins stay readable for recovery inside the run. A run that
/// outlives it recovers from the current row, as with the switch off.
const RUN_PIN_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// Most runs the registry holds pins for; a full map drops a new run's pins
/// rather than a live run's.
const MAX_ENTRIES: usize = 20_000;

/// Bound on the best-effort run-record stamp, so a slow session-core cannot hold a run's start.
const RECORD_TIMEOUT: Duration = Duration::from_secs(2);

/// The skill versions each live Do run injected, keyed by `(org, run_id)`.
/// In-process: the durable copy is the run record's `run_context.skills`.
#[derive(Clone, Default)]
pub struct RunPinRegistry {
    inner: Arc<DashMap<String, (Vec<pb::SkillPin>, Instant)>>,
}

impl RunPinRegistry {
    fn key(org_id: &str, run_id: &str) -> Option<String> {
        let (org, run) = (org_id.trim(), run_id.trim());
        if org.is_empty() || run.is_empty() || org.contains('\u{1f}') {
            return None;
        }
        Some(format!("{org}\u{1f}{run}"))
    }

    /// Keep a run's pins and return the run's full pin list when this call added
    /// to it, `None` when there was nothing new (or nothing could be kept).
    ///
    /// A skill the run already pinned keeps its first pin: that is the version
    /// the run started with, and a delegated child matching the same skill later
    /// must not move it.
    pub fn record(
        &self,
        org_id: &str,
        run_id: &str,
        pins: &[pb::SkillPin],
    ) -> Option<Vec<pb::SkillPin>> {
        if pins.is_empty() {
            return None;
        }
        let key = Self::key(org_id, run_id)?;
        let now = Instant::now();
        self.inner
            .retain(|_, (_, at)| now.duration_since(*at) < RUN_PIN_TTL);
        if self.inner.len() >= MAX_ENTRIES && !self.inner.contains_key(&key) {
            return None;
        }
        let mut entry = self.inner.entry(key).or_insert_with(|| (Vec::new(), now));
        let (held, at) = entry.value_mut();
        let mut added = false;
        for pin in pins {
            if held.iter().all(|have| have.skill_id != pin.skill_id) {
                held.push(pin.clone());
                added = true;
            }
        }
        *at = now;
        added.then(|| held.clone())
    }

    /// The pin this run holds for one skill, if any.
    #[must_use]
    pub fn pin(&self, org_id: &str, run_id: &str, skill_id: &str) -> Option<pb::SkillPin> {
        let entry = self.inner.get(&Self::key(org_id, run_id)?)?;
        let (pins, at) = entry.value();
        if Instant::now().duration_since(*at) >= RUN_PIN_TTL {
            return None;
        }
        pins.iter().find(|pin| pin.skill_id == skill_id).cloned()
    }
}

/// Process-wide Do-run pin registry.
pub fn run_pins() -> &'static RunPinRegistry {
    static GLOBAL: OnceLock<RunPinRegistry> = OnceLock::new();
    GLOBAL.get_or_init(RunPinRegistry::default)
}

/// The pin of one injected skill: ids, numbers and a hash, never its text. A
/// Do run injects only matched skills.
#[must_use]
pub fn pin_of(skill: &pb::AgentSkill) -> pb::SkillPin {
    pb::SkillPin {
        skill_id: skill.id.clone(),
        version: skill.version,
        digest: skill.digest.clone(),
        source: "matched".to_owned(),
    }
}

/// Whether recovery must read the pinned version instead of the current row:
/// the run pinned a numbered version and the row's text has moved since. A pin
/// without a version (a row from before versions) has nothing to read back, so
/// the current row stands.
#[must_use]
pub fn needs_version_read(pin: &pb::SkillPin, current: &pb::AgentSkill) -> bool {
    pin.version > 0 && pin.skill_id == current.id && pin.digest != current.digest
}

/// What `reattach_skill` says when the version this run pinned no longer exists
/// (the skill was deleted or erased). The model must not follow the current text
/// as if it were the one the run started with.
#[must_use]
pub fn render_gone(name: &str) -> String {
    serde_json::json!({
        "status": "not_available",
        "name": name,
        "detail": "the version of this skill this run used is no longer available; do not \
                   follow it, and do not guess at the missing instruction",
    })
    .to_string()
}

/// The message of the tool error a failed pinned read becomes.
pub const READ_FAILED: &str =
    "reattach_skill could not read the version of this skill this run used right now";

/// Stamp the skills a Do run injected: keep them for recovery inside the run and
/// record them on the run record, best-effort. A lost stamp costs the durable
/// record of which version steered the run, never the run, and is counted.
///
/// The caller has already checked [`applies`]; this checks nothing about retention.
pub async fn record(
    session_channel: &Channel,
    bearer: &str,
    org_id: &str,
    run_id: &str,
    pins: Vec<pb::SkillPin>,
) {
    let Some(all) = run_pins().record(org_id, run_id, &pins) else {
        return;
    };
    let request = crate::runtime_loop::authenticated_session_request(
        pb::RecordRunContextRequest {
            run_id: run_id.to_owned(),
            org_id: org_id.to_owned(),
            status: Some(pb::RunContextStatus {
                skills: all,
                ..Default::default()
            }),
        },
        bearer,
    );
    let Ok(request) = request else {
        tracing::warn!(%run_id, "skill pins: session credential is not forwardable; not recorded");
        metrics::counter!(RECORD_FAILED_METRIC).increment(1);
        return;
    };
    let mut client = SessionCoreClient::new(session_channel.clone());
    let call = client.record_run_context(request);
    match tokio::time::timeout(RECORD_TIMEOUT, call).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(code = ?error.code(), %run_id, "skill pins: not recorded (best-effort)");
            metrics::counter!(RECORD_FAILED_METRIC).increment(1);
        }
        Err(_elapsed) => {
            tracing::warn!(%run_id, "skill pins: recording timed out (best-effort)");
            metrics::counter!(RECORD_FAILED_METRIC).increment(1);
        }
    }
}

/// What recovering a named skill reads from.
#[derive(Debug, PartialEq)]
pub enum Recovery {
    /// The current row: pins off, no pin for this skill in this run, the row
    /// absent or disabled (recovery says so itself), or its text unchanged since
    /// the run pinned it.
    Current,
    /// The version the run pinned, its body read back: render it as the skill.
    Pinned(pb::AgentSkill),
    /// The pinned version no longer exists; the rendering says so.
    Gone(String),
    /// The pinned version could not be read. A tool error, never a fallback to
    /// the moved text.
    ReadFailed,
}

/// Decide where `reattach_skill` reads the named skill from. `skills` is the
/// org's current rows; `name` the model's request. Never reads for a ZDR or
/// waived run, and never when the switch is off.
pub async fn recover(
    session_channel: &Channel,
    bearer: &str,
    org_id: &str,
    run_id: &str,
    retention_bound: bool,
    skills: &[pb::AgentSkill],
    name: &str,
) -> Recovery {
    if !applies(enabled(), retention_bound, false) {
        return Recovery::Current;
    }
    let wanted = name.trim().to_lowercase();
    let Some(current) = skills
        .iter()
        .find(|skill| skill.enabled && skill.name.trim().to_lowercase() == wanted)
    else {
        return Recovery::Current;
    };
    let Some(pin) = run_pins().pin(org_id, run_id, &current.id) else {
        return Recovery::Current;
    };
    if !needs_version_read(&pin, current) {
        return Recovery::Current;
    }
    let request = crate::runtime_loop::authenticated_session_request(
        pb::GetAgentSkillVersionRequest {
            org_id: org_id.to_owned(),
            skill_id: pin.skill_id.clone(),
            version: pin.version,
            for_user_id: String::new(),
        },
        bearer,
    );
    let Ok(request) = request else {
        return Recovery::ReadFailed;
    };
    let response = match SkillVersionServiceClient::new(session_channel.clone())
        .get_agent_skill_version(request)
        .await
    {
        Ok(response) => response.into_inner(),
        Err(error) => {
            tracing::warn!(code = ?error.code(), %org_id, skill_id = %pin.skill_id, version = pin.version, "reattach_skill: pinned version read failed");
            return Recovery::ReadFailed;
        }
    };
    let Some(version) = response.version.filter(|_| !response.gone) else {
        return Recovery::Gone(render_gone(&current.name));
    };
    Recovery::Pinned(pb::AgentSkill {
        id: version.skill_id,
        name: version.name,
        description: version.description,
        content: version.content,
        enabled: true,
        ..Default::default()
    })
}

/// Section 3.4: the switch is documented as "only the literal true". A padded, differently cased or
/// truthy-word value must leave it off, so a mis-set deployment keeps today's behaviour.
#[cfg(test)]
mod switch_literal_parse {
    #[test]
    fn only_the_literal_true_switches_it_on() {
        assert!(super::switch_on(Some("true")));
        for raw in [
            None,
            Some(""),
            Some(" true"),
            Some("true\n"),
            Some("TRUE"),
            Some("True"),
            Some("1"),
            Some("on"),
            Some("yes"),
        ] {
            assert!(!super::switch_on(raw), "{raw:?} must leave the switch off");
        }
    }
}
