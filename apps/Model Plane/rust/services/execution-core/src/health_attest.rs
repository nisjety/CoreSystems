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
//! * `cap.process.background` — the S4.2 `process_*` family, which needs the
//!   same two probes AND the process host to be enabled on this instance.
//!
//! Three more are dependency-backed, probed every round rather than once at boot
//! (see [`probe_dependencies`]): the session-core pair behind the memory and
//! delegation tools, `cap.retrieval.query` (`knowledge_search`, probed against
//! the Data Plane retrieval engine) and `cap.tool.http` (`web_search`/`web_fetch`,
//! probed against quarry-edge AND the Quarry credentials dispatch needs).
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

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::capability_policy::ServiceTokenProvider;
use crate::web_tools::WebToolsClient;

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

/// The S4.2 background-process family (`process_*`).
pub const PROCESS_CAPABILITY: &str = "cap.process.background";

/// Knowledge retrieval over Data Plane v2 (`knowledge_search`).
pub const RETRIEVAL_QUERY_CAPABILITY: &str = "cap.retrieval.query";

/// Public-web access through Quarry (`web_search`, `web_fetch` and their
/// `web.search`/`web.read` aliases).
pub const WEB_HTTP_CAPABILITY: &str = "cap.tool.http";

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

/// Public-web tools reach a third-party search chain whose cost per call is not
/// ours to bound; this is also the cost class migration 0008 seeds the row with.
const COST_CLASS_VARIABLE: &str = "variable";

/// Env var naming the Data Plane retrieval engine's own HTTP readiness endpoint
/// (`http://retrieval-engine:8004/readyz`). Deliberately a separate setting from
/// the gRPC endpoint dispatch dials: the readiness route lives on the engine's
/// HTTP port, not the gRPC one. Absent or blank means "cannot measure", so
/// nothing is attested.
const RETRIEVAL_READY_URL_ENV: &str = "DATAPLANE_RETRIEVAL_READY_URL";

/// Ceiling on each retrieval probe leg (the session-core connect carries its own
/// 3 s). A hung dependency must cost the heartbeat seconds, not stall it.
const DEPENDENCY_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Ceiling on the whole web probe: the edge `/ready` call plus up to two token
/// mints (the token client itself allows 10 s, which would be far too long here).
const WEB_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// What execution-core measured about its own runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeOutcome {
    /// The bubblewrap sandbox can actually build a namespace here.
    pub sandbox: bool,
    /// The interpreter `code_interpreter` invokes runs AND imports every library
    /// the tool's description advertises (see [`InterpreterProbe::healthy`]).
    pub interpreter: bool,
}

/// The Python modules `code_interpreter` tells the model it can import, as
/// `import` names. Each names the part of its library the advertised feature
/// needs, not just the package, so a distro package whose compiled core is
/// broken (`PIL.Image`, `lxml.etree`) or whose plotting layer cannot load
/// (`matplotlib.pyplot` under the headless backend) is caught here and not in the
/// middle of a user's request.
///
/// The list is the sandbox image's Python packages (`Dockerfile`, runtime stage)
/// and nothing else: a test compares the two, as it does for the description the
/// model reads (`model-gateway`'s `builtin_tool_defs`), so none of the three can
/// drift. Library by library, what the model is told each one is for:
/// numpy and pandas (data), matplotlib (PNG charts), openpyxl and xlsxwriter
/// (`.xlsx` with native charts), python-docx (`.docx`), python-pptx (`.pptx`),
/// reportlab (`.pdf`), pypdf (read or merge PDFs), Pillow (images), lxml (the XML
/// layer under python-docx and python-pptx).
pub const SANDBOX_PYTHON_MODULES: &[&str] = &[
    "numpy",
    "pandas",
    "matplotlib.pyplot",
    "openpyxl.chart",
    "xlsxwriter",
    "docx",
    "reportlab.pdfgen.canvas",
    "pypdf",
    "PIL.Image",
    "pptx",
    "lxml.etree",
];

/// Ceiling on the interpreter import probe. Importing pandas and matplotlib takes
/// about a second and a half on a warm container (measured, 2026-10-01); this is
/// there so a wedged import costs the boot probe seconds, not forever.
const INTERPRETER_PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// What the interpreter probe measured.
///
/// Two different facts, kept apart on purpose: `ran` says the probe completed at
/// all, `missing` says what it found when it did. A probe that could not run (no
/// interpreter, a crash mid-import, the deadline) measured nothing, so it names no
/// module and is never healthy; it is not the same claim as "ran, and these
/// modules are missing".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterpreterProbe {
    /// The interpreter started and ran the probe script to its end.
    pub ran: bool,
    /// The modules that did not import, in probe order. Empty when none failed
    /// and also when the probe did not run.
    pub missing: Vec<String>,
}

