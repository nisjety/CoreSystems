//! The agent directory resolver tool (A-REC-03, F-t1-049): `space.directory.resolve`.
//!
//! A room agent turns a name, a unique name prefix or an exact email into one person or agent of its organization. The answer is
//! `ok` (one match), `ambiguous` (at most five candidates) or `none` (with a reason). It is presence, never authority: a match
//! lets the agent add, mention, message or act for nobody. Acting on a person still goes through Control's room roster.
//!
//! The hop, each step failing closed:
//! 1. The rollout switch `SPACE_DIRECTORY_AGENT_TOOL` (off by default, never an authorization input) and a complete configuration.
//! 2. The arguments are parsed against the closed schema (`PARAMETERS_JSON`). The organization, the subject and the room come
//!    from the run, never from the model, so the model cannot look into another organization or name a room.
//! 3. Control's run-bound read (`POST /api/v1/internal/spaces/run-directory/people`, execution-core principal, scope
//!    `spaces:directory:run-read`). Control resolves the run through Session Core, requires the run subject to be a current
//!    member of the run's room and the golden row plus the operator allowlist to admit the action, then searches as that subject
//!    in that organization. It never calls the gateway's human resolver.
//! 4. [`resolve`] is a pure function over the people Control returned and the agent registry rows. It marks each candidate with
//!    `in_this_room` and never carries another room's name or reference.
//!
//! Zero Data Retention: a resolve persists nothing, so it is not refused. A zero-retention or waived run (or a room Control reports
//! as zero-retention) gets ids only: no name, email or description reaches the model.
//!
//! The agent half reads Application's agent registry. execution-core has no Application client yet, so the agent source is
//! unavailable in this build: `kind:"agent"` answers `none`/`directory_unavailable`, and `kind:"any"` marks its answer
//! `incomplete` (an outage is never "no such agent").
//!
//! The row is offered only when the golden registry names it (`space_self_api::is_space_action`) and the run's profile carries
//! the self-API marker; until then no run is offered the tool and the dispatcher never reaches this module.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::control_http_client::{
    bounded_secret, env_flag, env_value, service_base_url, service_endpoint as endpoint,
};
use crate::space_self_api::{CODE_INVALID_ARGUMENTS, CODE_NOT_AUTHORIZED, CODE_NOT_OFFERED};
use crate::tool_bridge::ToolExecution;

/// The golden row name.
pub(crate) const ACTION: &str = "space.directory.resolve";
/// The rollout switch. Unset or `0` offers and executes nothing. It never authorizes anything.
pub(crate) const SWITCH_ENV: &str = "SPACE_DIRECTORY_AGENT_TOOL";
/// The service token execution-core presents to Control's run directory read.
const TOKEN_ENV: &str = "EXECUTION_CORE_CONTROL_RUN_DIRECTORY_TOKEN";
const CONTROL_PATH: &str = "/api/v1/internal/spaces/run-directory/people";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REPLY_BYTES: usize = 256 << 10;
const MAX_QUERY_CHARS: usize = 100;
/// Candidates named in an `ambiguous` answer.
pub(crate) const MAX_CANDIDATES: usize = 5;
/// The model-facing reason when a directory source could not be read.
pub(crate) const REASON_UNAVAILABLE: &str = "directory_unavailable";
/// The model-facing reason when the directory was read and nothing matched.
pub(crate) const REASON_NO_MATCH: &str = "no_match";

/// The model-facing description of the tool, for the golden row.
pub(crate) const DESCRIPTION: &str = "Look up a person or an agent in your organization by full name, a unique name prefix or an exact email. \
Answers status ok with one match, ambiguous with up to 5 candidates to ask the user about, or none with a reason \
(no_match, or directory_unavailable when the directory could not be read). A match is presence only: it does not let you add, \
mention, message or act for anyone. in_this_room says whether the match is in the room you run in. Other rooms are never named. \
Under zero data retention only ids are returned.";

/// The closed argument schema for the golden row: `query` and an optional `kind`, nothing else (argument order `query`, `kind`).
pub(crate) const PARAMETERS_JSON: &str = r#"{"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":100,"description":"A full name, a unique name prefix, or an exact email."},"kind":{"type":"string","enum":["person","agent","any"],"description":"What to look for. Defaults to any."}},"required":["query"],"additionalProperties":false}"#;

