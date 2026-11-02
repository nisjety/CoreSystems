//! Identity-bound mentions and addressed agent wake-up, BFF half (build plan WS08-S4).
//!
//! A human post may carry `mentions:{people:[{subject_id,label,source?}], agents:[{subject_id,
//! source?}]}`: ids, never text. [`plan_mentions`] is the one pure planner both post routes call
//! (through `human_posts`): it checks the caps, the ids, that every typed `@label` is visible in
//! the post, that a person is an active `user` of the Space and an agent an active bound `service`,
//! and decides for each agent whether a wake is `expected` or `refused` (and why). The plan is
//! advisory: Session Core validates and records the set and has the last word (a non-owner's agent
//! mention is downgraded there), and the wake itself needs a Control-signed `model.run.wake`
//! decision. Nothing here is an authorization input and nothing here reads a database.
//!
//! The planner never resolves a person or an agent by name: a label is only checked for being
//! visible in the text, so two members with the same display name stay distinct subjects.

use serde_json::{json, Map, Value};

use super::evaluate_agent_wake_policy;

/// Most people one post may mention.
pub(crate) const MAX_PEOPLE_MENTIONS: usize = 20;
/// Most agents one post may mention (and so most wakes of one post).
pub(crate) const MAX_AGENT_MENTIONS: usize = 3;
/// The longest label the planner reads.
const MAX_LABEL_CHARS: usize = 120;

/// `message_mentions.refusal_code` for an agent mention in a thread the poster does not own: only
/// the thread owner's runs may be started in it (Session Core enforces the same rule).
pub(crate) const REFUSAL_OWNER_ONLY_THREAD: &str = "owner_only_thread";

/// Why a `mentions` object is refused outright (400 `invalid_mentions`, `details.reason`). The
/// first four are the plan's closed set; `BadSource` and `Malformed` are the shape failures the
/// plan left implicit (DEVIATION recorded in the depth file).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InvalidMentions {
    TooManyPeople,
    TooManyAgents,
    Duplicate,
    BadSubjectId,
    BadSource,
    Malformed,
}

impl InvalidMentions {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::TooManyPeople => "too_many_people",
            Self::TooManyAgents => "too_many_agents",
            Self::Duplicate => "duplicate",
            Self::BadSubjectId => "bad_subject_id",
            Self::BadSource => "bad_source",
            Self::Malformed => "malformed",
        }
    }
}

/// How the mention got into the post: typed by the sender (its `@label` must be visible in the
/// text) or carried by the address lock (the agent is addressed without being typed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Typed,
    Locked,
}

impl Source {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Typed => "typed",
            Self::Locked => "locked",
        }
    }

    fn parse(value: Option<&Value>) -> Result<Self, InvalidMentions> {
        match value {
            None | Some(Value::Null) => Ok(Self::Typed),
            Some(Value::String(word)) => match word.as_str() {
                "typed" => Ok(Self::Typed),
                "locked" => Ok(Self::Locked),
                _ => Err(InvalidMentions::BadSource),
            },
            Some(_) => Err(InvalidMentions::BadSource),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PersonInput {
    pub(crate) subject_id: String,
    pub(crate) label: String,
    pub(crate) source: Source,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentInput {
    pub(crate) subject_id: String,
    pub(crate) source: Source,
}

/// The browser's `mentions` object after shape validation. Caps and ids are checked by
/// [`plan_mentions`], so one table covers them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MentionsInput {
    pub(crate) people: Vec<PersonInput>,
    pub(crate) agents: Vec<AgentInput>,
}

impl MentionsInput {
    /// Shape-checks the untrusted `mentions` value: an object with only `people` and `agents`,
    /// each an array of objects with only the documented string fields. Arrays longer than twice
    /// their cap are refused before they are walked.
    pub(crate) fn parse(value: &Value) -> Result<Self, InvalidMentions> {
        let object = value.as_object().ok_or(InvalidMentions::Malformed)?;
        if object.keys().any(|key| key != "people" && key != "agents") {
            return Err(InvalidMentions::Malformed);
        }
        let entries = |key: &str, cap: usize, too_many: InvalidMentions| match object.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) if items.len() > cap => Err(too_many),
            Some(Value::Array(items)) => Ok(items.clone()),
            Some(_) => Err(InvalidMentions::Malformed),
        };
        let people_items = entries("people", MAX_PEOPLE_MENTIONS, InvalidMentions::TooManyPeople)?;
        let agent_items = entries("agents", MAX_AGENT_MENTIONS, InvalidMentions::TooManyAgents)?;

        let mut people = Vec::with_capacity(people_items.len());
        for item in &people_items {
            let fields = entry_fields(item, &["subject_id", "label", "source"])?;
            let label = match fields.get("label") {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(label)) => label.trim().to_owned(),
                Some(_) => return Err(InvalidMentions::Malformed),
            };
            if label.chars().count() > MAX_LABEL_CHARS {
                return Err(InvalidMentions::Malformed);
            }
            people.push(PersonInput {
                subject_id: subject_id_field(fields)?,
                label,
                source: Source::parse(fields.get("source"))?,
            });
        }
        let mut agents = Vec::with_capacity(agent_items.len());
        for item in &agent_items {
            let fields = entry_fields(item, &["subject_id", "source"])?;
            agents.push(AgentInput {
                subject_id: subject_id_field(fields)?,
                source: Source::parse(fields.get("source"))?,
            });
        }
        Ok(Self { people, agents })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.people.is_empty() && self.agents.is_empty()
    }
}