impl InterpreterProbe {
    /// The code runtime is only healthy when the probe completed and nothing was
    /// missing: the tool advertises every one of these libraries, so a runtime
    /// without one of them would be attested as able to do what it cannot.
    #[must_use]
    pub fn healthy(&self) -> bool {
        self.ran && self.missing.is_empty()
    }
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

/// Run the real probes. The sandbox result is cached process-wide by
/// [`crate::sandbox::is_supported`]; the interpreter check is one short-lived
/// process per call that imports the libraries the tool advertises (about a
/// second and a half on a warm container), so it is made once at boot, not per
/// heartbeat. A library that fails to import is logged by name, at `warn`, here:
/// the attestation then simply does not happen, and without the name the only
/// symptom would be a `code_interpreter` that is denied for no visible reason.
#[must_use]
pub fn probe() -> ProbeOutcome {
    let interpreter = interpreter_probe();
    if !interpreter.healthy() {
        tracing::warn!(
            interpreter_ran = interpreter.ran,
            missing_modules = ?interpreter.missing,
            "the code_interpreter runtime is not healthy: the interpreter did not run, or cannot \
             import a library the tool advertises; cap.command.sandbox will not be attested"
        );
    }
    ProbeOutcome {
        sandbox: crate::sandbox::is_supported(),
        interpreter: interpreter.healthy(),
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

/// The `cap.retrieval.query` attestation, when the retrieval probe passed.
///
/// `knowledge_search` executes inside this process and its one external
/// dependency is the Data Plane retrieval engine, so this is a dependency-backed
/// attestation like [`memory_attestations`]. Until it existed the row sat at
/// `health_not_attested` and every `knowledge_search` call was refused at the
/// capability gate in about 50 ms.
///
/// What the probe proves, and the reason text says so: the engine's own
/// readiness (Postgres, Qdrant) and that its gRPC route and JWT boundary answer
/// on the endpoint dispatch uses. What it does NOT prove: the embedding
/// provider. An embed failure still fails that one call, with a typed degraded
/// result — so the `reason_code` stays this narrow, and the row's `low` risk
/// level (not this attestation) is what decides allow/ask/deny.
#[must_use]
pub fn retrieval_attestations(retrieval_up: bool) -> Vec<Attestation> {
    if !retrieval_up {
        return Vec::new();
    }
    vec![attestation_with_cost(
        RETRIEVAL_QUERY_CAPABILITY,
        "retrieval_engine_ready_probed",
        "Data Plane retrieval engine reported Postgres and Qdrant ready, and its retrieval \
         endpoint answered an unauthenticated call with Unauthenticated.",
        COST_CLASS_BOUNDED,
    )]
}

/// The `cap.tool.http` attestation, when the web probe passed.
///
/// One capability id covers `web_search`, `web_fetch` and the `web.search` /
/// `web.read` aliases, so it is attested only when BOTH tools' credentials mint
/// (see [`crate::web_tools::WebToolsClient::probe_ready`]) — granting one scope
/// must not make the tool that still cannot authenticate look healthy. While
/// Auth Core refuses either scope for this principal, this stays empty and the
/// row stays unavailable; it flips by itself once the grant lands.
///
/// The probe does not exercise a search provider (the edge's search route is
/// metered, so it is never called), so provider outages still surface per call.
#[must_use]
pub fn web_attestations(web_up: bool) -> Vec<Attestation> {
    if !web_up {
        return Vec::new();
    }
    vec![attestation_with_cost(
        WEB_HTTP_CAPABILITY,
        "quarry_edge_ready_credential_probed",
        "Quarry edge reported ready, and the search and extract credentials mint.",
        COST_CLASS_VARIABLE,
    )]
}

/// What the per-round dependency probes measured.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DependencyProbes {
    /// session-core accepts a connection (memory tools, delegation).
    pub session_core: bool,
    /// The Data Plane retrieval engine is ready AND its auth boundary answers.
    pub retrieval: bool,
    /// quarry-edge is ready AND the web tools' credentials mint.
    pub web: bool,
}

/// Every dependency-backed attestation a round's probes justify. A failed probe
/// contributes nothing, so an unhealthy dependency simply stops being renewed
/// (the row then lapses on its own inside capability-core's 5-minute TTL).
#[must_use]
pub fn dependency_attestations(probes: DependencyProbes) -> Vec<Attestation> {
    let mut planned = memory_attestations(probes.session_core);
    planned.extend(retrieval_attestations(probes.retrieval));
    planned.extend(web_attestations(probes.web));
    planned
}

/// The retrieval engine's own readiness, over its unauthenticated HTTP route.
///
/// Healthy only for an HTTP `200` whose body reports `status` `ready` with
/// `checks.postgres` and `checks.qdrant` both `true` (see
/// [`readyz_body_is_ready`]). No credential is sent, redirects are not followed
/// (a redirect target's answer is not the engine's own), and `timeout` bounds
/// the whole request including the body.
async fn retrieval_readyz_reports_ready(url: &str, timeout: Duration) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeout)
        .timeout(timeout)
        .build()
    else {
        return false;
    };
    let Ok(response) = client.get(url).send().await else {
        return false;
    };
    if response.status() != reqwest::StatusCode::OK {
        return false;
    }
    response
        .json::<serde_json::Value>()
        .await
        .is_ok_and(|body| readyz_body_is_ready(&body))
}

/// What a ready retrieval engine says, per its own `/readyz`: `status` is
/// `ready` and its two data dependencies, Postgres and Qdrant, report `true`.
/// The cache and the erasure consumer are folded into `status` by the engine
/// itself, so they are deliberately not re-derived here.
fn readyz_body_is_ready(body: &serde_json::Value) -> bool {
    let check = |name: &str| {
        body.get("checks")
            .and_then(|checks| checks.get(name))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    };
    body.get("status").and_then(serde_json::Value::as_str) == Some("ready")
        && check("postgres")
        && check("qdrant")
}

/// The web probe: ask dispatch's own client whether it could serve right now,
/// under `timeout`. Healthy only when
/// [`WebToolsClient::probe_ready`] is `Ok` — edge ready AND both credentials
/// mint. A timeout is a failure, so a hung Auth Core or edge cannot stall the
/// heartbeat. The failure reason is logged (statuses only: the probe never
/// returns or logs a token) because "why is `web_search` unavailable" is
/// otherwise unanswerable; it is `debug` because until the Control Plane grants
/// the scopes this fails every round by design.
async fn web_probe(client: &WebToolsClient, org_id: &str, timeout: Duration) -> bool {
    static PASSING: AtomicBool = AtomicBool::new(true);
    match tokio::time::timeout(timeout, client.probe_ready(org_id)).await {
        Ok(Ok(())) => {
            note_probe_outcome(&PASSING, true, WEB_HTTP_CAPABILITY, "");
            true
        }
        Ok(Err(reason)) => {
            tracing::debug!(
                %reason,
                "web tools probe did not pass; cap.tool.http is not attested this round"
            );
            note_probe_outcome(&PASSING, false, WEB_HTTP_CAPABILITY, &reason.to_string());
            false
        }
        Err(_elapsed) => {
            tracing::debug!("web tools probe timed out; cap.tool.http is not attested this round");
            note_probe_outcome(&PASSING, false, WEB_HTTP_CAPABILITY, "probe timed out");
            false
        }
    }
}

/// Logs at `warn`, once, each time a probe goes from passing (or never having
/// run) to failing. The per-round failure line is `debug` because a probe can
/// fail every round by design (the Control Plane has not granted the web
/// scopes), but an operator still has to be able to see WHY a capability is
/// unattested without turning on debug logging, and one line per 60 s round
/// would be noise. `detail` is a status-only reason, never a credential.
fn note_probe_outcome(
    previously_passing: &AtomicBool,
    passing: bool,
    capability: &str,
    detail: &str,
) {
    if passing {
        previously_passing.store(true, Ordering::Relaxed);
    } else if previously_passing.swap(false, Ordering::Relaxed) {
        tracing::warn!(
            capability,
            detail,
            "capability probe is failing; the capability stays unattested until it passes"
        );
    }
}

