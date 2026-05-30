//! NATS subject constants and helpers for the Model Plane v1 subject tree.
//!
//! New subject namespace: `mp.v1.*`
//! Legacy subjects (compatibility): `velion.agent.*`, `velion.session.*`, `aqencia.reasoning.*`

/// Compatibility mode for cutover between legacy and v1 NATS subject trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatMode {
    V1Only,
    DualWrite,
    DualRead,
    LegacyOnly,
}

/// Errors when resolving subscriber subject sets for compatibility modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectSelectionError {
    NoLegacyMapping { subject: String },
}

impl std::fmt::Display for SubjectSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoLegacyMapping { subject } => {
                write!(f, "no legacy mapping for subject {subject}")
            }
        }
    }
}

impl std::error::Error for SubjectSelectionError {}

impl CompatMode {
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "dual_write" => Self::DualWrite,
            "dual_read" => Self::DualRead,
            "legacy_only" => Self::LegacyOnly,
            "v1_only" | "" => Self::V1Only,
            _ => Self::V1Only,
        }
    }

    #[must_use]
    pub fn from_env() -> Self {
        Self::parse(&std::env::var("MP_COMPAT_MODE").unwrap_or_default())
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1Only => "v1_only",
            Self::DualWrite => "dual_write",
            Self::DualRead => "dual_read",
            Self::LegacyOnly => "legacy_only",
        }
    }
}

/// Root prefix for all new Model Plane events.
pub const PREFIX: &str = "mp.v1";

/// Build a subject for a specific event type and resource.
///
/// Example: `mp.v1.session.start.{session_key}`
#[must_use]
pub fn event_subject(event_type: &str, resource_id: &str) -> String {
    format!("{PREFIX}.{event_type}.{resource_id}")
}

/// Subject for run events: `mp.v1.run.{run_id}.event`
#[must_use]
pub fn run_event_subject(run_id: &str) -> String {
    format!("{PREFIX}.run.{run_id}.event")
}

/// Subject for session commands: `mp.v1.session.{session_key}.command`
#[must_use]
pub fn session_command_subject(session_key: &str) -> String {
    format!("{PREFIX}.session.{session_key}.command")
}

/// Subject for ingress events: `mp.v1.ingress.{kind}`
#[must_use]
pub fn ingress_subject(kind: &str) -> String {
    format!("{PREFIX}.ingress.{kind}")
}

/// Wildcard for all run events (for consumers): `mp.v1.run.*.event`
pub const RUN_EVENTS_WILDCARD: &str = "mp.v1.run.*.event";

/// Wildcard for all session commands: `mp.v1.session.*.command`
pub const SESSION_COMMANDS_WILDCARD: &str = "mp.v1.session.*.command";

/// Wildcard for all ingress events: `mp.v1.ingress.*`
pub const INGRESS_WILDCARD: &str = "mp.v1.ingress.*";

/// Subject for usage events: `mp.v1.usage.{org_id}`
#[must_use]
pub fn usage_subject(org_id: &str) -> String {
    format!("{PREFIX}.usage.{org_id}")
}

/// Subject for stream lifecycle events: `mp.v1.stream.{kind}`
#[must_use]
pub fn stream_subject(kind: &str) -> String {
    format!("{PREFIX}.stream.{kind}")
}

/// Wildcard for all usage events: `mp.v1.usage.*`
pub const USAGE_WILDCARD: &str = "mp.v1.usage.*";

/// Wildcard for all stream events: `mp.v1.stream.*`
pub const STREAM_WILDCARD: &str = "mp.v1.stream.*";

// ---------------------------------------------------------------------------
// Retrieval / learning loop subjects.
//
// Emitted by `session-core` when context assembly consumes Data Plane
// retrieval results, and by downstream consumers (App Shell, Data Plane
// relevance trainer) that want to learn from which retrievals were
// actually used in successful runs.
// ---------------------------------------------------------------------------

