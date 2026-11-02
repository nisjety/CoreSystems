//! WS05-S2 - telling capability-core which MCP tools the gateway discovered.
//!
//! Discovery already knows each tool's name, description, schema and the
//! server's own side-effect hints. The registry is the system of record for
//! what a tool may do, so after a discovery the gateway posts one observation
//! per tool to capability-core: the tool's digest, its description and schema
//! hashes, and the server's hints. Capability-core pins the first digest it
//! sees, derives a class from the hints (tightening only), and quarantines a
//! tool whose digest later changes.
//!
//! The write is best effort and never part of a turn: it runs on a spawned task
//! with a short timeout, a failure is logged and retried after a back-off, and
//! it is skipped for a ZDR caller, for a disabled switch and for a gateway with
//! no capability-core address or service secret. Nothing here carries a
//! description, a schema, an argument or an output.
//!
//! A 2xx answer is not by itself success: capability-core answers 200 with one
//! outcome per tool. A tool that was `contended` (it lost a race four times and
//! the registry did not write it) is retried after the back-off like a failed
//! post; one the registry `rejected` is settled, because posting the same tool
//! again cannot change the answer, and is logged once with its reason code.
//!
//! Authority: the route is capability-core's org-derived HMAC internal route
//! (`X-Mcp-Service-Token`, the same credential the OAuth token routes use). That
//! proves "the gateway, for this org" and nothing more: the route can create or
//! refresh an observation. It cannot approve a changed tool, lower a class or
//! touch a policy, which are user-bearer actions on other routes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use hmac::{Hmac, Mac};
use mp_contracts::model_plane::v1::McpToolMeta;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::mcp_jsonrpc::McpToolDef;

/// The switch. Only the literal `true` enables it, so a typo or an empty value
/// leaves the gateway observing nothing. Default off until the route is proven
/// on a running stack.
pub const OBSERVATION_ENV: &str = "MCP_TOOL_OBSERVATION_ENABLED";

const OBSERVE_PATH: &str = "/api/v1/internal/mcp/tools/observe";
const SERVICE_TOKEN_HEADER: &str = "X-Mcp-Service-Token";

/// Most observations one request carries. Capability-core refuses more, so the
/// two sides agree on the bound (`maxObservationsPerBatch` in `mcp_tools.go`).
const MAX_BATCH: usize = 200;

/// How long the post may take. A slow registry costs the registry write, never
/// the turn that triggered it.
const POST_TIMEOUT: Duration = Duration::from_secs(3);

/// A tool whose last post succeeded is posted again after this long even when
/// nothing changed, so the registry's observation window (one hour by default)
/// keeps seeing a tool that is still there.
const REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// A failed post is not retried sooner than this, so a registry that is down
/// does not cost one spawned task per tool listing.
const FAILURE_BACKOFF: Duration = Duration::from_secs(30);

/// Whether observation is switched on in this process.
#[must_use]
pub fn observation_enabled() -> bool {
    enabled_from(std::env::var(OBSERVATION_ENV).ok().as_deref())
}

fn enabled_from(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether a tool listing may report to the registry: the switch is on and the
/// caller is positively not ZDR (`None` is a posture that could not be read).
/// Observation is a durable write, so a ZDR caller never causes one, and an
/// unreadable posture is treated as ZDR rather than guessed.
#[must_use]
pub fn should_observe(switch_on: bool, zdr: Option<bool>) -> bool {
    switch_on && zdr == Some(false)
}

/// The four hints the server may annotate a tool with. `title` is deliberately
/// absent: it is free text from the server, and the registry stores no text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ObservedHints {
    pub read_only_hint: Option<bool>,
    pub destructive_hint: Option<bool>,
    pub idempotent_hint: Option<bool>,
    pub open_world_hint: Option<bool>,
}

/// One discovered tool, reduced to what the registry may store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolObservation {
    pub server_id: String,
    pub tool_name: String,
    pub qualified_name: String,
    pub digest: String,
    pub description_sha256: String,
    pub schema_sha256: String,
    pub declared_side_effect: String,
    pub annotations: ObservedHints,
}