/// What the model asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Person,
    Agent,
    Any,
}

impl Kind {
    fn people(self) -> bool {
        matches!(self, Self::Person | Self::Any)
    }

    fn agents(self) -> bool {
        matches!(self, Self::Agent | Self::Any)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Arguments {
    pub(crate) query: String,
    pub(crate) kind: Kind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    query: String,
    #[serde(default)]
    kind: Option<String>,
}

/// Parses the model's arguments against the closed schema. Unknown fields (a room, an org, a user) are refused.
pub(crate) fn parse_arguments(input: &str) -> Result<Arguments, String> {
    let parsed: Input =
        serde_json::from_str(input).map_err(|_| CODE_INVALID_ARGUMENTS.to_owned())?;
    let query = parsed.query.trim().to_owned();
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS || query.contains('\0') {
        return Err(CODE_INVALID_ARGUMENTS.to_owned());
    }
    let kind = match parsed.kind.as_deref().map(str::trim) {
        None | Some("" | "any") => Kind::Any,
        Some("person") => Kind::Person,
        Some("agent") => Kind::Agent,
        Some(_) => return Err(CODE_INVALID_ARGUMENTS.to_owned()),
    };
    Ok(Arguments { query, kind })
}

/// One organization member as Control returned it for the run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct PersonEntry {
    pub(crate) subject_id: String,
    #[serde(default)]
    pub(crate) display_name: String,
    #[serde(default)]
    pub(crate) email: String,
    #[serde(default)]
    pub(crate) in_room: bool,
}

/// One published agent definition of the organization, with whether the run's room holds it. No room is named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentEntry {
    pub(crate) agent_ref: String,
    pub(crate) display_name: String,
    pub(crate) description: String,
    pub(crate) in_room: bool,
}

/// A resolver candidate, flattened for matching.
struct Candidate {
    json: Value,
    name: String,
    email: String,
}

impl Candidate {
    fn exact(&self, needle: &str) -> bool {
        self.name.to_lowercase() == needle
            || (!self.email.is_empty() && self.email.to_lowercase() == needle)
    }

    fn prefix(&self, needle: &str) -> bool {
        self.name.to_lowercase().starts_with(needle)
            || (!self.email.is_empty() && self.email.to_lowercase().starts_with(needle))
    }
}

fn person_json(person: &PersonEntry, ids_only: bool) -> Value {
    if ids_only {
        return json!({ "kind": "person", "subject_id": person.subject_id, "in_this_room": person.in_room });
    }
    json!({
        "kind": "person",
        "subject_id": person.subject_id,
        "display_name": person.display_name,
        "email": person.email,
        "in_this_room": person.in_room,
    })
}

fn agent_json(agent: &AgentEntry, ids_only: bool) -> Value {
    if ids_only {
        return json!({ "kind": "agent", "agent_ref": agent.agent_ref, "in_this_room": agent.in_room });
    }
    json!({
        "kind": "agent",
        "agent_ref": agent.agent_ref,
        "display_name": agent.display_name,
        "description": agent.description,
        "in_this_room": agent.in_room,
    })
}