/// Subject for retrieval-fetched events: `mp.v1.retrieval.fetched.{org_id}`.
/// Published when session-core successfully fetches retrieval candidates
/// from Data Plane during context assembly.
#[must_use]
pub fn retrieval_fetched_subject(org_id: &str) -> String {
    format!("{PREFIX}.retrieval.fetched.{org_id}")
}

/// Subject for retrieval-used events: `mp.v1.retrieval.used.{org_id}`.
/// Published when retrieval candidates appear in the assembled context
/// shipped to a model — the strongest signal that the candidates were
/// considered useful. Data Plane relevance trainers should subscribe.
#[must_use]
pub fn retrieval_used_subject(org_id: &str) -> String {
    format!("{PREFIX}.retrieval.used.{org_id}")
}

/// Subject for low-confidence retrieval events: `mp.v1.retrieval.low_confidence.{org_id}`.
/// Published when `RetrieveResponse.low_confidence == true`. Useful for
/// triggering wiki-proposal workflows or alerting that the corpus has a
/// gap.
#[must_use]
pub fn retrieval_low_confidence_subject(org_id: &str) -> String {
    format!("{PREFIX}.retrieval.low_confidence.{org_id}")
}

/// Wildcard for all retrieval events: `mp.v1.retrieval.>`
pub const RETRIEVAL_WILDCARD: &str = "mp.v1.retrieval.>";

// ---------------------------------------------------------------------------
// Fine-tuning lifecycle subjects (Wave 7 slice 2d).
//
// Emitted by `model-gateway::finetune_routes` and `finetune_poller` on every
// transition so the App-Plane projector (Convex) can wake immediately rather
// than poll. The `event_type` is one of: `created`, `transitioned`,
// `succeeded`, `failed`, `cancelled`, `deployed`.
// ---------------------------------------------------------------------------

/// Sanitize an arbitrary identifier so it is safe to embed as a single NATS
/// subject token. NATS reserves `.` (separator), `*` (single-token wildcard),
/// `>` (multi-token wildcard) and whitespace. Any reserved character is
/// replaced with `_` so an attacker (or sloppy upstream caller) cannot pivot
/// into an unintended subject hierarchy or wildcard subscription.
fn sanitize_nats_token(token: &str) -> String {
    token
        .chars()
        .map(|c| match c {
            '.' | '*' | '>' | ' ' | '\t' | '\n' | '\r' => '_',
            _ => c,
        })
        .collect()
}

/// Subject for fine-tuning lifecycle events: `mp.v1.finetune.{org_id}.{event_type}`.
///
/// `org_id` is sanitized via [`sanitize_nats_token`] to prevent subject
/// injection — see HIGH-2 in the Wave 7 review.
#[must_use]
pub fn finetune_event_subject(org_id: &str, event_type: &str) -> String {
    let safe_org_id = sanitize_nats_token(org_id);
    format!("{PREFIX}.finetune.{safe_org_id}.{event_type}")
}

/// Wildcard for all fine-tuning events: `mp.v1.finetune.>`
pub const FINETUNE_WILDCARD: &str = "mp.v1.finetune.>";

/// Event-type constants for fine-tuning subjects. Subscribers should treat
/// any unknown event type as forward-compatible (log + skip).
pub const FINETUNE_EVENT_CREATED: &str = "created";
pub const FINETUNE_EVENT_TRANSITIONED: &str = "transitioned";
pub const FINETUNE_EVENT_SUCCEEDED: &str = "succeeded";
pub const FINETUNE_EVENT_FAILED: &str = "failed";
pub const FINETUNE_EVENT_CANCELLED: &str = "cancelled";
pub const FINETUNE_EVENT_DEPLOYED: &str = "deployed";

// ---------------------------------------------------------------------------
// Orchestration subjects (mirror Go orchestrator-core/internal/orchestration)
// ---------------------------------------------------------------------------