fn entry_fields<'a>(
    item: &'a Value,
    allowed: &[&str],
) -> Result<&'a Map<String, Value>, InvalidMentions> {
    let fields = item.as_object().ok_or(InvalidMentions::Malformed)?;
    if fields.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(InvalidMentions::Malformed);
    }
    Ok(fields)
}

fn subject_id_field(fields: &Map<String, Value>) -> Result<String, InvalidMentions> {
    match fields.get("subject_id") {
        Some(Value::String(id)) => Ok(id.clone()),
        _ => Err(InvalidMentions::Malformed),
    }
}

/// `^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`, the same id pattern Session Core accepts.
pub(crate) fn valid_subject_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id.len() <= 128
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

/// `^[A-Za-z0-9_-]{16,64}$`: the browser's per-(post, agent) wake id.
pub(crate) fn valid_client_wake_id(id: &str) -> bool {
    (16..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The wake the browser asks for: the stored post it answers and its own retry id. Both are
/// claims until Control signs the decision that binds them and Session Core compares the post.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WakeIntent {
    pub(crate) trigger_message_id: String,
    pub(crate) client_wake_id: String,
    /// The Control subject id of the agent, taken from the verified mention, never from `wake`.
    pub(crate) agent_subject_id: String,
}

/// Reads the browser's `wake:{message_id, client_wake_id}` object. `None` when it is not exactly
/// that shape (extra keys included), so a forged field is a 400, never silently ignored.
pub(crate) fn parse_wake_fields(raw: &Value) -> Option<(String, String)> {
    let object = raw.as_object()?;
    if object.len() != 2 {
        return None;
    }
    let message_id = object.get("message_id")?.as_str()?;
    let client_wake_id = object.get("client_wake_id")?.as_str()?;
    (valid_subject_id(message_id) && valid_client_wake_id(client_wake_id))
        .then(|| (message_id.to_owned(), client_wake_id.to_owned()))
}

/// Who owns the thread the post lands in. Only the owner's runs can be started in a thread, so an
/// agent mention by anyone else is recorded as refused `owner_only_thread`. `Unknown` (a new
/// thread, or a route that has not read it) plans no downgrade: Session Core decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThreadOwner {
    Invoker,
    Other,
    Unknown,
}

/// The poster: Control subject id and live Space role.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Invoker<'a> {
    pub(crate) subject_id: &'a str,
    pub(crate) role: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MentionKind {
    User,
    Agent,
}