/// The resolver, a pure function. `people` and `agents` are `None` when that source could not be read; a source the `kind` does
/// not ask for is ignored. Exact (case-insensitive) full name or email wins, else a name or email prefix. One match in the best
/// tier is `ok`, several are `ambiguous` (at most [`MAX_CANDIDATES`]), none is `none` with `no_match`, or `directory_unavailable`
/// when a source it needed could not be read. A match found while another requested source was unreadable carries
/// `incomplete:true`. With `ids_only` no name, email or description is returned.
#[must_use]
pub(crate) fn resolve(
    arguments: &Arguments,
    people: Option<&[PersonEntry]>,
    agents: Option<&[AgentEntry]>,
    ids_only: bool,
) -> Value {
    let needle = arguments.query.trim().to_lowercase();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut incomplete = false;
    if arguments.kind.people() {
        match people {
            Some(people) => candidates.extend(
                people
                    .iter()
                    .filter(|person| !person.subject_id.trim().is_empty())
                    .map(|person| Candidate {
                        json: person_json(person, ids_only),
                        name: person.display_name.clone(),
                        email: person.email.clone(),
                    }),
            ),
            None => incomplete = true,
        }
    }
    if arguments.kind.agents() {
        match agents {
            Some(agents) => candidates.extend(
                agents
                    .iter()
                    .filter(|agent| !agent.agent_ref.trim().is_empty())
                    .map(|agent| Candidate {
                        json: agent_json(agent, ids_only),
                        name: agent.display_name.clone(),
                        email: String::new(),
                    }),
            ),
            None => incomplete = true,
        }
    }
    let exact: Vec<&Candidate> = candidates.iter().filter(|c| c.exact(&needle)).collect();
    let best = if exact.is_empty() {
        candidates.iter().filter(|c| c.prefix(&needle)).collect()
    } else {
        exact
    };
    let mut answer = match best.as_slice() {
        [] => {
            let reason = if incomplete {
                REASON_UNAVAILABLE
            } else {
                REASON_NO_MATCH
            };
            return json!({ "status": "none", "reason": reason, "presence_only": true });
        }
        [one] => json!({ "status": "ok", "match": one.json, "presence_only": true }),
        many => json!({
            "status": "ambiguous",
            "candidates": many.iter().take(MAX_CANDIDATES).map(|c| c.json.clone()).collect::<Vec<_>>(),
            "presence_only": true,
        }),
    };
    if incomplete {
        answer["incomplete"] = json!(true);
    }
    answer
}

// ---------------------------------------------------------------------------------------------------------------------------------
// The adapter.
// ---------------------------------------------------------------------------------------------------------------------------------

struct DirectoryClient {
    control_base_url: Url,
    token: String,
    http: Client,
}

static SHARED: OnceLock<Result<Option<DirectoryClient>, String>> = OnceLock::new();

fn shared() -> Result<Option<&'static DirectoryClient>, String> {
    match SHARED.get_or_init(DirectoryClient::from_env).as_ref() {
        Ok(client) => Ok(client.as_ref()),
        Err(detail) => Err(detail.clone()),
    }
}

/// Whether the rollout switch is on (`SPACE_DIRECTORY_AGENT_TOOL`).
#[must_use]
pub(crate) fn enabled() -> bool {
    mp_contracts::agentic_subscription::switch_value_is_on(
        std::env::var(SWITCH_ENV).ok().as_deref(),
    )
}

impl DirectoryClient {
    fn from_env() -> Result<Option<Self>, String> {
        if !enabled() {
            return Ok(None);
        }
        let (Some(control), Some(token)) = (
            env_value("CONTROL_PLANE_USER_CORE_URL"),
            env_value(TOKEN_ENV),
        ) else {
            return Err(format!(
                "{SWITCH_ENV} requires CONTROL_PLANE_USER_CORE_URL and {TOKEN_ENV} together"
            ));
        };
        let allow_insecure = env_flag("EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK");
        let control_base_url = service_base_url(&control, "Control Plane", allow_insecure)?;
        let token = bounded_secret(&token, "Control run directory service token")?;
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|_| "Space directory HTTP client is unavailable".to_owned())?;
        Ok(Some(Self {
            control_base_url,
            token,
            http,
        }))
    }
}

/// Whether a name is the directory row.
#[must_use]
pub(crate) fn is_directory_action(name: &str) -> bool {
    name == ACTION
}

/// Whether the view may admit the directory row: the switch is on and the configuration is complete.
#[must_use]
pub(crate) fn is_built_action(name: &str) -> bool {
    is_directory_action(name) && matches!(shared(), Ok(Some(_)))
}

#[derive(Serialize)]
struct ControlRequest<'a> {
    run_id: &'a str,
    org_id: &'a str,
    query: &'a str,
}

#[derive(Deserialize)]
struct ControlEnvelope {
    data: ControlPeople,
}

#[derive(Deserialize)]
struct ControlPeople {
    #[serde(default)]
    people: Vec<PersonEntry>,
    #[serde(default)]
    zero_data_retention: bool,
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
}

/// What Control's run directory read produced.
#[derive(Debug, PartialEq, Eq)]
enum PeopleRead {
    /// The people and whether Control reports the room as zero-retention.
    Read(Vec<PersonEntry>, bool),
    /// Control could not be read: an outage is never an empty list.
    Unavailable,
}