/// Orchestration subject prefix: `mp.v1.orchestration`
pub const ORCHESTRATION_PREFIX: &str = "mp.v1.orchestration";
pub const SUBJECT_PLAN: &str = "mp.v1.orchestration.plan";
pub const SUBJECT_TODO: &str = "mp.v1.orchestration.todo";
pub const SUBJECT_APPROVAL: &str = "mp.v1.orchestration.approval";
pub const SUBJECT_SUBAGENT: &str = "mp.v1.orchestration.subagent";
pub const SUBJECT_RUN: &str = "mp.v1.orchestration.run";
/// Wildcard for all orchestration events: `mp.v1.orchestration.>`
pub const ORCHESTRATION_WILDCARD: &str = "mp.v1.orchestration.>";

pub const EVENT_PLAN_TRANSITIONED: &str = "plan.transitioned";
pub const EVENT_TODO_TRANSITIONED: &str = "todo.transitioned";
pub const EVENT_APPROVAL_STATE_CHANGED: &str = "approval.state_changed";
pub const EVENT_SUBAGENT_ATTACHED: &str = "subagent.attached";
pub const EVENT_SUBAGENT_STOPPED: &str = "subagent.stopped";
pub const EVENT_RUN_PAUSED_FOR_APPROVAL: &str = "run.paused_for_approval";
pub const EVENT_RUN_RESUMED_AFTER_APPROVAL: &str = "run.resumed_after_approval";

// ---------------------------------------------------------------------------
// Legacy subjects (compatibility window)
//
// These mirror the Go `pkg/natsx` Legacy helpers in `orchestrator-core` so
// Rust publishers and consumers can reference the same canonical strings
// during the migration from `velion.*` to `mp.v1.*`.
// ---------------------------------------------------------------------------

/// Legacy subject for run events: `velion.agent.run.{run_id}.event`
#[must_use]
pub fn legacy_run_event_subject(run_id: &str) -> String {
    format!("velion.agent.run.{run_id}.event")
}

/// Legacy subject for session commands: `velion.session.{session_key}.command`
#[must_use]
pub fn legacy_session_command_subject(session_key: &str) -> String {
    format!("velion.session.{session_key}.command")
}

/// Legacy wildcard for all run events: `velion.agent.run.*.event`
pub const LEGACY_RUN_EVENTS_WILDCARD: &str = "velion.agent.run.*.event";

/// Legacy wildcard for all session commands: `velion.session.*.command`
pub const LEGACY_SESSION_COMMAND_WILDCARD: &str = "velion.session.*.command";

// ---------------------------------------------------------------------------
// Legacy aqencia.* subjects (compat window)
// Mirror of go/pkg/natsx/compat.go aqencia.* LegacyMappings.
// ---------------------------------------------------------------------------
pub const LEGACY_AQENCIA_REASONING_STARTED: &str = "aqencia.reasoning.reasoning.started";
pub const LEGACY_AQENCIA_REASONING_COMPLETED: &str = "aqencia.reasoning.reasoning.completed";
pub const LEGACY_AQENCIA_USAGE_RECORDED: &str = "aqencia.reasoning.usage.recorded";
pub const LEGACY_AQENCIA_DECISION_MADE: &str = "aqencia.reasoning.decision.made";
pub const LEGACY_AQENCIA_QUOTA_EXCEEDED: &str = "aqencia.reasoning.quota.exceeded";
/// Wildcard for all aqencia reasoning subjects.
pub const LEGACY_AQENCIA_WILDCARD: &str = "aqencia.reasoning.>";

fn matches_pattern(subject: &str, pattern: &str) -> bool {
    let subject_parts: Vec<_> = subject.split('.').collect();
    let pattern_parts: Vec<_> = pattern.split('.').collect();

    if pattern_parts.last() == Some(&">") {
        if subject_parts.len() < pattern_parts.len() - 1 {
            return false;
        }

        return pattern_parts[..pattern_parts.len() - 1]
            .iter()
            .zip(subject_parts.iter())
            .all(|(pattern_part, subject_part)| *pattern_part == *subject_part);
    }

    if subject_parts.len() != pattern_parts.len() {
        return false;
    }

    pattern_parts
        .iter()
        .zip(subject_parts.iter())
        .all(|(pattern_part, subject_part)| *pattern_part == "*" || *pattern_part == *subject_part)
}

