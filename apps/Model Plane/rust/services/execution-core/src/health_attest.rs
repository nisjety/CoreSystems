//! Runtime capability health attestation — the health authority for the
//! capabilities execution-core owns.
//!
//! # Why this exists
//!
//! capability-core denies dispatch before it ever evaluates risk: its policy
//! path derives availability from the registry row, and a row that has never
//! been attested reports `health_not_attested` and blocks. Registry presence and
//! `enabled = true` deliberately do NOT prove that a runtime works.
//!
//! Nothing in the monorepo had ever called
//! `POST /api/v1/capabilities/availability`, so every row sat at
//! `availability_state = 'unavailable'` and the entire capability-gated surface
//! was unreachable by construction — `cap.command.shell` included. This module
//! closes that loop for the two capabilities execution-core is the runtime
//! authority for (see [`crate::capability_policy::trusted_capability_id`]):
//!
//! * `cap.command.sandbox` — the `code_interpreter` runtime (bubblewrap + the
//!   Python interpreter it invokes).
//! * `cap.command.shell` — the `shell` runtime (bubblewrap).
//!
//! # Measure, then attest — never attest blind
//!
//! An attestation is a claim about health, so it is only ever made from a real
//! probe: [`probe`] runs the bubblewrap sandbox probe
//! ([`crate::sandbox::is_supported`], a real `bwrap` invocation) and executes the
//! interpreter. A failed probe attests NOTHING: leaving the capability
//! unavailable is the correct, fail-closed outcome, and writing "available" for
//! a runtime that does not work would be worse than the current denial. This is
//! also why the row is never written by hand — capability-core's
//! `migration_security_test` exists to stop fabricated attestations.
//!
//! # The 5-minute TTL is why this repeats
//!
//! capability-core treats an attestation as current for
//! `AvailabilityAttestationTTL` = 5 minutes, after which availability derives to
//! `health_attestation_stale`. A one-shot attestation at startup would therefore
//! make the tool work for five minutes and then silently stop — while the
//! database column still read `available`, because staleness is computed at read
//! time, not written. So this runs as a heartbeat well inside the TTL.

use std::time::Duration;

use crate::capability_policy::ServiceTokenProvider;

/// The `code_interpreter` runtime capability (low risk: hermetic sandbox).
pub const SANDBOX_CAPABILITY: &str = "cap.command.sandbox";

/// The `shell` runtime capability (high risk: arbitrary host commands).
pub const SHELL_CAPABILITY: &str = "cap.command.shell";

/// Long-term memory search (`recall_memory`).
pub const MEMORY_SEARCH_CAPABILITY: &str = "cap.memory.search";

/// Long-term memory persist (`save_memory`).
pub const MEMORY_INDEX_CAPABILITY: &str = "cap.memory.index";

/// Governed delegation (`subagent.*`).
pub const AGENT_SPAWN_CAPABILITY: &str = "cap.agent.spawn";

/// Scope required to attest a `global` capability row. capability-core's handler
/// looks the row up with `GetGlobal` only for a service principal holding this
/// scope; with the tenant-level health scope it would instead look for a
/// tenant-owned row and return 404.
const GLOBAL_HEALTH_SCOPE: &str = "capability:health:global:write";

/// Read scope, needed because an attestation must echo the row's CURRENT
/// `version` (capability-core uses it as an optimistic-concurrency check), so the
/// row is read immediately before it is attested.
const READ_SCOPE: &str = "capability:read";

/// Audited purpose sent to Auth Core with the token request.
const TOKEN_REASON: &str = "execution runtime capability health attestation";

/// capability-core's `AvailabilityAttestationTTL`. Mirrored here as the ceiling
/// the heartbeat interval is clamped against — never as a value we send.
const ATTESTATION_TTL_SECS: u64 = 300;

/// Heartbeat interval. Well inside the 5-minute TTL so a single lost
/// attestation (a restart, a brief capability-core outage) cannot expire the
/// capability before the next one lands.
const DEFAULT_INTERVAL_SECS: u64 = 60;

/// Interval floor — a runaway knob must not turn health reporting into a
/// request flood.
const MIN_INTERVAL_SECS: u64 = 15;

/// Interval ceiling: past this the attestation would expire between beats, so an
/// operator cannot configure the heartbeat into uselessness.
const MAX_INTERVAL_SECS: u64 = ATTESTATION_TTL_SECS / 2;