/// Maps one Control reply. A refusal is a model-facing code; an outage is [`PeopleRead::Unavailable`].
fn map_control_reply(status: StatusCode, body: &[u8]) -> Result<PeopleRead, String> {
    if status.is_success() {
        return Ok(serde_json::from_slice::<ControlEnvelope>(body)
            .map(|envelope| {
                PeopleRead::Read(envelope.data.people, envelope.data.zero_data_retention)
            })
            .unwrap_or(PeopleRead::Unavailable));
    }
    if status.is_server_error() {
        return Ok(PeopleRead::Unavailable);
    }
    let code = serde_json::from_slice::<ErrorEnvelope>(body)
        .map(|envelope| envelope.error.code)
        .unwrap_or_default();
    Err(match (status.as_u16(), code.as_str()) {
        (_, "not_offered") | (404, _) => CODE_NOT_OFFERED,
        (400, _) => CODE_INVALID_ARGUMENTS,
        _ => CODE_NOT_AUTHORIZED,
    }
    .to_owned())
}

async fn read_people(
    client: &DirectoryClient,
    run_id: &str,
    org_id: &str,
    query: &str,
) -> Result<PeopleRead, String> {
    let Ok(url) = endpoint(&client.control_base_url, CONTROL_PATH) else {
        return Ok(PeopleRead::Unavailable);
    };
    let Ok(response) = client
        .http
        .post(url)
        .timeout(CONTROL_TIMEOUT)
        .header("x-service-id", "execution-core")
        .header("x-service-token", &client.token)
        .json(&ControlRequest {
            run_id,
            org_id,
            query,
        })
        .send()
        .await
    else {
        return Ok(PeopleRead::Unavailable);
    };
    let status = response.status();
    let Ok(body) = response.bytes().await else {
        return Ok(PeopleRead::Unavailable);
    };
    if body.len() > MAX_REPLY_BYTES {
        return Ok(PeopleRead::Unavailable);
    }
    map_control_reply(status, &body)
}

async fn execute(
    tool_input: &str,
    run_id: &str,
    org_id: &str,
    retention_bound: bool,
) -> Result<String, String> {
    let client = match shared() {
        Ok(Some(client)) => client,
        Ok(None) | Err(_) => return Err(CODE_NOT_OFFERED.to_owned()),
    };
    let arguments = parse_arguments(tool_input)?;
    let mut ids_only = retention_bound;
    let people = if arguments.kind.people() {
        match read_people(client, run_id, org_id, &arguments.query).await? {
            PeopleRead::Read(people, zdr) => {
                ids_only |= zdr;
                Some(people)
            }
            PeopleRead::Unavailable => None,
        }
    } else {
        None
    };
    // No Application agent registry client in this build: the agent source is unavailable (see the module note).
    let agents: Option<&[AgentEntry]> = None;
    Ok(resolve(&arguments, people.as_deref(), agents, ids_only).to_string())
}