/// Translate a legacy subject to its canonical `mp.v1.*` form.
#[must_use]
pub fn translate_legacy_subject(legacy_subject: &str) -> String {
    if matches_pattern(legacy_subject, LEGACY_RUN_EVENTS_WILDCARD) {
        let parts: Vec<_> = legacy_subject.split('.').collect();
        return run_event_subject(parts[3]);
    }
    if matches_pattern(legacy_subject, LEGACY_SESSION_COMMAND_WILDCARD) {
        let parts: Vec<_> = legacy_subject.split('.').collect();
        return session_command_subject(parts[2]);
    }

    match legacy_subject {
        LEGACY_AQENCIA_REASONING_STARTED => ingress_subject("run_started_compat"),
        LEGACY_AQENCIA_REASONING_COMPLETED => ingress_subject("run_completed_compat"),
        LEGACY_AQENCIA_USAGE_RECORDED => ingress_subject("usage"),
        LEGACY_AQENCIA_DECISION_MADE => ingress_subject("decision"),
        LEGACY_AQENCIA_QUOTA_EXCEEDED => ingress_subject("quota_exceeded"),
        _ => legacy_subject.to_owned(),
    }
}

/// Translate a canonical `mp.v1.*` subject back to its legacy equivalent.
///
/// Returns `None` when no lossless reverse mapping exists.
#[must_use]
pub fn translate_new_to_legacy(v1_subject: &str) -> Option<String> {
    if matches_pattern(v1_subject, RUN_EVENTS_WILDCARD) {
        let parts: Vec<_> = v1_subject.split('.').collect();
        return Some(legacy_run_event_subject(parts[3]));
    }
    if matches_pattern(v1_subject, SESSION_COMMANDS_WILDCARD) {
        let parts: Vec<_> = v1_subject.split('.').collect();
        return Some(legacy_session_command_subject(parts[3]));
    }

    match v1_subject {
        "mp.v1.ingress.run_started_compat" => Some(LEGACY_AQENCIA_REASONING_STARTED.to_owned()),
        "mp.v1.ingress.run_completed_compat" => Some(LEGACY_AQENCIA_REASONING_COMPLETED.to_owned()),
        "mp.v1.ingress.usage" => Some(LEGACY_AQENCIA_USAGE_RECORDED.to_owned()),
        "mp.v1.ingress.decision" => Some(LEGACY_AQENCIA_DECISION_MADE.to_owned()),
        "mp.v1.ingress.quota_exceeded" => Some(LEGACY_AQENCIA_QUOTA_EXCEEDED.to_owned()),
        _ => None,
    }
}