/// The configured readiness URL, if there is a usable one. Absent or blank means
/// "cannot measure", which attests nothing.
fn retrieval_ready_url_from_value(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn retrieval_ready_url_from_env() -> Option<String> {
    retrieval_ready_url_from_value(std::env::var(RETRIEVAL_READY_URL_ENV).ok())
}

/// The `cap.retrieval.query` probe: the engine's own readiness AND its auth
/// boundary on the endpoint dispatch dials, concurrently (so the cost is the
/// slower leg, not both). BOTH must hold:
///
/// * `/readyz` proves Postgres and Qdrant are up — the engine's own dependency
///   checks, which no credential-free gRPC call can see.
/// * The unauthenticated `Retrieve` proves the exact gRPC path `knowledge_search`
///   uses, and the JWT interceptor in front of it, are alive — which the HTTP
///   readiness route (a different port and router) cannot see.
async fn retrieval_engine_probe(grpc_endpoint: &str, ready_url: &str, timeout: Duration) -> bool {
    let (ready, boundary) = tokio::join!(
        retrieval_readyz_reports_ready(ready_url, timeout),
        crate::knowledge_tools::retrieval_auth_boundary_alive(grpc_endpoint, timeout),
    );
    static PASSING: AtomicBool = AtomicBool::new(true);
    if !(ready && boundary) {
        tracing::debug!(
            readyz_ready = ready,
            auth_boundary_alive = boundary,
            "retrieval engine probe did not pass; cap.retrieval.query is not attested this round"
        );
    }
    note_probe_outcome(
        &PASSING,
        ready && boundary,
        RETRIEVAL_QUERY_CAPABILITY,
        &format!("readyz_ready={ready} auth_boundary_alive={boundary}"),
    );
    ready && boundary
}

/// The retrieval probe against this process's own configuration: the gRPC
/// endpoint comes from the SAME resolver dispatch uses, so the probe cannot
/// vouch for an address `knowledge_search` does not dial.
async fn retrieval_engine_ready() -> bool {
    let (Some(grpc_endpoint), Some(ready_url)) = (
        crate::knowledge_tools::retrieval_endpoint_from_env(),
        retrieval_ready_url_from_env(),
    ) else {
        return false;
    };
    retrieval_engine_probe(&grpc_endpoint, &ready_url, DEPENDENCY_PROBE_TIMEOUT).await
}

/// The web probe against this process's own configuration: the very client
/// dispatch builds (same edge URL, same token source), asked whether it could
/// serve right now.
async fn web_tools_ready(org_id: &str) -> bool {
    match WebToolsClient::from_env() {
        Ok(Some(client)) => web_probe(&client, org_id, WEB_PROBE_TIMEOUT).await,
        // Not configured, or its credential configuration is unusable: the web
        // tools cannot work here, so nothing is attested.
        Ok(None) | Err(_) => false,
    }
}

/// Run every dependency probe for this round, concurrently and each under its
/// own timeout, so a hung dependency delays the round by one timeout at most
/// and one failure never blocks another's attestation.
pub async fn probe_dependencies(org_id: &str) -> DependencyProbes {
    let (session_core, retrieval, web) = tokio::join!(
        session_core_reachable(),
        retrieval_engine_ready(),
        web_tools_ready(org_id)
    );
    DependencyProbes {
        session_core,
        retrieval,
        web,
    }
}

/// The program the import probe runs: it imports every module in
/// [`SANDBOX_PYTHON_MODULES`] (passed as arguments, never spliced into this text)
/// and reports which ones failed on one marker line. `BaseException` rather than
/// `Exception` because a library that calls `sys.exit` at import is just as
/// unusable. A segfault in a compiled extension kills the process before the
/// marker is written, which the caller reads as "did not run".
const IMPORT_PROBE_SCRIPT: &str = concat!(
    "import importlib, sys\n",
    "missing = []\n",
    "for name in sys.argv[1:]:\n",
    "    try:\n",
    "        importlib.import_module(name)\n",
    "    except BaseException:\n",
    "        missing.append(name)\n",
    "sys.stdout.write('VEREVON_MISSING=' + ','.join(missing) + '\\n')\n",
);

/// Marker the import probe prints before the comma-separated missing modules.
const IMPORT_PROBE_MARKER: &str = "VEREVON_MISSING=";

/// Run the interpreter `code_interpreter` actually invokes and import every library
/// it advertises. Presence on `PATH` is not enough — a broken interpreter must not
/// be reported as healthy, and neither must one that cannot build the files the
/// tool promises.
#[must_use]
pub fn interpreter_probe() -> InterpreterProbe {
    probe_python_modules(
        crate::code_interpreter::PYTHON_PROGRAM,
        SANDBOX_PYTHON_MODULES,
        INTERPRETER_PROBE_TIMEOUT,
    )
}

/// [`interpreter_probe`] with the interpreter, module list and deadline injected —
/// the testable seam (this crate forbids environment mutation in tests, so they
/// pass these instead).
fn probe_python_modules(program: &str, modules: &[&str], timeout: Duration) -> InterpreterProbe {
    probe_with_script(program, IMPORT_PROBE_SCRIPT, modules, timeout)
}

/// Run `script` under `program` with `modules` as its arguments, and read the
/// missing-modules marker from its stdout. Anything short of a clean exit that
/// printed the marker is "did not run": see [`InterpreterProbe`].
///
/// The child gets the sandbox's own environment variables laid over this
/// process's (`HOME`, `TMPDIR`, `MPLCONFIGDIR` and `MPLBACKEND`, from
/// [`crate::code_interpreter::child_env_for`]) and a throwaway directory as its
/// working directory, so matplotlib has somewhere writable for its font cache and
/// the probe leaves nothing behind. It is not wrapped in bubblewrap: the sandbox
/// binds the same root filesystem read-only, so the same interpreter imports the
/// same site-packages, and the sandbox itself is probed separately.
fn probe_with_script(
    program: &str,
    script: &str,
    modules: &[&str],
    timeout: Duration,
) -> InterpreterProbe {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "verevon-probe-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let env = crate::code_interpreter::child_env_for(&scratch);
    for (key, directory) in &env {
        if key == "TMPDIR" || key == "MPLCONFIGDIR" {
            // Creates `scratch` itself on the way. A failure surfaces as the spawn
            // failing in a directory that is not there: "did not run".
            let _ = std::fs::create_dir_all(directory);
        }
    }
    let probe = run_probe_child(program, script, modules, &scratch, &env, timeout);
    let _ = std::fs::remove_dir_all(&scratch);
    probe
}

fn run_probe_child(
    program: &str,
    script: &str,
    modules: &[&str],
    working_dir: &std::path::Path,
    env: &[(String, String)],
    timeout: Duration,
) -> InterpreterProbe {
    let spawned = std::process::Command::new(program)
        .arg("-c")
        .arg(script)
        .args(modules)
        .envs(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .current_dir(working_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else {
        return InterpreterProbe::default();
    };
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            // Deadline passed, or the wait itself failed: stop the child and
            // report that nothing was measured.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return InterpreterProbe::default();
            }
        }
    };
    if !status.success() {
        return InterpreterProbe::default();
    }
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    match missing_modules_from_output(&stdout) {
        Some(missing) => InterpreterProbe { ran: true, missing },
        None => InterpreterProbe::default(),
    }
}