/// Compile-time guard: two beats must fit inside the TTL, so a single lost
/// attestation can never expire a capability's health.
const _: () = assert!(MAX_INTERVAL_SECS * 2 <= ATTESTATION_TTL_SECS);

/// Execution mode claimed for both capabilities: they run inside a governed
/// agentic loop, not as a direct read. capability-core additionally REQUIRES
/// `agentic` for anything approval-gated (a high-risk capability with any other
/// mode derives to `approval_path_unavailable`), which is exactly `shell`.
const EXECUTION_MODE_AGENTIC: &str = "agentic";

/// Both runtimes are wall-clock bounded and produce size-capped output, with no
/// token spend and no network — `bounded` is the honest cost class.
const COST_CLASS_BOUNDED: &str = "bounded";

/// What execution-core measured about its own runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeOutcome {
    /// The bubblewrap sandbox can actually build a namespace here.
    pub sandbox: bool,
    /// The interpreter `code_interpreter` invokes is present and runs.
    pub interpreter: bool,
}

/// One capability's attestation, derived from a probe.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Attestation {
    /// Capability row id. Serialized as `id` to match capability-core's request
    /// shape, which rejects unknown fields.
    #[serde(rename = "id")]
    pub capability_id: String,
    /// The row's current version, filled in from the read that precedes the
    /// attestation — capability-core rejects a mismatch.
    pub version: String,
    /// Always `available`. capability-core normalizes a high-risk capability to
    /// `approval_required` itself; claiming that state directly would be
    /// asserting a policy decision that is not ours to make.
    pub state: String,
    pub reason_code: String,
    pub reason: String,
    pub execution_mode: String,
    pub cost_class: String,
}

/// Run the real probes. Cheap: the sandbox result is cached process-wide by
/// [`crate::sandbox::is_supported`], and the interpreter check is one short-lived
/// process per call.
#[must_use]
pub fn probe() -> ProbeOutcome {
    ProbeOutcome {
        sandbox: crate::sandbox::is_supported(),
        interpreter: interpreter_available(),
    }
}

/// Probe the session-core dependency the memory tools actually call.
///
/// A real TCP+HTTP/2 connect to the configured endpoint, per heartbeat — not a
/// one-shot at boot, because session-core restarting under a running
/// execution-core is exactly the situation an attestation TTL exists for. No
/// endpoint configured means the memory tools cannot work in this deployment,
/// so nothing is attested and the rows keep their fail-closed denial.
async fn session_core_reachable() -> bool {
    let Some(url) = std::env::var("SESSION_CORE_URL")
        .or_else(|_| std::env::var("SESSION_CORE_ADDR"))
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return false;
    };
    let Ok(endpoint) = tonic::transport::Endpoint::from_shared(url) else {
        return false;
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        endpoint
            .connect_timeout(std::time::Duration::from_secs(3))
            .connect(),
    )
    .await
    .map(|result| result.is_ok())
    .unwrap_or(false)
}

/// The session-core-dependent attestations, when the dependency probe passed.
///
/// Separate from [`attestable`]: those two capabilities are facts about THIS
/// process (its sandbox, its interpreter); these two are facts about a
/// dependency, so they are re-probed every heartbeat rather than once at boot.
/// Until this existed, `save_memory`/`recall_memory` were advertised, prompted
/// for (`SNIPPET_MEMORY_TOOLS`), and dispatched — and every call died at the
/// capability gate because `cap.memory.{index,search}` had no attestor at all.
#[must_use]
pub fn memory_attestations(session_core_up: bool) -> Vec<Attestation> {
    if !session_core_up {
        return Vec::new();
    }
    vec![
        attestation(
            MEMORY_SEARCH_CAPABILITY,
            "session_memory_probed",
            "Session Core memory endpoint connected.",
        ),
        attestation(
            MEMORY_INDEX_CAPABILITY,
            "session_memory_probed",
            "Session Core memory endpoint connected.",
        ),
        // Delegation shares this dependency and no other: the nested loop runs
        // in-process on the parent's own inference path, and the ONE thing it
        // needs beyond that is `register_delegated_child_run`, a session-core
        // StartRun. Attested here rather than left unattested because
        // `subagent.task` is now advertised — an offered tool whose capability
        // nothing attests is the fail-closed denial this module exists to
        // remove, and the model would burn a round discovering it.
        //
        // The row's risk_level (medium, migration 0008) still decides
        // allow/ask/deny; attesting health is not granting permission.
        attestation(
            AGENT_SPAWN_CAPABILITY,
            "session_run_registration_probed",
            "Session Core run-registration endpoint connected.",
        ),
    ]
}