/// Executes one `space.directory.resolve` call for the run. `retention_bound` is a zero-retention or waived run.
pub(crate) async fn execute_if_configured(
    tool_input: &str,
    run_id: &str,
    org_id: &str,
    retention_bound: bool,
) -> ToolExecution {
    match execute(tool_input, run_id, org_id, retention_bound).await {
        Ok(output) => ToolExecution {
            output,
            error: None,
        },
        Err(code) => ToolExecution {
            output: String::new(),
            error: Some(code),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(id: &str, name: &str, email: &str, in_room: bool) -> PersonEntry {
        PersonEntry {
            subject_id: id.to_owned(),
            display_name: name.to_owned(),
            email: email.to_owned(),
            in_room,
        }
    }

    fn agent(id: &str, name: &str, in_room: bool) -> AgentEntry {
        AgentEntry {
            agent_ref: id.to_owned(),
            display_name: name.to_owned(),
            description: "Drafts replies".to_owned(),
            in_room,
        }
    }

    fn args(query: &str, kind: Kind) -> Arguments {
        Arguments {
            query: query.to_owned(),
            kind,
        }
    }

    fn people() -> Vec<PersonEntry> {
        vec![
            person("u-ada", "Ada Lovelace", "ada@example.test", true),
            person("u-adam", "Adam Smith", "adam@example.test", false),
            person("u-bob", "Bob Stone", "bob@example.test", true),
        ]
    }

    #[test]
    fn exact_name_or_email_is_ok() {
        let all = people();
        let answer = resolve(&args("ada lovelace", Kind::Person), Some(&all), None, false);
        assert_eq!(answer["status"], "ok");
        assert_eq!(answer["match"]["subject_id"], "u-ada");
        assert_eq!(answer["match"]["in_this_room"], true);
        assert_eq!(answer["presence_only"], true);
        let answer = resolve(
            &args("ADAM@example.test", Kind::Person),
            Some(&all),
            None,
            false,
        );
        assert_eq!(answer["match"]["subject_id"], "u-adam");
        assert_eq!(answer["match"]["in_this_room"], false);
    }

    #[test]
    fn exact_beats_prefix_and_a_unique_prefix_is_ok() {
        let mut all = people();
        all.push(person("u-ada2", "Ada", "", false));
        let answer = resolve(&args("ada", Kind::Person), Some(&all), None, false);
        assert_eq!(answer["status"], "ok");
        assert_eq!(answer["match"]["subject_id"], "u-ada2");
        let answer = resolve(&args("bo", Kind::Person), Some(&people()), None, false);
        assert_eq!(answer["match"]["subject_id"], "u-bob");
    }

    #[test]
    fn several_in_the_best_tier_are_ambiguous_and_capped_at_five() {
        let all: Vec<PersonEntry> = (0..8)
            .map(|i| person(&format!("u-{i}"), &format!("Kim {i}"), "", i % 2 == 0))
            .collect();
        let answer = resolve(&args("kim", Kind::Any), Some(&all), Some(&[]), false);
        assert_eq!(answer["status"], "ambiguous");
        assert_eq!(
            answer["candidates"].as_array().map(Vec::len),
            Some(MAX_CANDIDATES)
        );
        assert!(answer.get("incomplete").is_none());
    }

    #[test]
    fn nothing_is_none_with_a_reason() {
        let answer = resolve(&args("zed", Kind::Person), Some(&people()), None, false);
        assert_eq!(answer["status"], "none");
        assert_eq!(answer["reason"], REASON_NO_MATCH);
        // An unreadable source is never "no such person".
        let answer = resolve(&args("zed", Kind::Person), None, None, false);
        assert_eq!(answer["reason"], REASON_UNAVAILABLE);
        let answer = resolve(&args("zed", Kind::Agent), Some(&people()), None, false);
        assert_eq!(answer["reason"], REASON_UNAVAILABLE);
    }

    #[test]
    fn a_match_with_an_unreadable_source_is_marked_incomplete() {
        let answer = resolve(&args("bob stone", Kind::Any), Some(&people()), None, false);
        assert_eq!(answer["status"], "ok");
        assert_eq!(answer["incomplete"], true);
    }

    #[test]
    fn agents_resolve_and_kind_filters() {
        let agents = vec![
            agent("agent-1", "Ada Helper", true),
            agent("agent-2", "Triage", false),
        ];
        let answer = resolve(
            &args("triage", Kind::Agent),
            Some(&people()),
            Some(&agents),
            false,
        );
        assert_eq!(answer["match"]["kind"], "agent");
        assert_eq!(answer["match"]["agent_ref"], "agent-2");
        assert_eq!(answer["match"]["in_this_room"], false);
        // `person` never returns an agent, and `agent` never a person.
        let answer = resolve(
            &args("ada helper", Kind::Person),
            Some(&people()),
            Some(&agents),
            false,
        );
        assert_eq!(answer["status"], "none");
        let answer = resolve(
            &args("ada", Kind::Any),
            Some(&people()),
            Some(&agents),
            false,
        );
        assert_eq!(answer["status"], "ambiguous");
    }

    #[test]
    fn ids_only_drops_names_emails_and_descriptions() {
        let agents = vec![agent("agent-1", "Ada Helper", true)];
        let answer = resolve(
            &args("ada", Kind::Any),
            Some(&people()),
            Some(&agents),
            true,
        );
        let text = answer.to_string();
        for leaked in [
            "Ada Lovelace",
            "ada@example.test",
            "Adam Smith",
            "Ada Helper",
            "Drafts replies",
        ] {
            assert!(!text.contains(leaked), "{leaked} leaked: {text}");
        }
        assert_eq!(answer["status"], "ambiguous");
        assert!(text.contains("u-ada") && text.contains("agent-1"));
    }

    #[test]
    fn no_answer_names_a_room() {
        let agents = vec![agent("agent-1", "Helper", true)];
        for kind in [Kind::Person, Kind::Agent, Kind::Any] {
            let text = resolve(&args("a", kind), Some(&people()), Some(&agents), false).to_string();
            assert!(
                !text.contains("space_ref") && !text.contains("installed_in"),
                "{text}"
            );
        }
    }

    #[test]
    fn arguments_are_closed() {
        assert_eq!(
            parse_arguments(r#"{"query":" Ada ","kind":"person"}"#),
            Ok(args("Ada", Kind::Person))
        );
        assert_eq!(
            parse_arguments(r#"{"query":"Ada"}"#),
            Ok(args("Ada", Kind::Any))
        );
        for bad in [
            r#"{"query":"Ada","space_ref":"room-2"}"#,
            r#"{"query":"Ada","org_id":"other"}"#,
            r#"{"query":"   "}"#,
            r#"{"query":"Ada","kind":"room"}"#,
            r#"{"kind":"person"}"#,
            "not json",
        ] {
            assert_eq!(
                parse_arguments(bad),
                Err(CODE_INVALID_ARGUMENTS.to_owned()),
                "{bad}"
            );
        }
        let long = format!(r#"{{"query":"{}"}}"#, "a".repeat(101));
        assert_eq!(
            parse_arguments(&long),
            Err(CODE_INVALID_ARGUMENTS.to_owned())
        );
    }

    #[test]
    fn parameters_are_a_closed_object_naming_each_argument() {
        let schema: Value = serde_json::from_str(PARAMETERS_JSON).expect("parameters");
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        let properties = schema["properties"].as_object().expect("properties");
        assert_eq!(properties.len(), 2);
        for name in ["query", "kind"] {
            assert_eq!(properties[name]["type"], "string");
        }
        // Control's registry check refuses these keywords and these name parts.
        for forbidden in [
            "\"format\"",
            "$ref",
            "member",
            "role",
            "space",
            "org",
            "user",
            "actor",
        ] {
            assert!(!PARAMETERS_JSON.contains(forbidden), "{forbidden}");
        }
        assert!(!DESCRIPTION.trim().is_empty());
    }

    #[test]
    fn control_replies_map_to_model_codes() {
        let ok = br#"{"data":{"people":[{"subject_id":"u-1","display_name":"A","email":"a@x.test","in_room":true}],"zero_data_retention":true}}"#;
        assert_eq!(
            map_control_reply(StatusCode::OK, ok),
            Ok(PeopleRead::Read(
                vec![person("u-1", "A", "a@x.test", true)],
                true
            ))
        );
        assert_eq!(
            map_control_reply(StatusCode::OK, b"{}"),
            Ok(PeopleRead::Unavailable)
        );
        assert_eq!(
            map_control_reply(StatusCode::SERVICE_UNAVAILABLE, b""),
            Ok(PeopleRead::Unavailable)
        );
        let refusal =
            |code: &str| format!(r#"{{"error":{{"code":"{code}","message":"m","details":{{}}}}}}"#);
        assert_eq!(
            map_control_reply(StatusCode::FORBIDDEN, refusal("not_offered").as_bytes()),
            Err(CODE_NOT_OFFERED.to_owned())
        );
        assert_eq!(
            map_control_reply(StatusCode::FORBIDDEN, refusal("not_authorized").as_bytes()),
            Err(CODE_NOT_AUTHORIZED.to_owned())
        );
        assert_eq!(
            map_control_reply(
                StatusCode::BAD_REQUEST,
                refusal("invalid_request").as_bytes()
            ),
            Err(CODE_INVALID_ARGUMENTS.to_owned())
        );
    }

    #[test]
    fn switched_off_the_tool_is_not_offered_and_not_built() {
        if enabled() {
            return;
        }
        assert!(!is_built_action(ACTION));
        assert!(is_directory_action(ACTION));
        assert!(!is_directory_action("space.canvas.read"));
    }
}