/// The modules named on the import probe's marker line, or `None` when it never
/// wrote one.
fn missing_modules_from_output(stdout: &str) -> Option<Vec<String>> {
    let line = stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix(IMPORT_PROBE_MARKER))?;
    Some(
        line.split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

/// Which capabilities this probe can TRUTHFULLY attest, and with what.
///
/// * `cap.command.shell` needs only the sandbox: it runs no interpreter.
/// * `cap.command.sandbox` needs both, because `code_interpreter` is useless
///   without the interpreter even when the sandbox is fine.
/// * `cap.process.background` needs both AND the process host to be enabled
///   here. That third condition is not a probe of a dependency — it is a
///   property of THIS deployment, and it is the one that makes the attestation
///   honest. Every execution-core instance can run bubblewrap; only an
///   instance an operator opted in can keep a process alive past the call, and
///   an instance that cannot must not tell capability-core that it can. The
///   same flag decides what `sandbox::capability_profile` advertises, so the
///   profile a Space's decision is signed against and the capability row's
///   health say the same thing about the same instance.
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
            "Bubblewrap sandbox probe succeeded and the interpreter imported every library \
             code_interpreter advertises.",
        ));
        if crate::sandbox::process_host_enabled() {
            planned.push(attestation(
                PROCESS_CAPABILITY,
                "process_host_enabled_and_runtime_probed",
                "Bubblewrap sandbox and interpreter probes succeeded and the background \
                 process host is enabled on this instance.",
            ));
        }
    }
    planned
}

fn attestation(capability_id: &str, reason_code: &str, reason: &str) -> Attestation {
    attestation_with_cost(capability_id, reason_code, reason, COST_CLASS_BOUNDED)
}