impl ToolObservation {
    /// Reduce a discovered tool of `server_id` to its observation. The
    /// qualified name is built here exactly as the exposure bridge builds the
    /// model-facing name, so the registry and the offered tool list agree.
    #[must_use]
    pub fn from_tool(server_id: &str, tool: &McpToolDef) -> Self {
        let annotations = &tool.annotations;
        Self {
            server_id: server_id.to_owned(),
            tool_name: tool.name.clone(),
            qualified_name: format!("mcp__{server_id}__{}", tool.name),
            digest: tool.digest(),
            description_sha256: tool.description_sha256(),
            schema_sha256: tool.schema_sha256(),
            declared_side_effect: annotations.side_effect().as_wire().to_owned(),
            annotations: ObservedHints {
                read_only_hint: annotations.read_only_hint,
                destructive_hint: annotations.destructive_hint,
                idempotent_hint: annotations.idempotent_hint,
                open_world_hint: annotations.open_world_hint,
            },
        }
    }

    /// The wire form `ListMcpTools` returns beside the tool definitions.
    #[must_use]
    pub fn to_meta(&self) -> McpToolMeta {
        McpToolMeta {
            qualified_name: self.qualified_name.clone(),
            server_id: self.server_id.clone(),
            tool_name: self.tool_name.clone(),
            digest: self.digest.clone(),
            declared_side_effect: self.declared_side_effect.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
struct ObservationBatch<'a> {
    observations: &'a [ToolObservation],
}

/// One tool's answer in capability-core's reply (`observationResult` in
/// `mcp_tools.go`). Only the typed outcome and the short reason code are read;
/// the registry echoes nothing from the request.
#[derive(Debug, Deserialize)]
struct ObservationOutcome {
    outcome: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ObservationReply {
    #[serde(default)]
    results: Vec<ObservationOutcome>,
}

/// How a post went, tool by tool.
#[derive(Debug, Default, PartialEq, Eq)]
struct PostReport {
    /// Tools the registry answered for good: recorded, or rejected (retrying a
    /// rejected tool does not change the answer).
    settled: Vec<ToolObservation>,
    /// Tools to post again after the back-off: `contended`, an outcome this
    /// gateway does not know, or any tool of a batch that failed or whose answer
    /// could not be read.
    retry: Vec<ToolObservation>,
    /// The reason codes of the tools the registry rejected, one per tool.
    rejected: Vec<String>,
    /// The first batch failure. Never the credential or a response body.
    error: Option<String>,
}

impl PostReport {
    fn absorb(&mut self, batch: &[ToolObservation], answers: Vec<ObservationOutcome>) {
        for (tool, answer) in batch.iter().zip(answers) {
            match answer.outcome.as_str() {
                "recorded" => self.settled.push(tool.clone()),
                "rejected" => {
                    self.settled.push(tool.clone());
                    self.rejected.push(reason_code(&answer.reason));
                }
                // `contended` and anything this gateway does not recognise: the
                // registry did not take the observation, so ask again later.
                _ => self.retry.push(tool.clone()),
            }
        }
    }
}

/// A reason is logged only if it looks like one of capability-core's short
/// codes; anything else is not echoed.
fn reason_code(reason: &str) -> String {
    let well_formed = !reason.is_empty()
        && reason.len() <= 64
        && reason
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if well_formed {
        reason.to_owned()
    } else {
        "unspecified".to_owned()
    }
}

struct Posted {
    digest: String,
    at: Instant,
    succeeded: bool,
}

/// What the gateway has already told the registry, so a tool listing does not
/// repeat a post that is still fresh. Per process: a restart or another replica
/// posts again, which the registry treats as a refresh.
#[derive(Clone, Default)]
pub struct ObservationLedger {
    posted: Arc<DashMap<(String, String), Posted>>,
}

impl ObservationLedger {
    /// The observations that need posting now: never posted, a different digest
    /// from the last post, past the refresh interval, or a failed post whose
    /// back-off has run out.
    #[must_use]
    pub fn due(&self, org_id: &str, observations: &[ToolObservation]) -> Vec<ToolObservation> {
        observations
            .iter()
            .filter(|observation| {
                let key = (org_id.to_owned(), observation.qualified_name.clone());
                match self.posted.get(&key) {
                    None => true,
                    Some(last) => {
                        last.digest != observation.digest
                            || last.at.elapsed()
                                >= if last.succeeded {
                                    REFRESH_INTERVAL
                                } else {
                                    FAILURE_BACKOFF
                                }
                    }
                }
            })
            .cloned()
            .collect()
    }

    fn record(&self, org_id: &str, observations: &[ToolObservation], succeeded: bool) {
        for observation in observations {
            self.posted.insert(
                (org_id.to_owned(), observation.qualified_name.clone()),
                Posted {
                    digest: observation.digest.clone(),
                    at: Instant::now(),
                    succeeded,
                },
            );
        }
    }
}

/// The twin of capability-core's `expectedInternalServiceToken` (and of
/// `mcp_oauth::derive_org_scoped_service_token`): lowercase hex of
/// HMAC-SHA256 keyed by the root secret over the trimmed org id. The root
/// secret never crosses the wire; what does cross proves "the gateway, for this
/// org". Both ends pin the same known-answer vector, so drift fails a test
/// instead of 403-ing every post.
fn derive_org_scoped_service_token(root_secret: &str, org_id: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(root_secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(org_id.trim().as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).expect("nibble is < 16"));
        hex.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("nibble is < 16"));
    }
    hex
}

/// Post one batch and read capability-core's per-tool answer.
///
/// # Errors
/// Returns a short description when the batch could not be sent, was refused
/// (any non-2xx status, including a 5xx), or was answered with something that is
/// not one outcome per tool. It never contains the credential or a response body.
async fn post_batch(
    client: &reqwest::Client,
    url: &reqwest::Url,
    token: &str,
    batch: &[ToolObservation],
) -> Result<Vec<ObservationOutcome>, String> {
    let response = client
        .post(url.clone())
        .header(SERVICE_TOKEN_HEADER, token)
        .timeout(POST_TIMEOUT)
        .json(&ObservationBatch {
            observations: batch,
        })
        .send()
        .await
        .map_err(|error| format!("observation post failed: {}", error.without_url()))?;
    if !response.status().is_success() {
        return Err(format!(
            "observation post refused with status {}",
            response.status()
        ));
    }
    let reply: ObservationReply = response
        .json()
        .await
        .map_err(|_| "observation answer could not be read".to_owned())?;
    if reply.results.len() != batch.len() {
        return Err(format!(
            "observation answered {} of {} tools",
            reply.results.len(),
            batch.len()
        ));
    }
    Ok(reply.results)
}

/// Post `observations` for `org_id` to capability-core, in batches the registry
/// accepts, and say what became of each tool. A batch that fails puts its tools
/// and every later batch's tools in `retry` and stops posting: a refusal that
/// repeats (a wrong credential) would only repeat.
///
/// # Errors
/// Returns a short description when the capability-core address is not a URL, in
/// which case nothing was sent.
async fn post_observations(
    client: &reqwest::Client,
    capability_core_base_url: &str,
    service_token: &str,
    org_id: &str,
    observations: &[ToolObservation],
) -> Result<PostReport, String> {
    let mut url = reqwest::Url::parse(&format!("{capability_core_base_url}{OBSERVE_PATH}"))
        .map_err(|_| "capability-core address is not a URL".to_owned())?;
    url.query_pairs_mut().append_pair("org_id", org_id);
    let token = derive_org_scoped_service_token(service_token, org_id);
    let mut report = PostReport::default();
    let mut batches = observations.chunks(MAX_BATCH);
    while let Some(batch) = batches.next() {
        match post_batch(client, &url, &token, batch).await {
            Ok(answers) => report.absorb(batch, answers),
            Err(error) => {
                report.error.get_or_insert(error);
                report.retry.extend(batch.iter().cloned());
                for rest in batches.by_ref() {
                    report.retry.extend(rest.iter().cloned());
                }
                break;
            }
        }
    }
    Ok(report)
}

/// Tell capability-core about the tools that need telling, without making the
/// caller wait. Does nothing when `observations` is empty, when the gateway has
/// no capability-core address or service secret, or when every tool was posted
/// recently. The caller has already decided that this caller may be observed
/// (switch on, not ZDR).
pub fn spawn_observation(
    ledger: &ObservationLedger,
    client: &reqwest::Client,
    capability_core_base_url: &str,
    service_token: &str,
    org_id: &str,
    observations: &[ToolObservation],
) {
    if capability_core_base_url.is_empty() || service_token.is_empty() || org_id.trim().is_empty() {
        return;
    }
    let due = ledger.due(org_id, observations);
    if due.is_empty() {
        return;
    }
    // Marked before the task runs, as a failure: a second listing a moment later
    // finds the entry fresh and does not post the same tools twice while the
    // first post is in flight. The task overwrites it with the real outcome.
    ledger.record(org_id, &due, false);
    let ledger = ledger.clone();
    let client = client.clone();
    let base_url = capability_core_base_url.to_owned();
    let service_token = service_token.to_owned();
    let org_id = org_id.to_owned();
    tokio::spawn(async move {
        match post_observations(&client, &base_url, &service_token, &org_id, &due).await {
            Err(error) => {
                tracing::warn!(%org_id, tools = due.len(), %error, "mcp tool observation not recorded; the registry keeps its last view");
                ledger.record(&org_id, &due, false);
            }
            Ok(report) => {
                if let Some(error) = &report.error {
                    tracing::warn!(%org_id, failed = report.retry.len(), %error, "mcp tool observation not recorded; the registry keeps its last view");
                } else if report.retry.is_empty() {
                    tracing::debug!(%org_id, tools = due.len(), "mcp tool observation posted");
                } else {
                    tracing::warn!(%org_id, contended = report.retry.len(), "mcp tool observation contended in the registry; posting again after the back-off");
                }
                if !report.rejected.is_empty() {
                    let mut reasons = report.rejected.clone();
                    reasons.sort();
                    reasons.dedup();
                    tracing::warn!(%org_id, outcome = "rejected", tools = report.rejected.len(), reasons = %reasons.join(","), "mcp tool observation rejected by the registry; not retried");
                }
                ledger.record(&org_id, &report.settled, true);
                ledger.record(&org_id, &report.retry, false);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_jsonrpc::McpToolAnnotations;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ROOT_SECRET: &str = "mcp-oauth-service-token-test-vector";

    fn tool(name: &str, description: &str, read_only: Option<bool>) -> McpToolDef {
        McpToolDef {
            name: name.to_owned(),
            description: description.to_owned(),
            input_schema_json:
                r#"{"type":"object","properties":{"secret_looking_field":{"type":"string"}}}"#
                    .to_owned(),
            annotations: McpToolAnnotations {
                read_only_hint: read_only,
                ..McpToolAnnotations::default()
            },
        }
    }

    fn observation(name: &str, read_only: Option<bool>) -> ToolObservation {
        ToolObservation::from_tool(
            "mcp_srv",
            &tool(name, "Sends the quarterly report to the board.", read_only),
        )
    }

    /// What capability-core answers: HTTP 200 and one outcome per tool, in the
    /// order they were posted (`observationResult` in `mcp_tools.go`).
    fn answer(outcomes: &[(&str, &str)]) -> serde_json::Value {
        serde_json::json!({
            "results": outcomes
                .iter()
                .map(|(outcome, reason)| {
                    if reason.is_empty() {
                        serde_json::json!({"qualified_name": "mcp__mcp_srv__x", "outcome": outcome})
                    } else {
                        serde_json::json!({"outcome": outcome, "reason": reason})
                    }
                })
                .collect::<Vec<_>>()
        })
    }

    fn succeeded(ledger: &ObservationLedger, qualified_name: &str) -> Option<bool> {
        ledger
            .posted
            .get(&("org-a".to_owned(), qualified_name.to_owned()))
            .map(|posted| posted.succeeded)
    }

    #[test]
    fn only_the_literal_true_enables_observation() {
        assert!(enabled_from(Some("true")));
        for raw in [
            None,
            Some(""),
            Some("TRUE"),
            Some("True"),
            Some("1"),
            Some("on"),
            Some("yes"),
            Some(" true"),
            Some("true "),
        ] {
            assert!(!enabled_from(raw), "{raw:?} must not enable observation");
        }
    }

    #[test]
    fn mcp_tool_observation_under_zdr_never_happens() {
        // Golden case mcp_tool_observation_under_zdr: no write, so no row.
        assert!(should_observe(true, Some(false)));
        assert!(!should_observe(true, Some(true)), "a ZDR caller");
        assert!(!should_observe(true, None), "an unreadable posture is ZDR");
        assert!(!should_observe(false, Some(false)), "the switch is off");
        assert!(!should_observe(false, Some(true)));
    }

    #[test]
    fn derived_service_token_matches_the_cross_language_known_answer_vector() {
        // The identical (secret, org_id, expected hex) triple is asserted by
        // mcp_oauth.rs and by capability-core's mcp_oauth_internal_test.go.
        assert_eq!(
            derive_org_scoped_service_token(ROOT_SECRET, "org-a"),
            "9b70d49c9cecb4ae9d5c871372409886a492d8dde023674d24053f916f6dfd8b"
        );
        assert_ne!(
            derive_org_scoped_service_token(ROOT_SECRET, "org-a"),
            derive_org_scoped_service_token(ROOT_SECRET, "org-b"),
            "a token minted for one org must not authorise another"
        );
    }

    #[test]
    fn an_observation_names_the_tool_the_way_the_exposure_bridge_does() {
        let observed = observation("create_item", Some(false));
        assert_eq!(observed.qualified_name, "mcp__mcp_srv__create_item");
        assert_eq!(observed.server_id, "mcp_srv");
        assert_eq!(observed.tool_name, "create_item");
        assert_eq!(observed.declared_side_effect, "destructive");
        assert_eq!(observed.digest.len(), 64);

        let meta = observed.to_meta();
        assert_eq!(meta.qualified_name, observed.qualified_name);
        assert_eq!(meta.digest, observed.digest);
        assert_eq!(meta.declared_side_effect, "destructive");
    }

    #[test]
    fn the_observation_payload_carries_no_text_from_the_server() {
        let observed = observation("echo_read", Some(true));
        let payload = serde_json::to_string(&ObservationBatch {
            observations: std::slice::from_ref(&observed),
        })
        .expect("serialize");
        // Neither the description nor any part of the schema text, nor the
        // title, is in what leaves the gateway: only digests and hints.
        assert!(!payload.contains("quarterly report"));
        assert!(!payload.contains("secret_looking_field"));
        assert!(!payload.contains("title"));
        assert!(payload.contains(&observed.digest));
        assert!(payload.contains("\"read_only_hint\":true"));
    }

    #[test]
    fn the_ledger_posts_what_is_new_or_changed_and_backs_off_after_a_failure() {
        let ledger = ObservationLedger::default();
        let first = observation("echo_read", Some(true));
        let second = observation("create_item", Some(false));
        let both = [first.clone(), second.clone()];

        assert_eq!(ledger.due("org-a", &both).len(), 2, "nothing posted yet");

        ledger.record("org-a", std::slice::from_ref(&first), true);
        let due = ledger.due("org-a", &both);
        assert_eq!(due, vec![second.clone()], "a fresh success is not repeated");
        assert_eq!(
            ledger.due("org-b", &both).len(),
            2,
            "another org has its own record"
        );

        // A changed definition is due at once even though the tool was posted.
        let changed = ToolObservation::from_tool(
            "mcp_srv",
            &tool("echo_read", "Now it also deletes things.", Some(true)),
        );
        assert_eq!(ledger.due("org-a", &[changed]).len(), 1);

        // A failed post is held back by the back-off, not retried at once.
        ledger.record("org-a", std::slice::from_ref(&second), false);
        assert!(ledger.due("org-a", &both).is_empty());
    }

    #[tokio::test]
    async fn a_post_carries_the_org_derived_credential_and_only_digests_and_hints() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(OBSERVE_PATH))
            .and(query_param("org_id", "org-a"))
            .and(header(
                SERVICE_TOKEN_HEADER,
                derive_org_scoped_service_token(ROOT_SECRET, "org-a").as_str(),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer(&[("recorded", "")])))
            .expect(1)
            .mount(&server)
            .await;

        let observed = observation("echo_read", Some(true));
        let report = post_observations(
            &reqwest::Client::new(),
            &server.uri(),
            ROOT_SECRET,
            "org-a",
            std::slice::from_ref(&observed),
        )
        .await
        .expect("post accepted");
        assert_eq!(report.settled, vec![observed.clone()]);
        assert!(report.retry.is_empty() && report.error.is_none());

        let received = server.received_requests().await.expect("recorded");
        assert_eq!(received.len(), 1);
        let body = String::from_utf8(received[0].body.clone()).expect("utf8 body");
        assert!(body.contains(&observed.digest));
        assert!(!body.contains("quarterly report"));
        assert!(!body.contains("secret_looking_field"));
        // The root secret is never on the wire, in the body or in a header.
        assert!(!body.contains(ROOT_SECRET));
        for (name, value) in received[0].headers.iter() {
            assert!(
                !value.to_str().unwrap_or_default().contains(ROOT_SECRET),
                "header {name} leaked the root secret"
            );
        }
    }

    #[tokio::test]
    async fn a_refused_post_is_an_error_that_names_the_status_only() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_string("the body must not be echoed"))
            .mount(&server)
            .await;
        let report = post_observations(
            &reqwest::Client::new(),
            &server.uri(),
            ROOT_SECRET,
            "org-a",
            &[observation("echo_read", Some(true))],
        )
        .await
        .expect("the address is a URL");
        let error = report.error.expect("a 403 is a failure");
        assert!(error.contains("403"), "{error}");
        assert!(!error.contains("must not be echoed"));
        assert_eq!(
            report.retry.len(),
            1,
            "a refused tool is posted again later"
        );
        assert!(report.settled.is_empty());
    }

    #[tokio::test]
    async fn a_large_batch_is_split_at_the_registry_bound() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(|request: &wiremock::Request| {
                let sent = serde_json::from_slice::<serde_json::Value>(&request.body)
                    .expect("json")["observations"]
                    .as_array()
                    .expect("array")
                    .len();
                ResponseTemplate::new(200).set_body_json(answer(&vec![("recorded", ""); sent]))
            })
            .expect(2)
            .mount(&server)
            .await;
        let many: Vec<ToolObservation> = (0..=MAX_BATCH)
            .map(|index| observation(&format!("tool_{index}"), None))
            .collect();
        let report = post_observations(
            &reqwest::Client::new(),
            &server.uri(),
            ROOT_SECRET,
            "org-a",
            &many,
        )
        .await
        .expect("both batches accepted");
        assert_eq!(report.settled.len(), many.len());
        assert!(report.retry.is_empty());
        let received = server.received_requests().await.expect("recorded");
        let sizes: Vec<usize> = received
            .iter()
            .map(|request| {
                serde_json::from_slice::<serde_json::Value>(&request.body).expect("json")
                    ["observations"]
                    .as_array()
                    .expect("array")
                    .len()
            })
            .collect();
        assert_eq!(sizes, vec![MAX_BATCH, 1]);
    }