impl MentionKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    Expected,
    /// Recorded, no run may start; the word is the `refusal_code` enum.
    Refused(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Recorded {
    pub(crate) subject_id: String,
    pub(crate) kind: MentionKind,
    pub(crate) source: Source,
    /// Agents only.
    pub(crate) wake: Option<Wake>,
    /// Position among the recorded mentions of its kind.
    pub(crate) ordinal: u32,
}

/// A mention that was not recorded; the post still stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rejected {
    pub(crate) subject_id: String,
    /// `person_not_in_room`, `agent_not_bound` or `mention_text_missing`.
    pub(crate) code: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MentionPlan {
    pub(crate) recorded: Vec<Recorded>,
    pub(crate) rejected: Vec<Rejected>,
}

impl MentionPlan {
    /// The `mentions` entries forwarded to model-gateway's post routes (Session Core's
    /// `MentionInput`): ids and closed words only.
    pub(crate) fn session_entries(&self) -> Vec<Value> {
        self.recorded
            .iter()
            .map(|mention| {
                let mut entry = json!({
                    "subject_id": mention.subject_id,
                    "kind": mention.kind.as_str(),
                    "source": mention.source.as_str(),
                    "ordinal": mention.ordinal,
                });
                match mention.wake {
                    Some(Wake::Expected) => entry["wake_expectation"] = json!("expected"),
                    Some(Wake::Refused(reason)) => {
                        entry["wake_expectation"] = json!("refused");
                        entry["refusal_code"] = json!(reason);
                    }
                    None => {}
                }
                entry
            })
            .collect()
    }

    pub(crate) fn rejected_json(&self) -> Vec<Value> {
        self.rejected
            .iter()
            .map(|rejected| json!({ "subject_id": rejected.subject_id, "code": rejected.code }))
            .collect()
    }
}

/// `data.mentions.recorded` from the receipt model-gateway returned for the post. The receipt is
/// the truth about what was recorded (with the kill switches off it carries none), so a plan that
/// Session Core did not honour is never reported as recorded. An agent's `wake` is `refused` when
/// Session Core says so and `expected` otherwise; a person has none.
pub(crate) fn recorded_from_receipt(receipt: &Value) -> Vec<Value> {
    receipt
        .get("mentions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let subject_id = entry.get("subject_id").and_then(Value::as_str)?;
            let kind = entry.get("kind").and_then(Value::as_str)?;
            let mut out = json!({ "subject_id": subject_id, "kind": kind });
            if kind == "agent" {
                let refused = entry.get("wake_state").and_then(Value::as_str) == Some("refused");
                out["wake"] = json!(if refused { "refused" } else { "expected" });
                if let Some(reason) = entry
                    .get("reason")
                    .and_then(Value::as_str)
                    .filter(|reason| !reason.is_empty())
                {
                    out["reason"] = json!(reason);
                }
            }
            if let Some(ordinal) = entry.get("ordinal").and_then(Value::as_u64) {
                out["ordinal"] = json!(ordinal);
            }
            Some(out)
        })
        .collect()
}

/// Lower-cased text for the visible-text check. DEVIATION: the plan asks for case-folded NFC; the
/// gateway has no normalization dependency, so only case is folded (a composed/decomposed
/// difference reads as `mention_text_missing`, which fails closed for a mention).
fn fold(text: &str) -> String {
    text.to_lowercase()
}

fn text_has_mention(folded_content: &str, label: &str) -> bool {
    let label = fold(label.trim().trim_start_matches('@').trim());
    !label.is_empty() && folded_content.contains(&format!("@{label}"))
}

fn is_active(member: &Value) -> bool {
    member
        .get("status")
        .and_then(Value::as_str)
        .map_or(true, |status| status == "active")
}