fn attestation_with_cost(
    capability_id: &str,
    reason_code: &str,
    reason: &str,
    cost_class: &str,
) -> Attestation {
    Attestation {
        capability_id: capability_id.to_owned(),
        // Filled in from the row read that precedes the POST.
        version: String::new(),
        state: "available".to_owned(),
        reason_code: reason_code.to_owned(),
        reason: reason.to_owned(),
        execution_mode: EXECUTION_MODE_AGENTIC.to_owned(),
        cost_class: cost_class.to_owned(),
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

    /// Attest what one round's dependency probes justify, and ONLY that. A round
    /// in which every probe failed posts nothing at all — it does not even mint a
    /// credential — because "could not measure" must never be written as health,
    /// and a row that is not renewed lapses on its own inside the 5-minute TTL.
    pub async fn attest_dependency_round(&self, probes: DependencyProbes) -> usize {
        let planned = dependency_attestations(probes);
        if planned.is_empty() {
            return 0;
        }
        self.attest_all(&planned).await
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
            // session-core, retrieval-engine or quarry outage must stop renewing
            // them (TTL then takes the rows down, fail-closed), and its recovery
            // must bring them back without an execution-core restart.
            let probes = probe_dependencies(&attestor.org_id).await;
            attestor.attest_dependency_round(probes).await;
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
    /// Every id this reporter attests must be in capability-core's
    /// `genericGlobalHealthCapabilityIDs` allowlist.
    ///
    /// That allowlist exists so this service's health credential cannot become a
    /// universal "make available" authority, and it is enforced in Go — so no
    /// Rust-side test could see it. Live proof 2026-08-26: the reporter attested
    /// `cap.memory.{search,index}` and `cap.agent.spawn`, capability-core
    /// refused all three **403 Forbidden**, and the tools stayed denied at the
    /// policy gate while every Rust test passed. Read across the language
    /// boundary the way `cross_service_loop_contract.rs` does, for the same
    /// reason: the two services deploy separately and neither can depend on the
    /// other.
    #[test]
    fn every_attested_capability_is_allowlisted_by_capability_core() {
        let allowlist_source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../go/services/capability-core/internal/api/availability.go"),
        )
        .expect(
            "capability-core's availability.go is the authority on who may attest what;              if it moved, re-point this test — do not delete it, it is the only check that              an attestation can actually land",
        );
        let start = allowlist_source
            .find("var genericGlobalHealthCapabilityIDs = map[string]struct{}{")
            .expect("the allowlist was renamed; re-point this test");
        let block = &allowlist_source[start..];
        let end = block.find("\n}").expect("unterminated allowlist literal");
        let allowlist = &block[..end];

        let mut missing = Vec::new();
        for capability in [
            SANDBOX_CAPABILITY,
            SHELL_CAPABILITY,
            MEMORY_SEARCH_CAPABILITY,
            MEMORY_INDEX_CAPABILITY,
            AGENT_SPAWN_CAPABILITY,
            PROCESS_CAPABILITY,
            RETRIEVAL_QUERY_CAPABILITY,
            WEB_HTTP_CAPABILITY,
        ] {
            if !allowlist.contains(&format!("\"{capability}\"")) {
                missing.push(capability);
            }
        }
        assert!(
            missing.is_empty(),
            "this reporter attests {missing:?}, which capability-core's \
             genericGlobalHealthCapabilityIDs does not allow — every such attestation is \
             refused 403 and the bound tools stay denied at the policy gate. Add the id \
             there (and justify that execution-core OWNS that runtime), or stop attesting it."
        );
    }

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
                "reason": "Bubblewrap sandbox probe succeeded and the interpreter imported every \
                           library code_interpreter advertises.",
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
        assert_eq!(outcome.interpreter, interpreter_probe().healthy());
    }

    // ---- the interpreter probe: the libraries code_interpreter advertises -------
    //
    // `python3 --version` proved a binary exists, not that it can build the
    // documents the tool's description promises. The model is told it can write a
    // workbook, a deck and a PDF; the attestation must only say "available" when
    // the interpreter can import what produces them, and must name what is missing
    // when it cannot.

    const TEST_PROBE_TIMEOUT: Duration = Duration::from_secs(60);

    /// The probe's contract tests need a real interpreter, as `code_interpreter`'s
    /// own end-to-end tests do; a machine without one skips them out loud.
    fn host_python_runs() -> bool {
        let runs = std::process::Command::new(crate::code_interpreter::PYTHON_PROGRAM)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !runs {
            eprintln!("skipped: no python3 runs on this host");
        }
        runs
    }

    /// The Python packages the sandbox image installs (`Dockerfile`, runtime
    /// stage), reduced to the lower-case name both the apt package
    /// (`python3-docx`) and the pip wheel (`python-pptx==1.0.2`) share with their
    /// import name.
    fn dockerfile_python_packages() -> std::collections::BTreeSet<String> {
        let dockerfile = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Dockerfile"),
        )
        .expect("the sandbox image definition is the authority on what the interpreter can import");
        let runtime = dockerfile
            .split("AS runtime")
            .nth(1)
            .expect("the Dockerfile's runtime stage was renamed; re-point this test");
        let mut packages = std::collections::BTreeSet::new();
        for line in runtime
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#'))
        {
            for token in line.split_whitespace() {
                let token = token.trim_end_matches('\\');
                if let Some(package) = token.strip_prefix("python3-") {
                    // `python3-pip` is the installer, not a library.
                    if package != "pip" {
                        packages.insert(package.to_ascii_lowercase());
                    }
                } else if let Some((package, _pinned_version)) = token.split_once("==") {
                    if let Some(package) = package.strip_prefix("python-") {
                        packages.insert(package.to_ascii_lowercase());
                    }
                }
            }
        }
        packages
    }

    /// The drift guard: a library added to the image but not probed would be
    /// advertised on faith, and one probed but not in the image would keep the
    /// capability unavailable for a module nothing ever installs.
    #[test]
    fn the_probe_covers_exactly_the_python_packages_the_image_installs() {
        let probed: std::collections::BTreeSet<String> = SANDBOX_PYTHON_MODULES
            .iter()
            .map(|module| {
                module
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            })
            .collect();
        let shipped = dockerfile_python_packages();
        // Not vacuous: a Dockerfile layout this parser no longer understands must
        // fail here instead of comparing two empty sets.
        for required in ["pptx", "openpyxl", "matplotlib", "docx", "pil"] {
            assert!(
                shipped.contains(required),
                "the Dockerfile parse lost {required}: {shipped:?}"
            );
        }
        assert_eq!(
            probed, shipped,
            "SANDBOX_PYTHON_MODULES and the sandbox image's python packages disagree"
        );
    }

    #[test]
    fn the_probe_reports_the_modules_the_interpreter_cannot_import() {
        if !host_python_runs() {
            return;
        }
        let probe = probe_python_modules(
            crate::code_interpreter::PYTHON_PROGRAM,
            &[
                "json",
                "verevon_missing_module_a",
                "os.path",
                "os.verevon_missing_submodule",
                "verevon_missing_module_b",
            ],
            TEST_PROBE_TIMEOUT,
        );
        assert!(probe.ran, "the interpreter ran to the end of the probe");
        assert_eq!(
            probe.missing,
            vec![
                "verevon_missing_module_a",
                "os.verevon_missing_submodule",
                "verevon_missing_module_b"
            ],
            "every module that did not import is named, in probe order, and nothing else"
        );
        assert!(!probe.healthy());

        let all_present = probe_python_modules(
            crate::code_interpreter::PYTHON_PROGRAM,
            &["json", "os.path"],
            TEST_PROBE_TIMEOUT,
        );
        assert_eq!(
            all_present,
            InterpreterProbe {
                ran: true,
                missing: Vec::new()
            }
        );
        assert!(all_present.healthy());
    }

    /// "Could not run the interpreter" and "ran it and the modules are missing" are
    /// different measurements: the first names no module, because it measured none.
    #[test]
    fn a_missing_interpreter_is_not_healthy_and_names_no_module() {
        let probe = probe_python_modules(
            "verevon-no-such-interpreter",
            SANDBOX_PYTHON_MODULES,
            TEST_PROBE_TIMEOUT,
        );
        assert_eq!(probe, InterpreterProbe::default());
        assert!(!probe.healthy());
    }

    #[test]
    fn an_interpreter_that_hangs_is_killed_at_the_deadline() {
        if !host_python_runs() {
            return;
        }
        let started = std::time::Instant::now();
        let probe = probe_with_script(
            crate::code_interpreter::PYTHON_PROGRAM,
            "import time\ntime.sleep(120)\n",
            &[],
            Duration::from_millis(800),
        );
        assert_eq!(probe, InterpreterProbe::default());
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "a hung import must cost the boot probe its deadline, not two minutes"
        );
    }

    /// A broken C extension kills the interpreter part-way through an import. That
    /// must read as "could not measure", never as "no module is missing".
    #[test]
    fn a_probe_that_dies_or_stays_silent_is_not_healthy() {
        if !host_python_runs() {
            return;
        }
        let crashed = probe_with_script(
            crate::code_interpreter::PYTHON_PROGRAM,
            "import os\nos._exit(3)\n",
            &[],
            TEST_PROBE_TIMEOUT,
        );
        assert_eq!(crashed, InterpreterProbe::default());
        let silent = probe_with_script(
            crate::code_interpreter::PYTHON_PROGRAM,
            "pass\n",
            &[],
            TEST_PROBE_TIMEOUT,
        );
        assert_eq!(silent, InterpreterProbe::default());
    }

    /// The consequence that matters: a missing library keeps the code runtime
    /// unattested (the shell runtime does not need an interpreter, so it is
    /// unaffected), instead of telling the model it can build a deck it cannot.
    #[test]
    fn a_missing_library_withholds_the_code_runtime_attestation() {
        let degraded = InterpreterProbe {
            ran: true,
            missing: vec!["pptx".to_owned()],
        };
        assert!(!degraded.healthy());
        let planned = attestable(ProbeOutcome {
            sandbox: true,
            interpreter: degraded.healthy(),
        });
        let ids: Vec<&str> = planned.iter().map(|a| a.capability_id.as_str()).collect();
        assert_eq!(ids, vec![SHELL_CAPABILITY]);
    }

    // ---- retrieval + web: the dependency-backed attestations --------------------
    //
    // knowledge_search (cap.retrieval.query) and web_search/web_fetch
    // (cap.tool.http) were advertised, bound to a capability and dispatched, while
    // nothing attested either row — so every call died at the capability gate in
    // ~50 ms, silently. The rules pinned below are the honest way out: attest only
    // from a real probe, and let a failed probe attest NOTHING.

    use crate::knowledge_tools::tests::{spawn_stub, Answer};
    use crate::web_tools::tests::{
        edge_with_ready, minted, mounted_auth, quarry_client, ready_ok, refused,
    };
    use serde_json::json;

    const PROBE_BUDGET: Duration = Duration::from_secs(2);

    /// capability-core's `^[a-z][a-z0-9_]{0,127}$`, restated for the ids added here.
    fn assert_server_reason_code(reason_code: &str) {
        assert!(
            reason_code.starts_with(|c: char| c.is_ascii_lowercase())
                && reason_code
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                && reason_code.len() <= 128,
            "reason_code '{reason_code}' violates the server pattern"
        );
    }

    #[test]
    fn retrieval_attestations_track_the_dependency_probe() {
        assert!(retrieval_attestations(false).is_empty());
        let up = retrieval_attestations(true);
        assert_eq!(
            up.len(),
            1,
            "exactly the one capability knowledge_search is bound to"
        );
        let attestation = &up[0];
        assert_eq!(attestation.capability_id, RETRIEVAL_QUERY_CAPABILITY);
        assert_eq!(attestation.state, "available");
        assert_eq!(attestation.execution_mode, "agentic");
        // The seeded row is bounded (migration 0008).
        assert_eq!(attestation.cost_class, "bounded");
        assert_eq!(attestation.reason_code, "retrieval_engine_ready_probed");
        assert_server_reason_code(&attestation.reason_code);
        assert!(
            attestation.reason.len() <= 500,
            "the server rejects a longer reason"
        );
    }

    #[test]
    fn web_attestations_track_the_dependency_probe() {
        assert!(web_attestations(false).is_empty());
        let up = web_attestations(true);
        assert_eq!(
            up.len(),
            1,
            "exactly the one capability the four web tool names bind to"
        );
        let attestation = &up[0];
        assert_eq!(attestation.capability_id, WEB_HTTP_CAPABILITY);
        assert_eq!(attestation.state, "available");
        assert_eq!(attestation.execution_mode, "agentic");
        // The seeded row is variable (migration 0008): third-party search chain.
        assert_eq!(attestation.cost_class, "variable");
        assert_eq!(
            attestation.reason_code,
            "quarry_edge_ready_credential_probed"
        );
        assert_server_reason_code(&attestation.reason_code);
        assert!(attestation.reason.len() <= 500);
    }

    #[test]
    fn each_probe_gates_only_its_own_attestations() {
        assert!(dependency_attestations(DependencyProbes::default()).is_empty());
        for bits in 0..8_u8 {
            let probes = DependencyProbes {
                session_core: bits & 1 != 0,
                retrieval: bits & 2 != 0,
                web: bits & 4 != 0,
            };
            let ids: Vec<String> = dependency_attestations(probes)
                .into_iter()
                .map(|attestation| attestation.capability_id)
                .collect();
            let mut expected: Vec<&str> = Vec::new();
            if probes.session_core {
                expected.extend([
                    MEMORY_SEARCH_CAPABILITY,
                    MEMORY_INDEX_CAPABILITY,
                    AGENT_SPAWN_CAPABILITY,
                ]);
            }
            if probes.retrieval {
                expected.push(RETRIEVAL_QUERY_CAPABILITY);
            }
            if probes.web {
                expected.push(WEB_HTTP_CAPABILITY);
            }
            assert_eq!(ids, expected, "probes {probes:?}");
        }
    }

    #[test]
    fn readyz_must_report_ready_with_postgres_and_qdrant_true() {
        assert!(readyz_body_is_ready(&json!({
            "status": "ready",
            "service": "retrieval-engine-rs",
            "checks": {
                "postgres": true,
                "qdrant": true,
                "cache": true,
                "gdpr_erasure_consumer": "connected"
            }
        })));
        for not_ready in [
            json!({"status": "not_ready", "checks": {"postgres": true, "qdrant": true}}),
            json!({"status": "ready", "checks": {"postgres": false, "qdrant": true}}),
            json!({"status": "ready", "checks": {"postgres": true, "qdrant": false}}),
            json!({"status": "ready", "checks": {"qdrant": true}}),
            json!({"status": "ready", "checks": {"postgres": true}}),
            json!({"status": "ready", "checks": {}}),
            json!({"status": "ready"}),
            json!({"checks": {"postgres": true, "qdrant": true}}),
            json!({"status": true, "checks": {"postgres": true, "qdrant": true}}),
            // The string "true" is not the boolean true.
            json!({"status": "ready", "checks": {"postgres": "true", "qdrant": "true"}}),
            json!("ready"),
            json!(null),
            json!([]),
        ] {
            assert!(
                !readyz_body_is_ready(&not_ready),
                "{not_ready} must not read as ready"
            );
        }
    }

    #[test]
    fn a_blank_or_absent_ready_url_means_cannot_measure() {
        assert_eq!(retrieval_ready_url_from_value(None), None);
        assert_eq!(retrieval_ready_url_from_value(Some(String::new())), None);
        assert_eq!(retrieval_ready_url_from_value(Some("   ".to_owned())), None);
        assert_eq!(
            retrieval_ready_url_from_value(Some(
                "  http://retrieval-engine:8004/readyz ".to_owned()
            )),
            Some("http://retrieval-engine:8004/readyz".to_owned())
        );
    }

    /// The probe must dial what dispatch dials. `KnowledgeClient::from_env` and
    /// this module resolve the gRPC endpoint through ONE helper; if this module
    /// grew its own env read, an attestation could vouch for an address
    /// `knowledge_search` never uses.
    #[test]
    fn the_retrieval_probe_resolves_the_grpc_endpoint_through_the_shared_helper() {
        let source = include_str!("health_attest.rs");
        let production = source
            .split("mod tests {")
            .next()
            .expect("source has a production part");
        assert!(
            !production.contains("\"DATAPLANE_RETRIEVAL_URL\"")
                && !production.contains("\"DATAPLANE_RETRIEVAL_ADDR\""),
            "health_attest must not read the retrieval endpoint env itself"
        );
        assert!(production.contains("crate::knowledge_tools::retrieval_endpoint_from_env()"));
    }

    /// The readiness URL is a NEW env setting, and an unset one silently attests
    /// nothing, so a compose file that forgot it would leave `knowledge_search`
    /// denied with no error anywhere. Pinned the way
    /// `every_attested_capability_is_allowlisted_by_capability_core` pins the Go
    /// allowlist: by reading the other file.
    #[test]
    fn compose_gives_execution_core_the_retrieval_readiness_url() {
        let compose = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../deploy/docker-compose.yml"),
        )
        .expect("the Model Plane compose file; re-point this test if it moved");
        let mut in_block = false;
        let mut ready_url = None;
        let mut grpc_url = None;
        for line in compose.lines() {
            if line == "  execution-core:" {
                in_block = true;
                continue;
            }
            if !in_block {
                continue;
            }
            // The next two-space-indented `name:` line opens the next service.
            let opens_next_service = line.starts_with("  ")
                && !line.starts_with("   ")
                && !line.trim_start().starts_with('#')
                && line.trim_end().ends_with(':');
            if opens_next_service {
                break;
            }
            let line = line.trim();
            if let Some(value) = line.strip_prefix(&format!("{RETRIEVAL_READY_URL_ENV}:")) {
                ready_url = Some(value.trim().to_owned());
            }
            if let Some(value) = line.strip_prefix("DATAPLANE_RETRIEVAL_URL:") {
                grpc_url = Some(value.trim().to_owned());
            }
        }
        let ready_url = ready_url.unwrap_or_else(|| {
            panic!("the execution-core compose block does not set {RETRIEVAL_READY_URL_ENV}")
        });
        let grpc_url = grpc_url.expect("execution-core's DATAPLANE_RETRIEVAL_URL");
        assert!(
            ready_url.contains("http://retrieval-engine:8004/readyz"),
            "unexpected default: {ready_url}"
        );
        // Same host as the gRPC endpoint it vouches for, different (HTTP) port.
        assert!(
            grpc_url.contains("http://retrieval-engine:"),
            "readiness and dispatch must name the same engine: {grpc_url} vs {ready_url}"
        );
    }

    async fn readyz_stub(response: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/readyz"))
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    fn readyz_ready() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "status": "ready",
            "service": "retrieval-engine-rs",
            "sparse_backend": "tantivy",
            "checks": {
                "postgres": true,
                "qdrant": true,
                "cache": true,
                "gdpr_erasure_consumer": "connected"
            }
        }))
    }

    #[tokio::test]
    async fn the_readiness_leg_is_healthy_only_for_a_200_that_reports_ready() {
        let healthy = readyz_stub(readyz_ready()).await;
        assert!(
            retrieval_readyz_reports_ready(&format!("{}/readyz", healthy.uri()), PROBE_BUDGET)
                .await
        );
        // An unauthenticated GET: no credential rides along.
        let requests = healthy.received_requests().await.expect("recorded");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].headers.get("authorization").is_none());

        let cases = [
            (
                "503 not_ready",
                ResponseTemplate::new(503).set_body_json(json!({
                    "status": "not_ready",
                    "checks": {"postgres": false, "qdrant": true}
                })),
            ),
            (
                "200 but postgres false",
                ResponseTemplate::new(200).set_body_json(json!({
                    "status": "ready",
                    "checks": {"postgres": false, "qdrant": true}
                })),
            ),
            (
                "200 but qdrant false",
                ResponseTemplate::new(200).set_body_json(json!({
                    "status": "ready",
                    "checks": {"postgres": true, "qdrant": false}
                })),
            ),
            (
                "200 with no checks",
                ResponseTemplate::new(200).set_body_json(json!({"status": "ready"})),
            ),
            (
                "200 with no status",
                ResponseTemplate::new(200)
                    .set_body_json(json!({"checks": {"postgres": true, "qdrant": true}})),
            ),
            (
                "200 with malformed JSON",
                ResponseTemplate::new(200).set_body_string("{\"status\": \"ready\""),
            ),
            (
                "200 with an HTML body",
                ResponseTemplate::new(200).set_body_string("<html>ok</html>"),
            ),
            ("404", ResponseTemplate::new(404)),
        ];
        for (name, response) in cases {
            let server = readyz_stub(response).await;
            assert!(
                !retrieval_readyz_reports_ready(&format!("{}/readyz", server.uri()), PROBE_BUDGET)
                    .await,
                "{name} must not read as ready"
            );
        }
    }

    #[tokio::test]
    async fn the_readiness_leg_does_not_follow_redirects_and_survives_dead_hosts() {
        let sink = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(readyz_ready())
            .expect(0)
            .mount(&sink)
            .await;
        let redirecting = readyz_stub(
            ResponseTemplate::new(307).insert_header("location", format!("{}/readyz", sink.uri())),
        )
        .await;
        assert!(
            !retrieval_readyz_reports_ready(&format!("{}/readyz", redirecting.uri()), PROBE_BUDGET)
                .await,
            "a redirect target's answer is not the engine's own"
        );

        assert!(
            !retrieval_readyz_reports_ready("http://127.0.0.1:1/readyz", PROBE_BUDGET).await,
            "an unreachable host is not ready"
        );
        assert!(!retrieval_readyz_reports_ready("not a url", PROBE_BUDGET).await);

        let hung = readyz_stub(readyz_ready().set_delay(Duration::from_secs(5))).await;
        let started = std::time::Instant::now();
        assert!(
            !retrieval_readyz_reports_ready(
                &format!("{}/readyz", hung.uri()),
                Duration::from_millis(250)
            )
            .await
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the leg must give up at its own timeout"
        );
    }

    #[tokio::test]
    async fn the_retrieval_probe_needs_both_the_readiness_endpoint_and_the_auth_boundary() {
        let ready = readyz_stub(readyz_ready()).await;
        let ready_url = format!("{}/readyz", ready.uri());
        let not_ready =
            readyz_stub(ResponseTemplate::new(503).set_body_json(json!({"status": "not_ready"})))
                .await;
        let not_ready_url = format!("{}/readyz", not_ready.uri());

        let (alive, _) = spawn_stub(Answer::Unauthenticated).await;
        assert!(retrieval_engine_probe(&alive, &ready_url, PROBE_BUDGET).await);
        // Postgres/Qdrant down: the gRPC path answering is not enough.
        assert!(!retrieval_engine_probe(&alive, &not_ready_url, PROBE_BUDGET).await);

        // The engine reports ready but the path dispatch uses is not alive.
        for answer in [
            Answer::Unavailable,
            Answer::Unimplemented,
            Answer::PermissionDenied,
            Answer::Success,
        ] {
            let (endpoint, _) = spawn_stub(answer).await;
            assert!(
                !retrieval_engine_probe(&endpoint, &ready_url, PROBE_BUDGET).await,
                "{answer:?} on the gRPC path must keep the capability unattested"
            );
        }
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
            listener.local_addr().expect("address").port()
        };
        assert!(
            !retrieval_engine_probe(
                &format!("http://127.0.0.1:{port}"),
                &ready_url,
                PROBE_BUDGET
            )
            .await
        );
    }

    #[tokio::test]
    async fn a_slow_grpc_leg_cannot_stall_the_retrieval_probe() {
        let ready = readyz_stub(readyz_ready()).await;
        let (slow, _) = spawn_stub(Answer::TooSlow).await;
        let started = std::time::Instant::now();
        assert!(
            !retrieval_engine_probe(
                &slow,
                &format!("{}/readyz", ready.uri()),
                Duration::from_millis(300)
            )
            .await
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn the_web_probe_is_healthy_only_when_the_edge_is_ready_and_both_credentials_mint() {
        let edge = edge_with_ready(ready_ok()).await;
        let auth = mounted_auth(minted("search-token"), minted("extract-token"), "global").await;
        assert!(
            web_probe(
                &quarry_client(&edge.uri(), &auth.uri()),
                "global",
                PROBE_BUDGET
            )
            .await
        );

        // Auth Core refusing either scope (today's live state: both) keeps the
        // capability unavailable, whatever the edge says.
        for (search, extract) in [
            (refused(), refused()),
            (refused(), minted("extract-token")),
            (minted("search-token"), refused()),
        ] {
            let edge = edge_with_ready(ready_ok()).await;
            let auth = mounted_auth(search, extract, "global").await;
            assert!(
                !web_probe(
                    &quarry_client(&edge.uri(), &auth.uri()),
                    "global",
                    PROBE_BUDGET
                )
                .await
            );
        }

        let not_ready_edge = edge_with_ready(
            ResponseTemplate::new(503).set_body_json(json!({"status": "not_ready"})),
        )
        .await;
        let auth = mounted_auth(minted("a"), minted("b"), "global").await;
        assert!(
            !web_probe(
                &quarry_client(&not_ready_edge.uri(), &auth.uri()),
                "global",
                PROBE_BUDGET
            )
            .await
        );
    }

    #[tokio::test]
    async fn a_hung_token_endpoint_is_cut_off_at_the_web_probe_timeout() {
        let edge = edge_with_ready(ready_ok()).await;
        let auth = mounted_auth(
            minted("search-token").set_delay(Duration::from_secs(5)),
            minted("extract-token"),
            "global",
        )
        .await;
        let started = std::time::Instant::now();
        assert!(
            !web_probe(
                &quarry_client(&edge.uri(), &auth.uri()),
                "global",
                Duration::from_millis(300)
            )
            .await
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "a hung Auth Core must not stall the heartbeat past the probe timeout"
        );
    }

    /// A capability-core stub that serves the given rows (version `1.0.0`) and
    /// accepts any attestation.
    async fn capability_core_serving(ids: &[&str]) -> MockServer {
        let capability_core = MockServer::start().await;
        for id in ids {
            row_mock(id, "1.0.0").mount(&capability_core).await;
        }
        Mock::given(method("POST"))
            .and(path("/api/v1/capabilities/availability"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .mount(&capability_core)
            .await;
        capability_core
    }

    /// The JSON bodies of every attestation POST the stub received, in order.
    async fn posted_attestations(capability_core: &MockServer) -> Vec<serde_json::Value> {
        capability_core
            .received_requests()
            .await
            .expect("recorded requests")
            .iter()
            .filter(|request| {
                request.method.as_str() == "POST"
                    && request.url.path() == "/api/v1/capabilities/availability"
            })
            .map(|request| serde_json::from_slice(&request.body).expect("attestation JSON"))
            .collect()
    }

    #[tokio::test]
    async fn a_round_in_which_every_probe_failed_posts_nothing_and_mints_nothing() {
        let auth = MockServer::start().await;
        token_mock().expect(0).mount(&auth).await;
        let capability_core = capability_core_serving(&[
            MEMORY_SEARCH_CAPABILITY,
            RETRIEVAL_QUERY_CAPABILITY,
            WEB_HTTP_CAPABILITY,
        ])
        .await;
        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());

        assert_eq!(
            attestor
                .attest_dependency_round(DependencyProbes::default())
                .await,
            0
        );
        assert!(
            capability_core
                .received_requests()
                .await
                .expect("recorded")
                .is_empty(),
            "nothing may reach capability-core when no probe passed"
        );
        assert!(
            auth.received_requests().await.expect("recorded").is_empty(),
            "no credential is even minted when there is nothing to attest"
        );
    }

    #[tokio::test]
    async fn a_retrieval_only_round_attests_only_the_retrieval_row() {
        // session-core down, quarry down, retrieval up.
        let auth = MockServer::start().await;
        token_mock().mount(&auth).await;
        let capability_core = capability_core_serving(&[
            MEMORY_SEARCH_CAPABILITY,
            MEMORY_INDEX_CAPABILITY,
            AGENT_SPAWN_CAPABILITY,
            RETRIEVAL_QUERY_CAPABILITY,
            WEB_HTTP_CAPABILITY,
        ])
        .await;
        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());

        let attested = attestor
            .attest_dependency_round(DependencyProbes {
                session_core: false,
                retrieval: true,
                web: false,
            })
            .await;
        assert_eq!(attested, 1);

        let posted = posted_attestations(&capability_core).await;
        assert_eq!(posted.len(), 1, "only the retrieval row: {posted:?}");
        assert_eq!(
            posted[0],
            json!({
                "id": RETRIEVAL_QUERY_CAPABILITY,
                "version": "1.0.0",
                "state": "available",
                "reason_code": "retrieval_engine_ready_probed",
                "reason": retrieval_attestations(true)[0].reason,
                "execution_mode": "agentic",
                "cost_class": "bounded",
            })
        );
    }

    #[tokio::test]
    async fn a_web_only_round_attests_cap_tool_http_with_the_variable_cost_class() {
        let auth = MockServer::start().await;
        token_mock().mount(&auth).await;
        let capability_core = capability_core_serving(&[WEB_HTTP_CAPABILITY]).await;
        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());

        let attested = attestor
            .attest_dependency_round(DependencyProbes {
                session_core: false,
                retrieval: false,
                web: true,
            })
            .await;
        assert_eq!(attested, 1);

        let posted = posted_attestations(&capability_core).await;
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0]["id"], WEB_HTTP_CAPABILITY);
        assert_eq!(posted[0]["cost_class"], "variable");
        assert_eq!(
            posted[0]["reason_code"],
            "quarry_edge_ready_credential_probed"
        );
        assert_eq!(posted[0]["state"], "available");
        // Only an attestation body is posted — never a minted credential.
        let body = posted[0].to_string();
        assert!(!body.contains("health-token"));
    }

    #[tokio::test]
    async fn a_round_with_every_dependency_up_attests_all_five_rows_in_order() {
        let auth = MockServer::start().await;
        token_mock().mount(&auth).await;
        let ids = [
            MEMORY_SEARCH_CAPABILITY,
            MEMORY_INDEX_CAPABILITY,
            AGENT_SPAWN_CAPABILITY,
            RETRIEVAL_QUERY_CAPABILITY,
            WEB_HTTP_CAPABILITY,
        ];
        let capability_core = capability_core_serving(&ids).await;
        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());

        let attested = attestor
            .attest_dependency_round(DependencyProbes {
                session_core: true,
                retrieval: true,
                web: true,
            })
            .await;
        assert_eq!(attested, 5);
        let posted: Vec<String> = posted_attestations(&capability_core)
            .await
            .iter()
            .map(|body| body["id"].as_str().expect("id").to_owned())
            .collect();
        assert_eq!(posted, ids);
    }

    #[tokio::test]
    async fn one_rows_failed_attestation_never_blocks_the_others() {
        let auth = MockServer::start().await;
        token_mock().mount(&auth).await;
        let capability_core = MockServer::start().await;
        // retrieval's row read fails; web's succeeds.
        Mock::given(method("GET"))
            .and(path(format!(
                "/api/v1/capabilities/{RETRIEVAL_QUERY_CAPABILITY}"
            )))
            .respond_with(ResponseTemplate::new(500))
            .mount(&capability_core)
            .await;
        row_mock(WEB_HTTP_CAPABILITY, "1.0.0")
            .mount(&capability_core)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v1/capabilities/availability"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .expect(1)
            .mount(&capability_core)
            .await;
        let attestor = HealthAttestor::new_for_test(&capability_core.uri(), &auth.uri());

        let attested = attestor
            .attest_dependency_round(DependencyProbes {
                session_core: false,
                retrieval: true,
                web: true,
            })
            .await;
        assert_eq!(attested, 1);
        let posted = posted_attestations(&capability_core).await;
        assert_eq!(posted[0]["id"], WEB_HTTP_CAPABILITY);
    }
}
