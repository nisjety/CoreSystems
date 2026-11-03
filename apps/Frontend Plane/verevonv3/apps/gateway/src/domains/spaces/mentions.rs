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

#[cfg(test)]
mod tests {
    //! WS08-S4 tests: the pure planner, the shape parser, the wake-field parser and the receipt
    //! reader. No network and no state.
    use super::*;

    fn member(subject_id: &str) -> Value {
        json!({"subject_type": "user", "subject_id": subject_id, "role": "editor", "status": "active"})
    }

    fn agent(subject_id: &str, name: &str) -> Value {
        json!({"subject_id": subject_id, "name": name, "status": "active"})
    }

    fn people(items: &[(&str, &str)]) -> MentionsInput {
        MentionsInput {
            people: items
                .iter()
                .map(|(id, label)| PersonInput {
                    subject_id: (*id).to_owned(),
                    label: (*label).to_owned(),
                    source: Source::Typed,
                })
                .collect(),
            agents: Vec::new(),
        }
    }

    fn agents(items: &[&str], source: Source) -> MentionsInput {
        MentionsInput {
            people: Vec::new(),
            agents: items
                .iter()
                .map(|id| AgentInput {
                    subject_id: (*id).to_owned(),
                    source,
                })
                .collect(),
        }
    }

    fn invoker(role: &str) -> Invoker<'_> {
        Invoker {
            subject_id: "user-1",
            role,
        }
    }

    fn plan_people(content: &str, input: &MentionsInput, roster: &[Value]) -> MentionPlan {
        plan_mentions(content, input, roster, &[], &invoker("editor"), ThreadOwner::Invoker)
            .expect("a valid set plans")
    }

    #[test]
    fn a_person_who_is_not_in_the_room_is_rejected_softly() {
        let plan = plan_people("hi @Ada", &people(&[("u-ada", "Ada")]), &[member("u-bob")]);
        assert!(plan.recorded.is_empty());
        assert_eq!(
            plan.rejected,
            vec![Rejected {
                subject_id: "u-ada".to_owned(),
                code: "person_not_in_room"
            }]
        );
        // a removed member and a service with the same id are not people of the room either
        let removed = json!({"subject_type": "user", "subject_id": "u-ada", "status": "removed"});
        let service = json!({"subject_type": "service", "subject_id": "u-ada", "status": "active"});
        for roster in [vec![removed], vec![service]] {
            let plan = plan_people("hi @Ada", &people(&[("u-ada", "Ada")]), &roster);
            assert_eq!(plan.rejected[0].code, "person_not_in_room");
        }
    }

    #[test]
    fn a_typed_mention_needs_its_label_visible_in_the_text() {
        let roster = [member("u-ada")];
        let input = people(&[("u-ada", "Ada")]);
        let plan = plan_people("hello there", &input, &roster);
        assert!(plan.recorded.is_empty());
        assert_eq!(plan.rejected[0].code, "mention_text_missing");
        // case is folded and a leading @ on the label is tolerated
        let plan = plan_people("ping @ADA please", &people(&[("u-ada", "@Ada")]), &roster);
        assert_eq!(plan.recorded.len(), 1);
        assert_eq!(plan.recorded[0].kind, MentionKind::User);
        assert_eq!(plan.recorded[0].wake, None);
        // an empty label can never match
        let plan = plan_people("hi @", &people(&[("u-ada", "  ")]), &roster);
        assert_eq!(plan.rejected[0].code, "mention_text_missing");
    }

    #[test]
    fn two_members_with_the_same_display_name_stay_distinct_subjects() {
        let roster = [member("u-ada-1"), member("u-ada-2")];
        let input = people(&[("u-ada-1", "Ada"), ("u-ada-2", "Ada")]);
        let plan = plan_people("@Ada and @Ada", &input, &roster);
        let ids: Vec<&str> = plan.recorded.iter().map(|m| m.subject_id.as_str()).collect();
        assert_eq!(ids, ["u-ada-1", "u-ada-2"]);
        assert_eq!(plan.recorded[0].ordinal, 0);
        assert_eq!(plan.recorded[1].ordinal, 1);
    }

    #[test]
    fn the_caps_are_twenty_people_and_three_agents() {
        let twenty: Vec<(String, String)> = (0..20).map(|i| (format!("u-{i}"), format!("n{i}"))).collect();
        let roster: Vec<Value> = twenty.iter().map(|(id, _)| member(id)).collect();
        let text: String = twenty.iter().map(|(_, l)| format!("@{l} ")).collect();
        let refs: Vec<(&str, &str)> = twenty.iter().map(|(i, l)| (i.as_str(), l.as_str())).collect();
        let plan = plan_people(&text, &people(&refs), &roster);
        assert_eq!(plan.recorded.len(), 20, "20 people are allowed");

        let mut over = refs.clone();
        over.push(("u-21", "n21"));
        let result = plan_mentions("x", &people(&over), &roster, &[], &invoker("owner"), ThreadOwner::Invoker);
        assert_eq!(result, Err(InvalidMentions::TooManyPeople));

        let result = plan_mentions(
            "x",
            &agents(&["a1", "a2", "a3", "a4"], Source::Locked),
            &[],
            &[],
            &invoker("owner"),
            ThreadOwner::Invoker,
        );
        assert_eq!(result, Err(InvalidMentions::TooManyAgents));
        let three = [agent("a1", "A1"), agent("a2", "A2"), agent("a3", "A3")];
        let plan = plan_mentions(
            "x",
            &agents(&["a1", "a2", "a3"], Source::Locked),
            &[],
            &three,
            &invoker("owner"),
            ThreadOwner::Invoker,
        )
        .unwrap();
        assert_eq!(plan.recorded.len(), 3, "3 agents are allowed");
    }

    #[test]
    fn a_duplicate_or_malformed_subject_id_refuses_the_whole_set() {
        let roster = [member("u-ada")];
        let duplicate = people(&[("u-ada", "Ada"), ("u-ada", "Ada")]);
        assert_eq!(
            plan_mentions("@Ada", &duplicate, &roster, &[], &invoker("owner"), ThreadOwner::Invoker),
            Err(InvalidMentions::Duplicate)
        );
        // a person and an agent sharing one id is a duplicate too
        let mixed = MentionsInput {
            people: people(&[("same", "Ada")]).people,
            agents: agents(&["same"], Source::Locked).agents,
        };
        assert_eq!(
            plan_mentions("@Ada", &mixed, &roster, &[], &invoker("owner"), ThreadOwner::Invoker),
            Err(InvalidMentions::Duplicate)
        );
        for bad in ["", "-leading", "has space", "slash/id", "a".repeat(129).as_str()] {
            let result = plan_mentions(
                "@Ada",
                &people(&[(bad, "Ada")]),
                &roster,
                &[],
                &invoker("owner"),
                ThreadOwner::Invoker,
            );
            assert_eq!(result, Err(InvalidMentions::BadSubjectId), "{bad:?}");
        }
        assert!(valid_subject_id("a"));
        assert!(valid_subject_id("User_1.x:y-z"));
        assert!(valid_subject_id(&"a".repeat(128)));
    }

    #[test]
    fn a_locked_source_skips_the_text_check() {
        let roster = [member("u-ada")];
        let input = MentionsInput {
            people: vec![PersonInput {
                subject_id: "u-ada".to_owned(),
                label: "Ada".to_owned(),
                source: Source::Locked,
            }],
            agents: Vec::new(),
        };
        let plan = plan_people("no mention typed at all", &input, &roster);
        assert_eq!(plan.recorded.len(), 1);
        assert_eq!(plan.recorded[0].source, Source::Locked);

        // an address-locked agent is recorded without being typed; a typed one is not
        let helper = [agent("svc-1", "Helper")];
        let locked = plan_mentions("hello", &agents(&["svc-1"], Source::Locked), &[], &helper, &invoker("owner"), ThreadOwner::Invoker).unwrap();
        assert_eq!(locked.recorded[0].wake, Some(Wake::Expected));
        let typed = plan_mentions("hello", &agents(&["svc-1"], Source::Typed), &[], &helper, &invoker("owner"), ThreadOwner::Invoker).unwrap();
        assert_eq!(typed.rejected[0].code, "mention_text_missing");
        let typed = plan_mentions("hi @helper", &agents(&["svc-1"], Source::Typed), &[], &helper, &invoker("owner"), ThreadOwner::Invoker).unwrap();
        assert_eq!(typed.recorded.len(), 1);
    }

    #[test]
    fn an_agent_that_is_not_bound_or_not_active_is_rejected() {
        let inactive = json!({"subject_id": "svc-1", "name": "Helper", "status": "disabled"});
        for list in [vec![], vec![inactive]] {
            let plan = plan_mentions("@Helper", &agents(&["svc-1"], Source::Typed), &[], &list, &invoker("owner"), ThreadOwner::Invoker).unwrap();
            assert!(plan.recorded.is_empty());
            assert_eq!(plan.rejected[0].code, "agent_not_bound");
        }
    }

    #[test]
    fn respond_to_decides_the_wake_expectation_per_role() {
        let helper = |respond_to: &str, subjects: Value| {
            json!({"subject_id": "svc-1", "name": "Helper", "status": "active",
                   "respond_to": respond_to, "respond_to_subjects": subjects})
        };
        let wake_for = |agent: &Value, role: &str, caller: &str| {
            let invoker = Invoker { subject_id: caller, role };
            let plan = plan_mentions(
                "@Helper",
                &agents(&["svc-1"], Source::Typed),
                &[],
                std::slice::from_ref(agent),
                &invoker,
                ThreadOwner::Invoker,
            )
            .unwrap();
            plan.recorded[0].wake
        };
        let denied = Some(Wake::Refused("respond_to_denied"));
        // managers: only owner and manager
        let managers = helper("managers", json!([]));
        assert_eq!(wake_for(&managers, "owner", "u"), Some(Wake::Expected));
        assert_eq!(wake_for(&managers, "manager", "u"), Some(Wake::Expected));
        assert_eq!(wake_for(&managers, "editor", "u"), denied);
        assert_eq!(wake_for(&managers, "viewer", "u"), denied);
        // members: editors too, viewers never
        let members = helper("members", json!([]));
        assert_eq!(wake_for(&members, "editor", "u"), Some(Wake::Expected));
        assert_eq!(wake_for(&members, "viewer", "u"), denied);
        // allowlist: an editor only when listed
        let listed = helper("allowlist", json!(["u-listed"]));
        assert_eq!(wake_for(&listed, "editor", "u-listed"), Some(Wake::Expected));
        assert_eq!(wake_for(&listed, "editor", "u-other"), denied);
        assert_eq!(wake_for(&listed, "viewer", "u-listed"), denied);
        assert_eq!(wake_for(&listed, "manager", "u-other"), Some(Wake::Expected));
    }

    #[test]
    fn binding_policy_refusals_carry_their_closed_words() {
        let wake_for = |agent: Value| {
            plan_mentions(
                "@Helper",
                &agents(&["svc-1"], Source::Typed),
                &[],
                &[agent],
                &invoker("owner"),
                ThreadOwner::Invoker,
            )
            .unwrap()
            .recorded[0]
                .wake
        };
        assert_eq!(
            wake_for(json!({"subject_id": "svc-1", "name": "Helper", "status": "active", "trigger_modes": ["group"]})),
            Some(Wake::Refused("mention_not_enabled"))
        );
        assert_eq!(
            wake_for(json!({"subject_id": "svc-1", "name": "Helper", "status": "active", "approval_mode": "blocked"})),
            Some(Wake::Refused("invocation_blocked"))
        );
    }

    #[test]
    fn a_thread_owned_by_someone_else_refuses_the_wake_as_owner_only_thread() {
        let helper = [agent("svc-1", "Helper")];
        let plan_for = |owner: ThreadOwner| {
            plan_mentions("@Helper", &agents(&["svc-1"], Source::Typed), &[], &helper, &invoker("owner"), owner)
                .unwrap()
                .recorded[0]
                .wake
        };
        assert_eq!(plan_for(ThreadOwner::Other), Some(Wake::Refused(REFUSAL_OWNER_ONLY_THREAD)));
        assert_eq!(REFUSAL_OWNER_ONLY_THREAD, "owner_only_thread");
        assert_eq!(plan_for(ThreadOwner::Invoker), Some(Wake::Expected));
        // unknown plans no downgrade: Session Core decides
        assert_eq!(plan_for(ThreadOwner::Unknown), Some(Wake::Expected));
        // a binding refusal outranks the thread fact
        let blocked = [json!({"subject_id": "svc-1", "name": "Helper", "status": "active", "approval_mode": "blocked"})];
        let plan = plan_mentions("@Helper", &agents(&["svc-1"], Source::Typed), &[], &blocked, &invoker("owner"), ThreadOwner::Other).unwrap();
        assert_eq!(plan.recorded[0].wake, Some(Wake::Refused("invocation_blocked")));
    }

    #[test]
    fn session_entries_carry_ids_and_closed_words_only() {
        let roster = [member("u-ada")];
        let helper = [agent("svc-1", "Helper")];
        let input = MentionsInput {
            people: people(&[("u-ada", "Ada")]).people,
            agents: agents(&["svc-1"], Source::Typed).agents,
        };
        let plan = plan_mentions("@Ada @Helper", &input, &roster, &helper, &invoker("owner"), ThreadOwner::Other).unwrap();
        assert_eq!(
            plan.session_entries(),
            vec![
                json!({"subject_id": "u-ada", "kind": "user", "source": "typed", "ordinal": 0}),
                json!({"subject_id": "svc-1", "kind": "agent", "source": "typed", "ordinal": 0,
                       "wake_expectation": "refused", "refusal_code": "owner_only_thread"}),
            ]
        );
        let plan = plan_mentions("@Helper", &agents(&["svc-1"], Source::Typed), &[], &helper, &invoker("owner"), ThreadOwner::Invoker).unwrap();
        assert_eq!(plan.session_entries()[0]["wake_expectation"], "expected");
        assert!(plan.session_entries()[0].get("refusal_code").is_none());
        let plan = plan_people("hi", &people(&[("u-gone", "Gone")]), &[]);
        assert_eq!(plan.rejected_json(), vec![json!({"subject_id": "u-gone", "code": "person_not_in_room"})]);
    }

    #[test]
    fn mentions_input_parse_accepts_the_documented_shapes() {
        let parsed = MentionsInput::parse(&json!({
            "people": [{"subject_id": "u-1", "label": "  Ada  "}, {"subject_id": "u-2", "label": "Bob", "source": "locked"}],
            "agents": [{"subject_id": "svc-1", "source": null}]
        }))
        .unwrap();
        assert_eq!(parsed.people[0].label, "Ada", "labels are trimmed");
        assert_eq!(parsed.people[0].source, Source::Typed, "source defaults to typed");
        assert_eq!(parsed.people[1].source, Source::Locked);
        assert_eq!(parsed.agents[0].source, Source::Typed);
        assert!(!parsed.is_empty());
        assert!(MentionsInput::parse(&json!({})).unwrap().is_empty());
        assert!(MentionsInput::parse(&json!({"people": null, "agents": null})).unwrap().is_empty());
        // a person without a label parses (a locked one needs none)
        assert_eq!(MentionsInput::parse(&json!({"people": [{"subject_id": "u-1"}]})).unwrap().people[0].label, "");
    }

    #[test]
    fn mentions_input_parse_refuses_wrong_shapes() {
        let reason = |value: Value| MentionsInput::parse(&value).unwrap_err().reason();
        assert_eq!(reason(json!("text")), "malformed");
        assert_eq!(reason(json!([])), "malformed");
        assert_eq!(reason(json!({"people": [], "extra": 1})), "malformed");
        assert_eq!(reason(json!({"people": {}})), "malformed");
        assert_eq!(reason(json!({"people": ["u-1"]})), "malformed");
        assert_eq!(reason(json!({"people": [{"subject_id": "u-1", "email": "a@b.c"}]})), "malformed");
        assert_eq!(reason(json!({"people": [{"subject_id": 7}]})), "malformed");
        assert_eq!(reason(json!({"people": [{"subject_id": "u-1", "label": 7}]})), "malformed");
        assert_eq!(reason(json!({"people": [{"subject_id": "u-1", "label": "x".repeat(121)}]})), "malformed");
        assert_eq!(reason(json!({"agents": [{"subject_id": "s", "label": "Ada"}]})), "malformed");
        assert_eq!(reason(json!({"agents": [{"subject_id": "s", "source": "other"}]})), "bad_source");
        assert_eq!(reason(json!({"agents": [{"subject_id": "s", "source": 3}]})), "bad_source");
        let many_people: Vec<Value> = (0..21).map(|i| json!({"subject_id": format!("u-{i}")})).collect();
        assert_eq!(reason(json!({"people": many_people})), "too_many_people");
        let many_agents: Vec<Value> = (0..4).map(|i| json!({"subject_id": format!("s-{i}")})).collect();
        assert_eq!(reason(json!({"agents": many_agents})), "too_many_agents");
        assert_eq!(InvalidMentions::Duplicate.reason(), "duplicate");
        assert_eq!(InvalidMentions::BadSubjectId.reason(), "bad_subject_id");
    }

    #[test]
    fn parse_wake_fields_accepts_exactly_message_id_and_client_wake_id() {
        let wake_id = "wake_id_0123456789";
        assert_eq!(
            parse_wake_fields(&json!({"message_id": "msg-1", "client_wake_id": wake_id})),
            Some(("msg-1".to_owned(), wake_id.to_owned()))
        );
        // an extra key is a forged field, not ignorable
        assert_eq!(
            parse_wake_fields(&json!({"message_id": "msg-1", "client_wake_id": wake_id, "agent": "x"})),
            None
        );
        assert_eq!(parse_wake_fields(&json!({"message_id": "msg-1"})), None);
        assert_eq!(parse_wake_fields(&json!("msg-1")), None);
        assert_eq!(parse_wake_fields(&json!({"message_id": 1, "client_wake_id": wake_id})), None);
        assert_eq!(parse_wake_fields(&json!({"message_id": "bad id", "client_wake_id": wake_id})), None);
        assert_eq!(parse_wake_fields(&json!({"message_id": "msg-1", "client_wake_id": "short"})), None);
        assert_eq!(
            parse_wake_fields(&json!({"message_id": "msg-1", "client_wake_id": "has.dot_0123456789"})),
            None
        );
        assert!(valid_client_wake_id(&"a".repeat(16)));
        assert!(valid_client_wake_id(&"a".repeat(64)));
        assert!(!valid_client_wake_id(&"a".repeat(65)));
        assert!(!valid_client_wake_id(&"a".repeat(15)));
    }

    #[test]
    fn recorded_from_receipt_reports_what_session_core_recorded() {
        let receipt = json!({"thread_id": "t", "mentions": [
            {"subject_id": "u-1", "kind": "user", "ordinal": 0},
            {"subject_id": "svc-1", "kind": "agent", "wake_state": "refused", "reason": "respond_to_denied", "ordinal": 0},
            {"subject_id": "svc-2", "kind": "agent", "wake_state": "pending", "reason": "", "ordinal": 1},
            {"kind": "user"},
            {"subject_id": "u-9"}
        ]});
        assert_eq!(
            recorded_from_receipt(&receipt),
            vec![
                json!({"subject_id": "u-1", "kind": "user", "ordinal": 0}),
                json!({"subject_id": "svc-1", "kind": "agent", "wake": "refused", "reason": "respond_to_denied", "ordinal": 0}),
                json!({"subject_id": "svc-2", "kind": "agent", "wake": "expected", "ordinal": 1}),
            ]
        );
        // with the kill switches off the receipt carries none: nothing is reported as recorded
        assert!(recorded_from_receipt(&json!({"thread_id": "t"})).is_empty());
        assert!(recorded_from_receipt(&json!({"mentions": "x"})).is_empty());
    }
}