/// Execute the interpreter `code_interpreter` actually invokes. Presence on
/// `PATH` is not enough — a broken interpreter must not be reported as healthy.
fn interpreter_available() -> bool {
    std::process::Command::new(crate::code_interpreter::PYTHON_PROGRAM)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Which capabilities this probe can TRUTHFULLY attest, and with what.
///
/// * `cap.command.shell` needs only the sandbox: it runs no interpreter.
/// * `cap.command.sandbox` needs both, because `code_interpreter` is useless
///   without the interpreter even when the sandbox is fine.
///
/// A capability whose runtime did not probe healthy is simply absent from the
/// result — it is never attested as unhealthy either, because "we could not
/// measure it" and "we measured it and it is broken" are different claims and
/// only the latter would be an honest attestation from here. Left alone, the row
/// keeps its `health_not_attested` denial, which is the correct fail-closed end
/// state.
#[must_use]
pub fn attestable(outcome: ProbeOutcome) -> Vec<Attestation> {
    let mut planned = Vec::new();
    if outcome.sandbox {
        planned.push(attestation(
            SHELL_CAPABILITY,
            "shell_sandbox_probed",
            "Bubblewrap sandbox probe succeeded.",
        ));
    }
    if outcome.sandbox && outcome.interpreter {
        planned.push(attestation(
            SANDBOX_CAPABILITY,
            "code_runtime_probed",
            "Bubblewrap sandbox and interpreter probes succeeded.",
        ));
    }
    planned
}

fn attestation(capability_id: &str, reason_code: &str, reason: &str) -> Attestation {
    Attestation {
        capability_id: capability_id.to_owned(),
        // Filled in from the row read that precedes the POST.
        version: String::new(),
        state: "available".to_owned(),
        reason_code: reason_code.to_owned(),
        reason: reason.to_owned(),
        execution_mode: EXECUTION_MODE_AGENTIC.to_owned(),
        cost_class: COST_CLASS_BOUNDED.to_owned(),
    }
}

/// Resolve the heartbeat interval, clamped so it can never exceed half the
/// attestation TTL (see the module header).
fn interval_secs(raw: Option<&str>) -> u64 {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(DEFAULT_INTERVAL_SECS)
        .clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS)
}

/// Posts measured health to capability-core's availability API.
pub struct HealthAttestor {
    base_url: String,
    org_id: String,
    http: reqwest::Client,
    tokens: ServiceTokenProvider,
}

impl std::fmt::Debug for HealthAttestor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HealthAttestor")
            .field("base_url", &self.base_url)
            .field("org_id", &self.org_id)
            .finish_non_exhaustive()
    }
}