/// Plans the mentions of one human post. Pure: `roster` is Control's Space roster members
/// (`subject_type` `user` | `service`, no email), `agents` the composed agent list (roster
/// services joined with their Application binding: `subject_id`, `status`, `name`, policy fields).
///
/// # Errors
/// An `InvalidMentions` for a cap, a malformed id or a duplicate; every per-mention problem is a
/// soft `rejected` entry instead, because a mention never fails the post.
pub(crate) fn plan_mentions(
    content: &str,
    input: &MentionsInput,
    roster: &[Value],
    agents: &[Value],
    invoker: &Invoker<'_>,
    thread_owner: ThreadOwner,
) -> Result<MentionPlan, InvalidMentions> {
    if input.people.len() > MAX_PEOPLE_MENTIONS {
        return Err(InvalidMentions::TooManyPeople);
    }
    if input.agents.len() > MAX_AGENT_MENTIONS {
        return Err(InvalidMentions::TooManyAgents);
    }
    let mut seen = std::collections::HashSet::new();
    for id in input
        .people
        .iter()
        .map(|person| person.subject_id.as_str())
        .chain(input.agents.iter().map(|agent| agent.subject_id.as_str()))
    {
        if !valid_subject_id(id) {
            return Err(InvalidMentions::BadSubjectId);
        }
        if !seen.insert(id) {
            return Err(InvalidMentions::Duplicate);
        }
    }

    let folded = fold(content);
    let mut plan = MentionPlan::default();

    for person in &input.people {
        let in_room = roster.iter().any(|member| {
            member.get("subject_type").and_then(Value::as_str) == Some("user")
                && member.get("subject_id").and_then(Value::as_str) == Some(person.subject_id.as_str())
                && is_active(member)
        });
        if !in_room {
            plan.rejected.push(Rejected {
                subject_id: person.subject_id.clone(),
                code: "person_not_in_room",
            });
            continue;
        }
        if person.source == Source::Typed && !text_has_mention(&folded, &person.label) {
            plan.rejected.push(Rejected {
                subject_id: person.subject_id.clone(),
                code: "mention_text_missing",
            });
            continue;
        }
        let ordinal = plan
            .recorded
            .iter()
            .filter(|mention| mention.kind == MentionKind::User)
            .count();
        plan.recorded.push(Recorded {
            subject_id: person.subject_id.clone(),
            kind: MentionKind::User,
            source: person.source,
            wake: None,
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
        });
    }

    for agent_input in &input.agents {
        let bound = agents.iter().find(|agent| {
            agent.get("subject_id").and_then(Value::as_str) == Some(agent_input.subject_id.as_str())
                && agent.get("status").and_then(Value::as_str) == Some("active")
        });
        let Some(agent) = bound else {
            plan.rejected.push(Rejected {
                subject_id: agent_input.subject_id.clone(),
                code: "agent_not_bound",
            });
            continue;
        };
        let label = agent
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(agent_input.subject_id.as_str());
        if agent_input.source == Source::Typed && !text_has_mention(&folded, label) {
            plan.rejected.push(Rejected {
                subject_id: agent_input.subject_id.clone(),
                code: "mention_text_missing",
            });
            continue;
        }
        let wake = match evaluate_agent_wake_policy(agent, Some(invoker.role), invoker.subject_id) {
            Err(refusal) => Wake::Refused(refusal.reason()),
            Ok(()) if thread_owner == ThreadOwner::Other => Wake::Refused(REFUSAL_OWNER_ONLY_THREAD),
            Ok(()) => Wake::Expected,
        };
        let ordinal = plan
            .recorded
            .iter()
            .filter(|mention| mention.kind == MentionKind::Agent)
            .count();
        plan.recorded.push(Recorded {
            subject_id: agent_input.subject_id.clone(),
            kind: MentionKind::Agent,
            source: agent_input.source,
            wake: Some(wake),
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
        });
    }
    Ok(plan)
}

/// Wakes per user per Space per minute. A test run shares one process-wide limiter and one user,
/// so the ceiling is lifted there.
const WAKES_PER_MINUTE: f64 = if cfg!(test) { 100_000.0 } else { 10.0 };

/// Process-wide limiter, built on first use from the gateway's Dragonfly connection (in-process
/// fallback when the cache is off), like `human_posts`' post ceiling. DEVIATION: the plan names an
/// `AppState` field; a static keeps every test `AppState` literal unchanged.
static WAKE_LIMITER: std::sync::OnceLock<crate::rate_limit::RateLimiter> = std::sync::OnceLock::new();

/// One wake token for (user, Space), or the Retry-After seconds. FAIL-OPEN by design (the limiter
/// falls back to its in-process bucket when Dragonfly errors): the hard backstop against a flood of
/// runs is Session Core's per-thread run slot, not this bucket.
pub(crate) async fn check_wake_ceiling(
    state: &crate::config::AppState,
    org_id: &str,
    user_id: &str,
    space_ref: &str,
) -> Result<(), u64> {
    let limiter = WAKE_LIMITER.get_or_init(|| {
        crate::rate_limit::RateLimiter::from_cache_with_rpm(&state.cache, WAKES_PER_MINUTE)
    });
    limiter
        .try_acquire(&format!("space-wake:{}:{user_id}:{}", org_id.trim(), space_ref.trim()))
        .await
}