    #[tokio::test]
    async fn spawn_observation_is_a_no_op_without_an_address_or_secret() {
        let ledger = ObservationLedger::default();
        let observed = [observation("echo_read", Some(true))];
        spawn_observation(
            &ledger,
            &reqwest::Client::new(),
            "",
            ROOT_SECRET,
            "org-a",
            &observed,
        );
        spawn_observation(
            &ledger,
            &reqwest::Client::new(),
            "http://127.0.0.1:1",
            "",
            "org-a",
            &observed,
        );
        // Nothing was marked as posted, so a configured gateway would still post.
        assert_eq!(ledger.due("org-a", &observed).len(), 1);
    }

    #[tokio::test]
    async fn spawn_observation_posts_once_and_records_the_outcome() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(OBSERVE_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer(&[("recorded", "")])))
            .expect(1)
            .mount(&server)
            .await;
        let ledger = ObservationLedger::default();
        let observed = [observation("echo_read", Some(true))];
        for _ in 0..3 {
            spawn_observation(
                &ledger,
                &reqwest::Client::new(),
                &server.uri(),
                ROOT_SECRET,
                "org-a",
                &observed,
            );
        }
        // Let the spawned task finish.
        for _ in 0..50 {
            if server
                .received_requests()
                .await
                .is_some_and(|r| !r.is_empty())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(server.received_requests().await.expect("recorded").len(), 1);
        assert!(
            ledger.due("org-a", &observed).is_empty(),
            "the posted tool is fresh"
        );
    }

    #[tokio::test]
    async fn contended_and_rejected_answers_are_told_apart_from_recorded_ones() {
        // capability-core answers 200 whatever happened to the individual tools.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer(&[
                ("recorded", ""),
                ("contended", "row_changed_concurrently"),
                ("rejected", "invalid_digest"),
            ])))
            .expect(1)
            .mount(&server)
            .await;
        let tools = [
            observation("echo_read", Some(true)),
            observation("create_item", Some(false)),
            observation("drop_all", None),
        ];
        let report = post_observations(
            &reqwest::Client::new(),
            &server.uri(),
            ROOT_SECRET,
            "org-a",
            &tools,
        )
        .await
        .expect("the address is a URL");
        assert_eq!(report.error, None, "a 200 is not a batch failure");
        assert_eq!(
            report.retry,
            vec![tools[1].clone()],
            "a contended tool is posted again after the back-off"
        );
        assert_eq!(
            report.settled,
            vec![tools[0].clone(), tools[2].clone()],
            "a rejected tool is settled: retrying cannot change the answer"
        );
        assert_eq!(report.rejected, vec!["invalid_digest".to_owned()]);
    }

    #[tokio::test]
    async fn the_ledger_marks_a_contended_tool_failed_and_a_rejected_one_done() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer(&[
                ("recorded", ""),
                ("contended", "row_changed_concurrently"),
                ("rejected", "invalid_digest"),
            ])))
            .expect(1)
            .mount(&server)
            .await;
        let ledger = ObservationLedger::default();
        let tools = [
            observation("echo_read", Some(true)),
            observation("create_item", Some(false)),
            observation("drop_all", None),
        ];
        spawn_observation(
            &ledger,
            &reqwest::Client::new(),
            &server.uri(),
            ROOT_SECRET,
            "org-a",
            &tools,
        );
        // Every tool is marked failed the moment the post is spawned; the task
        // overwrites it with what the registry answered.
        for _ in 0..100 {
            if succeeded(&ledger, "mcp__mcp_srv__echo_read") == Some(true) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(succeeded(&ledger, "mcp__mcp_srv__echo_read"), Some(true));
        assert_eq!(
            succeeded(&ledger, "mcp__mcp_srv__create_item"),
            Some(false),
            "a contended tool must not be recorded as done"
        );
        assert_eq!(
            succeeded(&ledger, "mcp__mcp_srv__drop_all"),
            Some(true),
            "a rejected tool is settled"
        );
    }

    #[tokio::test]
    async fn a_5xx_fails_every_tool_and_stops_posting_the_later_batches() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503).set_body_string("the body must not be echoed"))
            .expect(1)
            .mount(&server)
            .await;
        let many: Vec<ToolObservation> = (0..=MAX_BATCH)
            .map(|index| observation(&format!("tool_{index}"), None))
            .collect();
        let report = post_observations(
            &reqwest::Client::new(),
            &server.uri(),
            ROOT_SECRET,
            "org-a",
            &many,
        )
        .await
        .expect("the address is a URL");
        assert_eq!(
            report.retry, many,
            "every tool is retried, the unsent batch included"
        );
        assert!(report.settled.is_empty());
        let error = report.error.expect("a 503 is a failure");
        assert!(error.contains("503"), "{error}");
        assert!(!error.contains("must not be echoed"));
    }

    #[tokio::test]
    async fn a_2xx_that_does_not_answer_every_tool_is_not_success() {
        let tools = [
            observation("echo_read", Some(true)),
            observation("create_item", Some(false)),
        ];
        for (name, response, want) in [
            (
                "one answer for two tools",
                ResponseTemplate::new(200).set_body_json(answer(&[("recorded", "")])),
                "1 of 2",
            ),
            ("no body", ResponseTemplate::new(200), "could not be read"),
            (
                "not json",
                ResponseTemplate::new(200).set_body_string("<html>ok</html>"),
                "could not be read",
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(response)
                .mount(&server)
                .await;
            let report = post_observations(
                &reqwest::Client::new(),
                &server.uri(),
                ROOT_SECRET,
                "org-a",
                &tools,
            )
            .await
            .expect("the address is a URL");
            assert_eq!(report.retry.len(), 2, "{name}");
            assert!(report.settled.is_empty(), "{name}");
            let error = report.error.expect("an unreadable answer is a failure");
            assert!(error.contains(want), "{name}: {error}");
        }
    }

    #[test]
    fn an_unknown_outcome_is_retried_and_only_a_short_code_is_logged_as_a_reason() {
        let tools = [observation("echo_read", Some(true))];
        let mut report = PostReport::default();
        report.absorb(
            &tools,
            vec![ObservationOutcome {
                outcome: "something_new".to_owned(),
                reason: String::new(),
            }],
        );
        assert_eq!(report.retry.len(), 1);
        assert!(report.settled.is_empty());

        assert_eq!(reason_code("invalid_digest"), "invalid_digest");
        for not_a_code in [
            "",
            "Sends the quarterly report",
            "has-dashes",
            &"a".repeat(65),
        ] {
            assert_eq!(reason_code(not_a_code), "unspecified");
        }
    }
}