impl HealthAttestor {
    /// Build from deployment configuration, or `Ok(None)` when
    /// `CAPABILITY_CORE_HTTP_URL` is not configured — health reporting is then
    /// simply off, which must not stop the service from starting.
    ///
    /// # Errors
    /// Returns an error only when the service credential configuration is
    /// present but unusable (see [`ServiceTokenProvider::from_env`]).
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(base_url) = std::env::var("CAPABILITY_CORE_HTTP_URL")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            base_url,
            // A global capability row is not tenant-owned, but capability-core's
            // authorization still requires a verified tenant on the credential.
            // `global` names that honestly instead of borrowing a real customer
            // org; the row lookup itself is tenant-independent for this scope.
            org_id: std::env::var("EXECUTION_HEALTH_ATTEST_ORG_ID")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "global".to_owned()),
            http: reqwest::Client::builder()
                // No redirects: a redirected attestation would send a privileged
                // credential to an unverified host.
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()?,
            tokens: ServiceTokenProvider::from_env()?,
        }))
    }

    #[cfg(test)]
    fn new_for_test(capability_core_url: &str, auth_core_url: &str) -> Self {
        Self {
            base_url: capability_core_url.trim_end_matches('/').to_owned(),
            org_id: "global".to_owned(),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(5))
                .build()
                .expect("test HTTP client"),
            tokens: ServiceTokenProvider::new_for_test(
                auth_core_url,
                "execution-core",
                "service-secret",
            ),
        }
    }

    async fn bearer(&self) -> anyhow::Result<String> {
        // One credential for both calls: the row read needs `capability:read`
        // and the attestation needs the global health scope.
        self.tokens
            .token_with_scopes(
                &self.org_id,
                &[READ_SCOPE, GLOBAL_HEALTH_SCOPE],
                TOKEN_REASON,
            )
            .await
    }

    /// Attest one capability: read its current version, then post the health it
    /// belongs to.
    ///
    /// # Errors
    /// Returns an error when the credential cannot be minted, the row cannot be
    /// read, or capability-core rejects the attestation.
    pub async fn attest(&self, planned: &Attestation) -> anyhow::Result<()> {
        let bearer = self.bearer().await?;
        // Read the version per attestation rather than caching it: capability-core
        // guards the write with `version = $expected`, so a catalog version bump
        // would otherwise turn every later heartbeat into a silent 404.
        let version = self.row_version(&bearer, &planned.capability_id).await?;
        let body = Attestation {
            version,
            ..planned.clone()
        };
        let response = self
            .http
            .post(format!(
                "{}/api/v1/capabilities/availability",
                self.base_url
            ))
            .bearer_auth(&bearer)
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "capability-core refused the {} attestation (status {})",
                planned.capability_id,
                response.status()
            );
        }
        Ok(())
    }

    async fn row_version(&self, bearer: &str, capability_id: &str) -> anyhow::Result<String> {
        let response = self
            .http
            .get(format!(
                "{}/api/v1/capabilities/{capability_id}",
                self.base_url
            ))
            .bearer_auth(bearer)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!(
                "capability-core did not return the {capability_id} row (status {})",
                response.status()
            );
        }
        let row = response.json::<serde_json::Value>().await?;
        row.get("version")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .filter(|version| !version.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("{capability_id} row has no version to attest against"))
    }

    /// Attest every planned capability, reporting each outcome. Never returns an
    /// error: one capability's failure must not suppress another's attestation,
    /// and none of them may affect service startup.
    pub async fn attest_all(&self, planned: &[Attestation]) -> usize {
        let mut attested = 0;
        for capability in planned {
            match self.attest(capability).await {
                Ok(()) => {
                    attested += 1;
                    tracing::info!(
                        capability = %capability.capability_id,
                        reason_code = %capability.reason_code,
                        "runtime capability health attested"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        capability = %capability.capability_id,
                        "runtime capability health attestation failed; capability stays unavailable"
                    );
                }
            }
        }
        attested
    }
}