/// Resolve subscribed subjects from a canonical subject and compatibility mode.
///
/// Mirrors Go subscriber semantics:
/// - `v1_only` and `dual_write`: subscribe canonical only
/// - `dual_read`: subscribe canonical + legacy mirror (if available)
/// - `legacy_only`: subscribe legacy mirror only
///
/// Non-canonical subjects (outside `mp.v1.*`) are returned unchanged for all
/// modes so service-local subjects keep working during cutover.
pub fn subscriber_subjects(
    canonical_subject: &str,
    mode: CompatMode,
) -> Result<Vec<String>, SubjectSelectionError> {
    if !canonical_subject.starts_with(PREFIX) {
        return Ok(vec![canonical_subject.to_owned()]);
    }

    match mode {
        CompatMode::V1Only | CompatMode::DualWrite => Ok(vec![canonical_subject.to_owned()]),
        CompatMode::DualRead => {
            let mut subjects = vec![canonical_subject.to_owned()];
            if let Some(legacy_subject) = translate_new_to_legacy(canonical_subject) {
                subjects.push(legacy_subject);
            }
            Ok(subjects)
        }
        CompatMode::LegacyOnly => translate_new_to_legacy(canonical_subject)
            .map(|legacy_subject| vec![legacy_subject])
            .ok_or_else(|| SubjectSelectionError::NoLegacyMapping {
                subject: canonical_subject.to_owned(),
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_event_subject_format() {
        let subject = run_event_subject("01HXYZ");
        assert_eq!(subject, "mp.v1.run.01HXYZ.event");
    }

    #[test]
    fn session_command_subject_format() {
        let subject = session_command_subject("01HABC");
        assert_eq!(subject, "mp.v1.session.01HABC.command");
    }

    #[test]
    fn legacy_run_event_subject_format() {
        assert_eq!(
            legacy_run_event_subject("01HXYZ"),
            "velion.agent.run.01HXYZ.event"
        );
    }

    #[test]
    fn legacy_session_command_subject_format() {
        assert_eq!(
            legacy_session_command_subject("01HABC"),
            "velion.session.01HABC.command"
        );
    }

    #[test]
    fn legacy_wildcards_match_go_constants() {
        assert_eq!(LEGACY_RUN_EVENTS_WILDCARD, "velion.agent.run.*.event");
        assert_eq!(LEGACY_SESSION_COMMAND_WILDCARD, "velion.session.*.command");
    }

    #[test]
    fn legacy_aqencia_subject_constants() {
        assert_eq!(
            LEGACY_AQENCIA_REASONING_STARTED,
            "aqencia.reasoning.reasoning.started"
        );
        assert_eq!(
            LEGACY_AQENCIA_REASONING_COMPLETED,
            "aqencia.reasoning.reasoning.completed"
        );
        assert_eq!(
            LEGACY_AQENCIA_USAGE_RECORDED,
            "aqencia.reasoning.usage.recorded"
        );
        assert_eq!(
            LEGACY_AQENCIA_DECISION_MADE,
            "aqencia.reasoning.decision.made"
        );
        assert_eq!(
            LEGACY_AQENCIA_QUOTA_EXCEEDED,
            "aqencia.reasoning.quota.exceeded"
        );
        assert_eq!(LEGACY_AQENCIA_WILDCARD, "aqencia.reasoning.>");
    }

    #[test]
    fn compat_mode_parse_matches_go_contract() {
        assert_eq!(CompatMode::parse(""), CompatMode::V1Only);
        assert_eq!(CompatMode::parse("v1_only"), CompatMode::V1Only);
        assert_eq!(CompatMode::parse("dual_write"), CompatMode::DualWrite);
        assert_eq!(CompatMode::parse("dual_read"), CompatMode::DualRead);
        assert_eq!(CompatMode::parse("legacy_only"), CompatMode::LegacyOnly);
        assert_eq!(CompatMode::parse("unknown"), CompatMode::V1Only);
    }

    #[test]
    fn translate_legacy_subject_matches_go_contract() {
        assert_eq!(
            translate_legacy_subject("velion.agent.run.01HXYZ.event"),
            "mp.v1.run.01HXYZ.event"
        );
        assert_eq!(
            translate_legacy_subject("velion.session.abc-123.command"),
            "mp.v1.session.abc-123.command"
        );
        assert_eq!(
            translate_legacy_subject(LEGACY_AQENCIA_REASONING_STARTED),
            "mp.v1.ingress.run_started_compat"
        );
        assert_eq!(
            translate_legacy_subject(LEGACY_AQENCIA_REASONING_COMPLETED),
            "mp.v1.ingress.run_completed_compat"
        );
        assert_eq!(
            translate_legacy_subject(LEGACY_AQENCIA_USAGE_RECORDED),
            "mp.v1.ingress.usage"
        );
        assert_eq!(
            translate_legacy_subject("some.unknown.subject"),
            "some.unknown.subject"
        );
    }

    #[test]
    fn translate_new_to_legacy_matches_go_contract() {
        assert_eq!(
            translate_new_to_legacy("mp.v1.run.01HXYZ.event"),
            Some("velion.agent.run.01HXYZ.event".to_owned())
        );
        assert_eq!(
            translate_new_to_legacy("mp.v1.session.abc-123.command"),
            Some("velion.session.abc-123.command".to_owned())
        );
        assert_eq!(
            translate_new_to_legacy("mp.v1.ingress.usage"),
            Some("aqencia.reasoning.usage.recorded".to_owned())
        );
        assert_eq!(
            translate_new_to_legacy("mp.v1.ingress.decision"),
            Some("aqencia.reasoning.decision.made".to_owned())
        );
        assert_eq!(
            translate_new_to_legacy("mp.v1.ingress.quota_exceeded"),
            Some("aqencia.reasoning.quota.exceeded".to_owned())
        );
        assert_eq!(translate_new_to_legacy("mp.v1.ingress.accepted"), None);
    }

    #[test]
    fn subscriber_subjects_match_go_subscriber_modes() {
        assert_eq!(
            subscriber_subjects("mp.v1.run.r1.event", CompatMode::V1Only).expect("v1_only"),
            vec!["mp.v1.run.r1.event".to_owned()]
        );
        assert_eq!(
            subscriber_subjects("mp.v1.run.r1.event", CompatMode::DualWrite).expect("dual_write"),
            vec!["mp.v1.run.r1.event".to_owned()]
        );
        assert_eq!(
            subscriber_subjects("mp.v1.run.r1.event", CompatMode::DualRead).expect("dual_read"),
            vec![
                "mp.v1.run.r1.event".to_owned(),
                "velion.agent.run.r1.event".to_owned(),
            ]
        );
        assert_eq!(
            subscriber_subjects("mp.v1.run.r1.event", CompatMode::LegacyOnly).expect("legacy_only"),
            vec!["velion.agent.run.r1.event".to_owned()]
        );
    }

    #[test]
    fn subscriber_subjects_keep_non_canonical_subjects_unchanged() {
        assert_eq!(
            subscriber_subjects("tools.completions.*", CompatMode::DualRead)
                .expect("non canonical"),
            vec!["tools.completions.*".to_owned()]
        );
    }

    #[test]
    fn subscriber_subjects_legacy_only_errors_when_no_mapping() {
        let error = subscriber_subjects("mp.v1.ingress.accepted", CompatMode::LegacyOnly)
            .expect_err("legacy_only should fail for unmapped subject");
        assert_eq!(
            error,
            SubjectSelectionError::NoLegacyMapping {
                subject: "mp.v1.ingress.accepted".to_owned(),
            }
        );
    }

    #[test]
    fn finetune_subject_sanitizes_nats_reserved_chars() {
        // org_id containing `.`, `*`, or `>` must not let the caller pivot
        // into a different subject hierarchy or wildcard.
        assert_eq!(
            finetune_event_subject("acme.evil", "created"),
            "mp.v1.finetune.acme_evil.created"
        );
        assert_eq!(
            finetune_event_subject("acme*", "created"),
            "mp.v1.finetune.acme_.created"
        );
        assert_eq!(
            finetune_event_subject("acme>foo", "created"),
            "mp.v1.finetune.acme_foo.created"
        );
        assert_eq!(
            finetune_event_subject("a b\tc", "created"),
            "mp.v1.finetune.a_b_c.created"
        );
        // Safe characters pass through unchanged.
        assert_eq!(
            finetune_event_subject("org_acme-123", "succeeded"),
            "mp.v1.finetune.org_acme-123.succeeded"
        );
    }

    #[test]
    fn retrieval_subjects_compose_correctly() {
        assert_eq!(
            retrieval_fetched_subject("org_acme"),
            "mp.v1.retrieval.fetched.org_acme"
        );
        assert_eq!(
            retrieval_used_subject("org_acme"),
            "mp.v1.retrieval.used.org_acme"
        );
        assert_eq!(
            retrieval_low_confidence_subject("org_acme"),
            "mp.v1.retrieval.low_confidence.org_acme"
        );
        assert_eq!(RETRIEVAL_WILDCARD, "mp.v1.retrieval.>");
    }
}