/// Start the health-attestation heartbeat.
///
/// Deliberately infallible and detached: a service that cannot report its health
/// must still serve. Every failure path here logs at WARN and returns, leaving
/// the capabilities unavailable — which is the correct fail-closed state, not an
/// outage of execution-core itself.
pub fn spawn_heartbeat() {
    let outcome = probe();
    let planned = attestable(outcome);
    if planned.is_empty() {
        tracing::warn!(
            sandbox_probe = outcome.sandbox,
            interpreter_probe = outcome.interpreter,
            "no runtime capability can be attested: sandbox and/or interpreter probe failed, so \
             capability-core will keep denying command execution (fail-closed, nothing attested)"
        );
        return;
    }
    let attestor = match HealthAttestor::from_env() {
        Ok(Some(attestor)) => attestor,
        Ok(None) => {
            tracing::warn!(
                "CAPABILITY_CORE_HTTP_URL is not configured: runtime capability health cannot be \
                 attested, so capability-gated tools stay unavailable"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "runtime health attestation is unavailable");
            return;
        }
    };
    let interval = Duration::from_secs(interval_secs(
        std::env::var("EXECUTION_HEALTH_ATTEST_INTERVAL_SECS")
            .ok()
            .as_deref(),
    ));
    tokio::spawn(async move {
        // A heartbeat, not a one-shot: capability-core expires an attestation
        // after 5 minutes (module header), so stopping after the first success
        // would take the capability down again five minutes later.
        loop {
            attestor.attest_all(&planned).await;
            // Dependency-backed capabilities are re-probed every round: a
            // session-core outage must stop renewing them (TTL then takes the
            // rows down, fail-closed), and its recovery must bring them back
            // without an execution-core restart.
            let memory = memory_attestations(session_core_reachable().await);
            if !memory.is_empty() {
                attestor.attest_all(&memory).await;
            }
            tokio::time::sleep(interval).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn token_mock() -> Mock {
        Mock::given(method("POST"))
            .and(path("/api/capability-core/internal-token"))
            // The scope set is part of the contract: without the GLOBAL health
            // scope capability-core looks for a tenant-owned row and 404s.
            .and(body_json(serde_json::json!({
                "orgId": "global",
                "scopes": ["capability:read", "capability:health:global:write"],
                "reason": TOKEN_REASON,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "health-token",
                "expiresInSeconds": 300,
                "audience": "capability-core"
            })))
    }

    /// The row read must carry the same minted credential as the attestation —
    /// asserted here so an unauthenticated read can never satisfy a test.
    fn row_mock(capability_id: &str, version: &str) -> Mock {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/capabilities/{capability_id}")))
            .and(header("authorization", "Bearer health-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": capability_id,
                "version": version,
                "risk_level": "low",
            })))
    }

    /// The dependency-backed pair follows the same honesty rule as the probe
    /// pair: an unreachable session-core attests NOTHING (the rows keep their
    /// fail-closed denial), and a reachable one attests exactly the two memory
    /// capabilities the tools are bound to.
    #[test]
    fn memory_attestations_track_the_dependency_probe() {
        assert!(memory_attestations(false).is_empty());
        let up = memory_attestations(true);
        let ids: Vec<&str> = up.iter().map(|a| a.capability_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                MEMORY_SEARCH_CAPABILITY,
                MEMORY_INDEX_CAPABILITY,
                AGENT_SPAWN_CAPABILITY
            ]
        );
        assert!(up.iter().all(|a| a.state == "available"));
    }

    #[test]
    fn a_failed_probe_attests_nothing() {
        // No sandbox → nothing is claimed, for either capability. Leaving the row
        // unattested is the fail-closed outcome; inventing health would not be.
        assert!(attestable(ProbeOutcome {
            sandbox: false,
            interpreter: true
        })
        .is_empty());
        assert!(attestable(ProbeOutcome {
            sandbox: false,
            interpreter: false
        })
        .is_empty());
    }

    #[test]
    fn a_missing_interpreter_attests_only_shell() {
        // shell runs no interpreter, so it is still truthfully healthy; the code
        // runtime is not, and must not be attested.
        let planned = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: false,
        });
        let ids: Vec<&str> = planned.iter().map(|a| a.capability_id.as_str()).collect();
        assert_eq!(ids, vec![SHELL_CAPABILITY]);
    }

    #[test]
    fn a_full_probe_attests_both_capabilities_agentically() {
        let planned = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: true,
        });
        let ids: Vec<&str> = planned.iter().map(|a| a.capability_id.as_str()).collect();
        assert_eq!(ids, vec![SHELL_CAPABILITY, SANDBOX_CAPABILITY]);
        for attestation in &planned {
            assert_eq!(attestation.state, "available");
            // capability-core rejects an approval-gated capability that has no
            // agentic path, and `shell` is exactly that.
            assert_eq!(attestation.execution_mode, "agentic");
            assert_eq!(attestation.cost_class, "bounded");
            // reason_code must satisfy capability-core's ^[a-z][a-z0-9_]{0,127}$.
            assert!(
                attestation
                    .reason_code
                    .starts_with(|c: char| c.is_ascii_lowercase())
                    && attestation
                        .reason_code
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    && attestation.reason_code.len() <= 128,
                "reason_code '{}' violates the server pattern",
                attestation.reason_code
            );
        }
    }

    #[test]
    fn a_sandbox_probe_never_attests_the_owner_action_ticket_adapter() {
        let planned = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: true,
        });

        assert!(
            !planned
                .iter()
                .any(|attestation| attestation.capability_id == crate::ticket_tools::CAPABILITY_ID),
            "a sandbox probe cannot prove the independent Control and Conversation Core owner-action path"
        );
    }

    /// capability-core decodes the body with `DisallowUnknownFields`, so the key
    /// set is an exact contract — an extra or renamed field is a 400.
    #[test]
    fn the_request_body_matches_the_servers_exact_field_set() {
        let mut attestation = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: true,
        })
        .pop()
        .expect("sandbox attestation");
        attestation.version = "1.0.0".to_owned();
        let body: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&attestation).expect("serializes"))
                .expect("json");
        let mut keys: Vec<&str> = body
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "cost_class",
                "execution_mode",
                "id",
                "reason",
                "reason_code",
                "state",
                "version"
            ]
        );
        assert_eq!(body["id"], SANDBOX_CAPABILITY);
        assert_eq!(body["version"], "1.0.0");
    }

    #[tokio::test]
    async fn a_successful_attestation_reads_the_version_then_posts_it() {
        let auth = MockServer::start().await;
        token_mock().expect(1).mount(&auth).await;
        let capability_core = MockServer::start().await;
        row_mock(SANDBOX_CAPABILITY, "2.1.0")
            .expect(1)
            .mount(&capability_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/capabilities/availability"))
            .and(header("authorization", "Bearer health-token"))
            // The version MUST be the one just read: capability-core guards the
            // write with `version = $expected`.
            .and(body_json(serde_json::json!({
                "id": SANDBOX_CAPABILITY,
                "version": "2.1.0",
                "state": "available",
                "reason_code": "code_runtime_probed",
                "reason": "Bubblewrap sandbox and interpreter probes succeeded.",
                "execution_mode": "agentic",
                "cost_class": "bounded",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"id": SANDBOX_CAPABILITY, "state": "available"}
            })))
            .expect(1)
            .mount(&capability_core)
            .await;

        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());
        let planned = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: true,
        });
        let sandbox = planned
            .iter()
            .find(|a| a.capability_id == SANDBOX_CAPABILITY)
            .expect("sandbox planned");
        attestor
            .attest(sandbox)
            .await
            .expect("attestation succeeds");
    }

    #[tokio::test]
    async fn a_refused_credential_is_reported_without_attesting() {
        // Today's live state: the deployment principal has no
        // `capability:health:global:write`, so Auth Core refuses the token. That
        // must surface as a warning, never as a panic or a startup failure.
        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&auth)
            .await;
        let capability_core = MockServer::start().await;
        // Nothing must reach capability-core without a credential.
        Mock::given(method("POST"))
            .and(path("/api/v1/capabilities/availability"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&capability_core)
            .await;

        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());
        let planned = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: true,
        });
        assert_eq!(
            attestor.attest_all(&planned).await,
            0,
            "no capability may be attested without a credential"
        );
    }

    #[tokio::test]
    async fn a_transport_failure_is_contained_and_other_capabilities_still_attest() {
        let auth = MockServer::start().await;
        token_mock().mount(&auth).await;
        let capability_core = MockServer::start().await;
        // shell's row read fails; sandbox's succeeds. One broken capability must
        // not suppress the other.
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/capabilities/{SHELL_CAPABILITY}")))
            .respond_with(ResponseTemplate::new(500))
            .mount(&capability_core)
            .await;
        row_mock(SANDBOX_CAPABILITY, "1.0.0")
            .mount(&capability_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/capabilities/availability"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": {}})))
            .expect(1)
            .mount(&capability_core)
            .await;

        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());
        let planned = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: true,
        });
        assert_eq!(attestor.attest_all(&planned).await, 1);
    }

    #[tokio::test]
    async fn a_rejected_attestation_never_reports_success() {
        let auth = MockServer::start().await;
        token_mock().mount(&auth).await;
        let capability_core = MockServer::start().await;
        row_mock(SANDBOX_CAPABILITY, "1.0.0")
            .mount(&capability_core)
            .await;
        row_mock(SHELL_CAPABILITY, "1.0.0")
            .mount(&capability_core)
            .await;
        // 404 is what capability-core returns when the row/version does not match
        // or the attestation is not newer than the stored one.
        Mock::given(method("POST"))
            .and(path("/api/v1/capabilities/availability"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&capability_core)
            .await;

        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());
        let planned = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: true,
        });
        assert_eq!(attestor.attest_all(&planned).await, 0);
    }

    /// The heartbeat must never outrun the server's 5-minute expiry, whatever an
    /// operator puts in the environment.
    #[test]
    fn the_heartbeat_interval_stays_inside_the_attestation_ttl() {
        assert_eq!(interval_secs(None), DEFAULT_INTERVAL_SECS);
        assert_eq!(interval_secs(Some("90")), 90);
        assert_eq!(interval_secs(Some(" 45 ")), 45);
        // Clamped: too fast is a request flood, too slow expires the capability.
        assert_eq!(interval_secs(Some("1")), MIN_INTERVAL_SECS);
        assert_eq!(interval_secs(Some("6000")), MAX_INTERVAL_SECS);
        // Garbage and zero fall back to the default rather than to no delay.
        for bad in [Some(""), Some("0"), Some("later")] {
            assert_eq!(interval_secs(bad), DEFAULT_INTERVAL_SECS);
        }
    }

    /// The probe must be a real measurement of the runtimes this service ships.
    #[test]
    fn the_probe_measures_the_real_runtimes() {
        let outcome = probe();
        assert_eq!(outcome.sandbox, crate::sandbox::is_supported());
        assert_eq!(outcome.interpreter, interpreter_available());
    }
}
