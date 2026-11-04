//! The org agent policy as the gateway enforces it (build plan WS25-S9, DEC-13 and DEC-14).
//!
//! capability-core is the system of record for an org's agent policy (WS25-S7). This module reads
//! the org's document through `GET /api/v1/org-agent-policy`, holds it briefly, and turns it into the
//! caps the gateway enforces: the mode cap (with the lockdown bundle folded in) and the rung cap. The
//! fetch follows the shape DEC-13 sets for every entitlement the gateway reads: a 30 s TTL, and when a
//! refresh fails the last answer may still be served for 900 s if it was restrictive or 120 s if it
//! was not. Past that bound the org's policy is unknown, and a Do run is refused with the typed code
//! `org_policy_unavailable`. A plain turn never waits on the policy: it keeps the floor it had.
//!
//! `ORG_AGENT_POLICY_MODE` decides how far any of this binds. Unset, empty or `off` reads nothing and
//! changes nothing. `shadow` reads and logs what would bind but applies nothing. Any other value
//! enforces, because an authority switch treats an unknown value as enforce (K3-26).
//!
//! The rung dispatch and the grant are synchronous, so they read the cache the turn's admission
//! refreshed ([`admit_turn`]) instead of fetching again.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use mp_contracts::agent_mode::{self, AgentMode, Resolved};
use mp_contracts::autonomy;
use mp_contracts::model_plane::v1::{AutonomyRung, OrgPolicyContext, WebPolicyContext};
use mp_contracts::org_agent_policy::{Kind, SettingDef, SETTINGS};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::profile::{mode_floor_from, ApprovalPosture};

/// The environment switch of the org agent policy's enforcement (WS25-S9). Read once, by [`policy_mode`].
pub const ORG_AGENT_POLICY_MODE_ENV: &str = "ORG_AGENT_POLICY_MODE";

/// The typed code of a Do run refused because its org policy cannot be read inside its bound (D8).
pub const POLICY_UNAVAILABLE_CODE: &str = "org_policy_unavailable";

/// The words a person reads with [`POLICY_UNAVAILABLE_CODE`]. The code is the contract; the words are copy.
pub const POLICY_UNAVAILABLE_MESSAGE: &str =
    "The organisation's agent policy cannot be read right now, so this agent run cannot start. Try again shortly.";

/// How long a read org policy is served without asking capability-core again (DEC-13, D8).
pub const POLICY_TTL: Duration = Duration::from_secs(30);

/// How long the last answer may still be served once a refresh has failed. A restrictive answer holds
/// longer than a permissive one, so a registry outage cannot quietly loosen an org that had narrowed itself (D8).
pub const POLICY_STALE_RESTRICTIVE: Duration = Duration::from_secs(900);

/// The stale bound of an answer that narrows nothing (D8).
pub const POLICY_STALE_PERMISSIVE: Duration = Duration::from_secs(120);

/// capability-core's org policy route, relative to its base address.
const ORG_AGENT_POLICY_PATH: &str = "/api/v1/org-agent-policy";

/// How far the org policy binds (`ORG_AGENT_POLICY_MODE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyMode {
    /// Nothing is read and nothing changes.
    Off,
    /// The policy is read and what it would bind is logged; nothing is applied.
    Shadow,
    /// The policy binds, and a policy that cannot be read refuses a Do run.
    Enforce,
}

impl PolicyMode {
    /// Parses the switch. Unset, empty and `off` are [`PolicyMode::Off`], `shadow` is [`PolicyMode::Shadow`],
    /// and any other value enforces.
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            None | Some("" | "off") => Self::Off,
            Some("shadow") => Self::Shadow,
            Some(_) => Self::Enforce,
        }
    }
}

/// The process's mode, read once so it cannot change under a request.
#[must_use]
pub fn policy_mode() -> PolicyMode {
    static MODE: OnceLock<PolicyMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        PolicyMode::from_env_value(std::env::var(ORG_AGENT_POLICY_MODE_ENV).ok().as_deref())
    })
}

/// The org's web switches as the gateway enforces them (Wave 3 EP-9 W2). Each is read from its key of the document
/// with the schema's fail-closed value for any token this build cannot read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebCaps {
    /// Whether `web.mode` is `live`. `off` withholds public web access from every agent path; so does any token this
    /// build does not enforce (`cached_only` is reserved and never stored).
    pub live: bool,
    /// Whether `web.paid_providers` is `on`: a paid search provider may serve the org's agents.
    pub paid: bool,
}

impl WebCaps {
    /// The switches an enforced org the gateway cannot see is held to: the schema's fail-closed values.
    pub const FAIL_CLOSED: Self = Self {
        live: false,
        paid: false,
    };
}

/// An org's policy as the gateway enforces it, built from the document capability-core answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgPolicy {
    /// The policy revision. Zero when the org has no row and the platform defaults apply.
    pub revision: i64,
    /// The digest of the document the revision names.
    pub digest: String,
    /// The strictest mode a turn of this org may run under: `Lockdown` when the lockdown bundle is set, else `mode_cap`.
    pub mode_cap: AgentMode,
    /// The strictest rung a grant may reach. `None` when the org sets no cap below the loosest rung.
    pub rung_cap: Option<AutonomyRung>,
    /// Whether the document narrows anything from the defaults. This decides how long a stale answer may still be served.
    pub restrictive: bool,
    /// The most items one wide-research run may fan out (`research.max_fan_out_items`, EP-2 S8). A value this build
    /// cannot read is 0: wide research off for the org.
    pub max_fan_out_items: u32,
    /// The org's web switches (`web.mode`, `web.paid_providers`, Wave 3 EP-9 W2).
    pub web: WebCaps,
}

impl OrgPolicy {
    /// Reads a document. A key the document omits takes its default. A value this build cannot read takes the
    /// key's fail-closed value from the schema, so a malformed value can only narrow the org, never loosen it.
    #[must_use]
    pub fn from_settings(revision: i64, digest: &str, settings: &Map<String, Value>) -> Self {
        let token = |key: &str| {
            SETTINGS
                .iter()
                .find(|def| def.key == key)
                .map_or_else(String::new, |def| token_of(def, settings))
        };
        let mode_cap = if token("lockdown") == "true" {
            AgentMode::Lockdown
        } else {
            AgentMode::parse(&token("mode_cap")).unwrap_or(AgentMode::Lockdown)
        };
        Self {
            revision,
            digest: digest.to_owned(),
            mode_cap,
            rung_cap: rung_from_token(&token("rung_cap")),
            restrictive: SETTINGS
                .iter()
                .any(|def| token_of(def, settings) != def.default),
            max_fan_out_items: token(mp_contracts::org_agent_policy::RESEARCH_MAX_FAN_OUT_ITEMS)
                .parse()
                .unwrap_or(0),
            web: WebCaps {
                live: token("web.mode") == "live",
                paid: token("web.paid_providers") == "on",
            },
        }
    }
}

/// The token a document holds for one key, in the key's own order: its default when the key is absent, and its
/// fail-closed value when the stored value cannot be read.
fn token_of(def: &SettingDef, settings: &Map<String, Value>) -> String {
    let Some(value) = settings.get(def.key) else {
        return def.default.to_owned();
    };
    // An integer key's token is its decimal form, held here so the match can lend it out.
    let count;
    let read = match def.kind {
        Kind::Enum => value.as_str(),
        Kind::Bool => value
            .as_bool()
            .map(|flag| if flag { "true" } else { "false" }),
        Kind::AllowList => value.get("mode").and_then(Value::as_str),
        Kind::Integer => {
            count = value.as_u64().map(|n| n.to_string());
            count.as_deref()
        }
    };
    read.filter(|token| def.strictness_order.iter().any(|allowed| allowed == token))
        .unwrap_or(def.fail_closed_value)
        .to_owned()
}

/// The rung a cap token names. `danger_full_access` is the loosest rung and caps nothing, so it is `None`.
/// Any token this build cannot read is the narrowest rung, never a wider one.
fn rung_from_token(token: &str) -> Option<AutonomyRung> {
    match token {
        "danger_full_access" => None,
        "workspace_write" => Some(AutonomyRung::WorkspaceWrite),
        _ => Some(AutonomyRung::ReadOnly),
    }
}

/// capability-core's answer to `GET /api/v1/org-agent-policy`.
#[derive(Debug, Deserialize)]
struct PolicyAnswer {
    #[serde(default)]
    org_id: String,
    revision: i64,
    #[serde(default)]
    digest: String,
    #[serde(default)]
    settings: Map<String, Value>,
}

/// One org's policy as capability-core last answered for it, and when.
#[derive(Debug)]
struct Snapshot {
    policy: Arc<OrgPolicy>,
    fetched: Instant,
}

/// The org policies this replica holds. A refresh replaces an entry whole, so a reader never sees two answers mixed.
#[derive(Debug, Default)]
pub struct OrgPolicyCache {
    entries: DashMap<String, Arc<Snapshot>>,
}

impl OrgPolicyCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds a policy read at `fetched`. Production records answers through [`ensure`]; this is the seam tests use.
    pub fn store(&self, org_id: &str, policy: OrgPolicy, fetched: Instant) {
        self.insert(org_id, Arc::new(policy), fetched);
    }

    fn insert(&self, org_id: &str, policy: Arc<OrgPolicy>, fetched: Instant) {
        self.entries
            .insert(org_id.to_owned(), Arc::new(Snapshot { policy, fetched }));
    }

    fn snapshot(&self, org_id: &str) -> Option<Arc<Snapshot>> {
        self.entries
            .get(org_id)
            .map(|entry| Arc::clone(entry.value()))
    }

    fn remove(&self, org_id: &str) {
        self.entries.remove(org_id);
    }

    /// The policy held for an org, if any. The synchronous dispatch points read it here.
    #[must_use]
    pub fn policy(&self, org_id: &str) -> Option<Arc<OrgPolicy>> {
        self.snapshot(org_id)
            .map(|snapshot| Arc::clone(&snapshot.policy))
    }
}

/// The cache this process shares across requests.
#[must_use]
pub fn shared_cache() -> &'static OrgPolicyCache {
    static CACHE: OnceLock<OrgPolicyCache> = OnceLock::new();
    CACHE.get_or_init(OrgPolicyCache::new)
}

fn fresh(snapshot: &Snapshot, now: Instant) -> bool {
    now.saturating_duration_since(snapshot.fetched) <= POLICY_TTL
}

fn servable_when_stale(snapshot: &Snapshot, now: Instant) -> bool {
    let bound = if snapshot.policy.restrictive {
        POLICY_STALE_RESTRICTIVE
    } else {
        POLICY_STALE_PERMISSIVE
    };
    now.saturating_duration_since(snapshot.fetched) <= bound
}

/// Reads an org's policy from capability-core and replaces the held entry with the answer.
///
/// A refused bearer (401, 403) names the caller's credential, not the registry, and changes nothing that
/// [`ensure`] would otherwise keep. Any other failure leaves the entry where it was, and [`ensure`] decides
/// whether the last answer may still be served.
async fn hydrate(
    cache: &OrgPolicyCache,
    client: &reqwest::Client,
    base_url: &str,
    org_id: &str,
    bearer: &str,
    now: Instant,
) -> Result<Arc<OrgPolicy>, String> {
    let bearer = bearer.trim();
    if bearer.is_empty() {
        return Err("capability bearer is unavailable".to_owned());
    }
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("capability registry is unavailable".to_owned());
    }
    let response = client
        .get(format!("{base}{ORG_AGENT_POLICY_PATH}"))
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|error| format!("capability registry request failed: {error}"))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(format!(
            "capability registry refused the bearer: HTTP {status}"
        ));
    }
    if !status.is_success() {
        return Err(format!("capability registry returned HTTP {status}"));
    }
    let answer: PolicyAnswer = response
        .json()
        .await
        .map_err(|error| format!("capability registry answer is not an org policy: {error}"))?;
    // capability-core scopes the read by the verified caller, so an answer for another tenant is a registry defect.
    // Refuse it rather than hold one tenant's policy under another's id.
    if !answer.org_id.is_empty() && answer.org_id != org_id {
        return Err(format!(
            "capability registry answered for org '{}', not '{org_id}'",
            answer.org_id
        ));
    }
    let policy = Arc::new(OrgPolicy::from_settings(
        answer.revision,
        &answer.digest,
        &answer.settings,
    ));
    cache.insert(org_id, Arc::clone(&policy), now);
    Ok(policy)
}

/// The org's policy for one admission, read through the cache. It is served while fresh, refreshed once its TTL
/// lapses, and when a refresh fails it is served only inside its D8 bound. Past the bound the held entry is dropped
/// and the call fails, so the caller refuses rather than run under a policy it can no longer see.
///
/// # Errors
///
/// Why the policy cannot be known: the refresh failed and no answer is inside its bound.
pub async fn ensure(
    cache: &OrgPolicyCache,
    client: &reqwest::Client,
    base_url: &str,
    org_id: &str,
    bearer: &str,
) -> Result<Arc<OrgPolicy>, String> {
    ensure_at(cache, client, base_url, org_id, bearer, Instant::now()).await
}

async fn ensure_at(
    cache: &OrgPolicyCache,
    client: &reqwest::Client,
    base_url: &str,
    org_id: &str,
    bearer: &str,
    now: Instant,
) -> Result<Arc<OrgPolicy>, String> {
    let current = cache.snapshot(org_id);
    if let Some(held) = current.as_deref() {
        if fresh(held, now) {
            return Ok(Arc::clone(&held.policy));
        }
    }
    match hydrate(cache, client, base_url, org_id, bearer, now).await {
        Ok(policy) => Ok(policy),
        Err(error) => {
            if let Some(held) = current.as_deref() {
                if servable_when_stale(held, now) {
                    tracing::warn!(
                        %org_id,
                        %error,
                        "org agent policy refresh failed; serving the last answer inside its bound"
                    );
                    return Ok(Arc::clone(&held.policy));
                }
            }
            cache.remove(org_id);
            Err(error)
        }
    }
}

/// Decides, at admission and before the turn's mode is resolved, what the org policy does to one turn. Returns the
/// org's mode cap that binds the turn, or `None` when nothing binds it. The rung cap is read later, at dispatch,
/// from the cache this call leaves warm.
///
/// # Errors
///
/// [`POLICY_UNAVAILABLE_CODE`] when the policy is enforced, the turn is a Do run, and the policy cannot be read
/// inside its bound. A plain turn is not refused: it keeps the floor it already had.
pub async fn admit_turn(
    mode: PolicyMode,
    cache: &OrgPolicyCache,
    client: &reqwest::Client,
    base_url: &str,
    org_id: &str,
    bearer: &str,
    agentic: bool,
) -> Result<Option<AgentMode>, &'static str> {
    match mode {
        PolicyMode::Off => Ok(None),
        PolicyMode::Shadow => {
            match ensure(cache, client, base_url, org_id, bearer).await {
                Ok(policy) => {
                    if policy.mode_cap != AgentMode::Auto {
                        tracing::info!(
                            %org_id,
                            would_mode_cap = policy.mode_cap.as_str(),
                            "org agent policy (shadow): the mode cap would bind this turn"
                        );
                    }
                    if !policy.web.live {
                        tracing::info!(
                            %org_id,
                            "org agent policy (shadow): web access would be withheld for this turn"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(%org_id, %error, "org agent policy (shadow) could not be read")
                }
            }
            Ok(None)
        }
        PolicyMode::Enforce => match ensure(cache, client, base_url, org_id, bearer).await {
            Ok(policy) => Ok(Some(policy.mode_cap)),
            Err(error) => {
                tracing::warn!(%org_id, %error, agentic, "org agent policy cannot be read");
                if agentic {
                    Err(POLICY_UNAVAILABLE_CODE)
                } else {
                    Ok(None)
                }
            }
        },
    }
}

/// The mode a turn runs under with the org's cap applied: the strictest of the platform floor its posture states,
/// the org's cap and the mode the person chose. `None` when neither the person nor the org asked for a mode, so such
/// a turn is exactly what it was before the policy existed.
#[must_use]
pub fn resolve_turn(
    posture: ApprovalPosture,
    chosen: Option<AgentMode>,
    org_cap: Option<AgentMode>,
) -> Option<Resolved> {
    if chosen.is_none() && org_cap.is_none_or(|cap| cap == AgentMode::Auto) {
        return None;
    }
    Some(agent_mode::resolve(
        mode_floor_from(posture),
        org_cap,
        chosen,
        &[],
    ))
}

/// The rung cap the cache holds for an org, when the switch enforces it and the policy is held. `None` when the switch
/// does not enforce, the policy is not held, or the org sets no cap.
#[must_use]
pub fn held_rung_cap(
    mode: PolicyMode,
    cache: &OrgPolicyCache,
    org_id: &str,
) -> Option<AutonomyRung> {
    match mode {
        PolicyMode::Enforce => cache.policy(org_id).and_then(|policy| policy.rung_cap),
        PolicyMode::Off | PolicyMode::Shadow => None,
    }
}

/// The rung cap a fresh run is held to at dispatch. An enforced org whose policy is not held is held to the narrowest
/// rung: the admission that should have read it left no copy, and an unknown policy must not widen a run.
#[must_use]
pub fn dispatch_rung_cap(
    mode: PolicyMode,
    cache: &OrgPolicyCache,
    org_id: &str,
) -> Option<AutonomyRung> {
    if mode == PolicyMode::Enforce && cache.policy(org_id).is_none() {
        return Some(AutonomyRung::ReadOnly);
    }
    held_rung_cap(mode, cache, org_id)
}

/// The rung a run states under an org's rung cap. A grant is held to the cap: the narrower of the two. With no grant
/// the cap itself is stated, because an unstated rung is no constraint in the loop, so it would hold the run to nothing.
/// With no cap the grant passes through unchanged.
#[must_use]
pub fn clamp_rung(
    granted: Option<AutonomyRung>,
    cap: Option<AutonomyRung>,
) -> Option<AutonomyRung> {
    match (granted, cap) {
        (granted, None) => granted,
        (None, Some(cap)) => Some(cap),
        (Some(granted), Some(cap)) => Some(if autonomy::rank(granted) <= autonomy::rank(cap) {
            granted
        } else {
            cap
        }),
    }
}

/// The policy revision and digest a run is attributed to, when the switch enforces and the policy is held.
#[must_use]
pub fn held_revision(
    mode: PolicyMode,
    cache: &OrgPolicyCache,
    org_id: &str,
) -> Option<(i64, String)> {
    match mode {
        PolicyMode::Enforce => cache
            .policy(org_id)
            .map(|policy| (policy.revision, policy.digest.clone())),
        PolicyMode::Off | PolicyMode::Shadow => None,
    }
}

/// Attributes a Do run's org policy context to the revision it was admitted under. A run with no hook rules still
/// carries the revision, so the run can be tied to a policy whether or not it has rules.
pub fn stamp_policy_revision(
    org_policy: &mut Option<OrgPolicyContext>,
    held: Option<(i64, String)>,
) {
    let Some((revision, digest)) = held else {
        return;
    };
    let context = org_policy.get_or_insert_with(OrgPolicyContext::default);
    context.policy_revision = revision;
    context.policy_digest = digest;
}

/// The org's web switches for an inline turn, when the switch enforces them and the policy is held. `None` leaves the
/// turn's web access as the person chose it: a turn is never refused web access because its policy cannot be read,
/// since an outage must not silently break an answer.
#[must_use]
pub fn held_web_caps(mode: PolicyMode, cache: &OrgPolicyCache, org_id: &str) -> Option<WebCaps> {
    match mode {
        PolicyMode::Enforce => cache.policy(org_id).map(|policy| policy.web),
        PolicyMode::Off | PolicyMode::Shadow => None,
    }
}

/// Whether the org's switch lets a paid search provider serve this turn. It reads the cache the turn's admission left
/// warm, as [`held_rung_cap`] does, so the search path needs no extra parameter.
#[must_use]
pub fn org_allows_paid_providers(org_id: &str) -> bool {
    held_web_caps(policy_mode(), shared_cache(), org_id).is_none_or(|caps| caps.paid)
}

/// Carries the org's web switches on a Do run's policy context, so execution-core withholds the web tools by the policy
/// the run was admitted under. Enforced and held: the held switches and their revision. Enforced but not held: the
/// fail-closed switches at revision zero, as the rung is held to the narrowest rung ([`dispatch_rung_cap`]). Off and
/// shadow stamp nothing, so the request is exactly as it was.
pub fn stamp_web_policy(
    mode: PolicyMode,
    cache: &OrgPolicyCache,
    org_id: &str,
    org_policy: &mut Option<OrgPolicyContext>,
) {
    let (caps, revision) = match mode {
        PolicyMode::Enforce => match cache.policy(org_id) {
            Some(policy) => (policy.web, policy.revision),
            None => (WebCaps::FAIL_CLOSED, 0),
        },
        PolicyMode::Off | PolicyMode::Shadow => return,
    };
    let context = org_policy.get_or_insert_with(OrgPolicyContext::default);
    context.web = Some(WebPolicyContext {
        mode: if caps.live { "live" } else { "off" }.to_owned(),
        paid_providers: caps.paid,
        revision,
        site_policy_revision: 0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn settings(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    fn document(org: &str, revision: i64, body: Value) -> Value {
        json!({
            "org_id": org,
            "revision": revision,
            "digest": format!("sha256:{revision}"),
            "settings": body,
        })
    }

    #[test]
    fn org_agent_policy_reads_the_wide_research_item_cap() {
        assert_eq!(OrgPolicy::from_settings(0, "", &settings(json!({}))).max_fan_out_items, 8);
        assert_eq!(
            OrgPolicy::from_settings(1, "d", &settings(json!({"research.max_fan_out_items": 3})))
                .max_fan_out_items,
            3
        );
        // A value this build cannot read turns wide research off for the org, never on.
        assert_eq!(
            OrgPolicy::from_settings(1, "d", &settings(json!({"research.max_fan_out_items": "x"})))
                .max_fan_out_items,
            0
        );
    }

    #[test]
    fn org_agent_policy_mode_switch_reads_off_shadow_and_enforce() {
        assert_eq!(PolicyMode::from_env_value(None), PolicyMode::Off);
        assert_eq!(PolicyMode::from_env_value(Some("")), PolicyMode::Off);
        assert_eq!(PolicyMode::from_env_value(Some("off")), PolicyMode::Off);
        assert_eq!(
            PolicyMode::from_env_value(Some("shadow")),
            PolicyMode::Shadow
        );
        assert_eq!(
            PolicyMode::from_env_value(Some("enforce")),
            PolicyMode::Enforce
        );
        // An unknown value enforces: a typo must not leave an org unrestricted.
        assert_eq!(PolicyMode::from_env_value(Some("on")), PolicyMode::Enforce);
    }

    #[test]
    fn org_agent_policy_a_document_with_no_keys_is_the_platform_default() {
        let policy = OrgPolicy::from_settings(0, "", &Map::new());
        assert_eq!(policy.mode_cap, AgentMode::Auto);
        assert_eq!(policy.rung_cap, None);
        assert!(!policy.restrictive);
    }

    #[test]
    fn org_agent_policy_caps_are_read_from_the_document() {
        let policy = OrgPolicy::from_settings(
            4,
            "sha256:4",
            &settings(json!({"mode_cap": "read_only", "rung_cap": "workspace_write"})),
        );
        assert_eq!(policy.mode_cap, AgentMode::ReadOnly);
        assert_eq!(policy.rung_cap, Some(AutonomyRung::WorkspaceWrite));
        assert!(policy.restrictive);
    }

    #[test]
    fn org_agent_policy_the_lockdown_bundle_locks_the_mode_whatever_the_mode_cap_says() {
        let policy = OrgPolicy::from_settings(
            1,
            "",
            &settings(json!({"mode_cap": "auto", "lockdown": true})),
        );
        assert_eq!(policy.mode_cap, AgentMode::Lockdown);
    }

    #[test]
    fn org_agent_policy_an_unreadable_value_takes_its_fail_closed_value() {
        let policy = OrgPolicy::from_settings(
            1,
            "",
            &settings(json!({"mode_cap": "full_access", "rung_cap": 7})),
        );
        assert_eq!(policy.mode_cap, AgentMode::Lockdown);
        assert_eq!(policy.rung_cap, Some(AutonomyRung::ReadOnly));
    }

    #[test]
    fn org_agent_policy_a_cap_binds_a_turn_the_person_did_not_choose() {
        let resolved = resolve_turn(ApprovalPosture::Auto, None, Some(AgentMode::ReadOnly))
            .expect("the cap binds the turn");
        assert_eq!(resolved.mode, AgentMode::ReadOnly);
        assert!(resolved.preset.plan_gate);
        assert_eq!(resolved.source.label(), "org");
    }

    #[test]
    fn org_agent_policy_a_person_cannot_loosen_the_cap() {
        let resolved = resolve_turn(
            ApprovalPosture::Auto,
            Some(AgentMode::Auto),
            Some(AgentMode::ReadOnly),
        )
        .expect("the cap binds the turn");
        assert_eq!(resolved.mode, AgentMode::ReadOnly);
    }

    #[test]
    fn org_agent_policy_no_cap_and_no_choice_leaves_the_turn_as_it_was() {
        assert_eq!(resolve_turn(ApprovalPosture::Auto, None, None), None);
        assert_eq!(
            resolve_turn(ApprovalPosture::Ask, None, Some(AgentMode::Auto)),
            None
        );
        assert_eq!(
            resolve_turn(ApprovalPosture::Auto, Some(AgentMode::ReadOnly), None),
            crate::profile::resolve_turn_mode(Some(AgentMode::ReadOnly), ApprovalPosture::Auto),
        );
    }

    #[test]
    fn org_agent_policy_lockdown_refuses_a_do_run() {
        let resolved = resolve_turn(ApprovalPosture::Auto, None, Some(AgentMode::Lockdown))
            .expect("the cap binds the turn");
        assert!(!resolved.preset.do_runs);
    }

    #[test]
    fn org_agent_policy_a_grant_is_held_to_the_cap_and_the_cap_states_for_no_grant() {
        use AutonomyRung::{DangerFullAccess, ReadOnly, WorkspaceWrite};
        assert_eq!(
            clamp_rung(Some(DangerFullAccess), Some(WorkspaceWrite)),
            Some(WorkspaceWrite)
        );
        assert_eq!(
            clamp_rung(Some(ReadOnly), Some(WorkspaceWrite)),
            Some(ReadOnly)
        );
        assert_eq!(clamp_rung(None, Some(WorkspaceWrite)), Some(WorkspaceWrite));
        assert_eq!(
            clamp_rung(Some(DangerFullAccess), None),
            Some(DangerFullAccess)
        );
        assert_eq!(clamp_rung(None, None), None);
    }

    #[test]
    fn org_agent_policy_an_enforced_org_the_cache_does_not_hold_is_held_to_read_only() {
        let cache = OrgPolicyCache::new();
        assert_eq!(
            dispatch_rung_cap(PolicyMode::Enforce, &cache, "org-unknown"),
            Some(AutonomyRung::ReadOnly)
        );
        assert_eq!(
            dispatch_rung_cap(PolicyMode::Off, &cache, "org-unknown"),
            None
        );
        assert_eq!(
            dispatch_rung_cap(PolicyMode::Shadow, &cache, "org-unknown"),
            None
        );
        cache.store(
            "org-held",
            OrgPolicy::from_settings(
                2,
                "sha256:2",
                &settings(json!({"rung_cap": "workspace_write"})),
            ),
            Instant::now(),
        );
        assert_eq!(
            dispatch_rung_cap(PolicyMode::Enforce, &cache, "org-held"),
            Some(AutonomyRung::WorkspaceWrite)
        );
        assert_eq!(held_rung_cap(PolicyMode::Shadow, &cache, "org-held"), None);
    }

    #[test]
    fn org_agent_policy_web_switches_are_read_from_the_document_and_fail_closed() {
        let default = OrgPolicy::from_settings(0, "", &settings(json!({})));
        assert_eq!(default.web, WebCaps { live: true, paid: true });
        let off = OrgPolicy::from_settings(
            1,
            "d",
            &settings(json!({"web.mode": "off", "web.paid_providers": "off"})),
        );
        assert_eq!(off.web, WebCaps { live: false, paid: false });
        // cached_only is reserved and never stored, and a value this build cannot read takes the fail-closed off.
        let reserved = OrgPolicy::from_settings(1, "d", &settings(json!({"web.mode": "cached_only"})));
        assert!(!reserved.web.live);
        let unreadable = OrgPolicy::from_settings(
            1,
            "d",
            &settings(json!({"web.mode": 3, "web.paid_providers": "maybe"})),
        );
        assert_eq!(unreadable.web, WebCaps::FAIL_CLOSED);
    }

    #[test]
    fn org_agent_policy_an_off_web_switch_makes_the_policy_restrictive() {
        let off = OrgPolicy::from_settings(2, "sha256:2", &settings(json!({"web.mode": "off"})));
        assert!(off.restrictive, "a stale answer that withholds web access keeps the long bound");
        assert!(!OrgPolicy::from_settings(0, "", &Map::new()).restrictive);
    }

    #[test]
    fn org_agent_policy_web_switches_bind_inline_turns_only_under_enforcement_and_when_held() {
        let cache = OrgPolicyCache::new();
        cache.store(
            "org-web-off",
            OrgPolicy::from_settings(3, "sha256:3", &settings(json!({"web.mode": "off"}))),
            Instant::now(),
        );
        assert_eq!(
            held_web_caps(PolicyMode::Enforce, &cache, "org-web-off"),
            Some(WebCaps { live: false, paid: true })
        );
        assert_eq!(held_web_caps(PolicyMode::Shadow, &cache, "org-web-off"), None);
        assert_eq!(held_web_caps(PolicyMode::Off, &cache, "org-web-off"), None);
        assert_eq!(
            held_web_caps(PolicyMode::Enforce, &cache, "org-web-unknown"),
            None,
            "an unread policy never turns web access off for an inline turn"
        );
    }

    #[test]
    fn org_agent_policy_a_do_run_carries_the_web_switches_it_was_admitted_under() {
        let cache = OrgPolicyCache::new();
        cache.store(
            "org-web-run",
            OrgPolicy::from_settings(4, "sha256:4", &settings(json!({"web.mode": "off"}))),
            Instant::now(),
        );
        let mut context = None;
        stamp_web_policy(PolicyMode::Enforce, &cache, "org-web-run", &mut context);
        let web = context
            .as_ref()
            .and_then(|context| context.web.as_ref())
            .expect("the run carries its web switches");
        assert_eq!((web.mode.as_str(), web.paid_providers, web.revision), ("off", true, 4));

        cache.store(
            "org-web-live",
            OrgPolicy::from_settings(5, "sha256:5", &Map::new()),
            Instant::now(),
        );
        let mut live = None;
        stamp_web_policy(PolicyMode::Enforce, &cache, "org-web-live", &mut live);
        let web = live
            .as_ref()
            .and_then(|context| context.web.as_ref())
            .expect("the run carries its web switches");
        assert_eq!(web.mode, "live");

        // An enforced org the cache does not hold is held to the fail-closed switches at revision zero.
        let mut unknown = None;
        stamp_web_policy(PolicyMode::Enforce, &cache, "org-web-unknown", &mut unknown);
        let web = unknown
            .as_ref()
            .and_then(|context| context.web.as_ref())
            .expect("the run is held to the fail-closed switches");
        assert_eq!((web.mode.as_str(), web.paid_providers, web.revision), ("off", false, 0));

        // Off and shadow stamp nothing, so the request is exactly as it was.
        let mut shadow = None;
        stamp_web_policy(PolicyMode::Shadow, &cache, "org-web-run", &mut shadow);
        assert!(shadow.is_none());
    }

    #[test]
    fn org_agent_policy_attribution_carries_the_revision_without_hook_rules() {
        let mut context = None;
        stamp_policy_revision(&mut context, Some((7, "sha256:7".to_owned())));
        let context = context.expect("the run is attributed");
        assert_eq!(context.policy_revision, 7);
        assert_eq!(context.policy_digest, "sha256:7");
        assert!(context.hook_context.is_empty());
    }

    #[tokio::test]
    async fn org_agent_policy_a_restrictive_answer_outlives_a_failed_refresh_for_900_seconds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(document(
                "org-a",
                3,
                json!({"mode_cap": "read_only"}),
            )))
            .mount(&server)
            .await;
        let cache = OrgPolicyCache::new();
        let client = reqwest::Client::new();
        let t0 = Instant::now();
        let first = ensure_at(&cache, &client, &server.uri(), "org-a", "token", t0)
            .await
            .expect("the registry answers");
        assert_eq!(first.revision, 3);

        server.reset().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let inside = ensure_at(
            &cache,
            &client,
            &server.uri(),
            "org-a",
            "token",
            t0 + Duration::from_secs(600),
        )
        .await
        .expect("the last answer is inside its 900 s bound");
        assert_eq!(inside.revision, 3);
        assert!(
            ensure_at(
                &cache,
                &client,
                &server.uri(),
                "org-a",
                "token",
                t0 + Duration::from_secs(901)
            )
            .await
            .is_err(),
            "past the bound the org's policy is unknown"
        );
        assert!(
            cache.policy("org-a").is_none(),
            "the expired entry is dropped"
        );
    }

    #[tokio::test]
    async fn org_agent_policy_a_web_off_answer_is_held_for_the_restrictive_bound() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(document(
                "org-web-stale",
                6,
                json!({"web.mode": "off"}),
            )))
            .mount(&server)
            .await;
        let cache = OrgPolicyCache::new();
        let client = reqwest::Client::new();
        let t0 = Instant::now();
        ensure_at(&cache, &client, &server.uri(), "org-web-stale", "token", t0)
            .await
            .expect("the registry answers");

        server.reset().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        // Past the 30 s TTL, a refresh that fails keeps web access off for the restrictive bound: a registry
        // outage cannot quietly turn an org's web access back on.
        let held = ensure_at(
            &cache,
            &client,
            &server.uri(),
            "org-web-stale",
            "token",
            t0 + Duration::from_secs(600),
        )
        .await
        .expect("the restrictive answer is inside its 900 s bound");
        assert!(!held.web.live);
    }

    #[tokio::test]
    async fn org_agent_policy_a_permissive_answer_is_served_for_120_seconds_only() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(document("org-b", 0, json!({}))))
            .mount(&server)
            .await;
        let cache = OrgPolicyCache::new();
        let client = reqwest::Client::new();
        let t0 = Instant::now();
        ensure_at(&cache, &client, &server.uri(), "org-b", "token", t0)
            .await
            .expect("the registry answers");

        server.reset().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(ensure_at(
            &cache,
            &client,
            &server.uri(),
            "org-b",
            "token",
            t0 + Duration::from_secs(100)
        )
        .await
        .is_ok());
        assert!(ensure_at(
            &cache,
            &client,
            &server.uri(),
            "org-b",
            "token",
            t0 + Duration::from_secs(121)
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn org_agent_policy_a_never_seen_org_with_the_registry_down_refuses_a_do_run_only() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let cache = OrgPolicyCache::new();
        let client = reqwest::Client::new();
        assert_eq!(
            admit_turn(
                PolicyMode::Enforce,
                &cache,
                &client,
                &server.uri(),
                "org-new",
                "token",
                true
            )
            .await,
            Err(POLICY_UNAVAILABLE_CODE)
        );
        assert_eq!(
            admit_turn(
                PolicyMode::Enforce,
                &cache,
                &client,
                &server.uri(),
                "org-new",
                "token",
                false
            )
            .await,
            Ok(None),
            "a plain turn keeps the floor it had"
        );
    }

    #[tokio::test]
    async fn org_agent_policy_off_reads_nothing_and_shadow_applies_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(document(
                "org-c",
                5,
                json!({"mode_cap": "lockdown"}),
            )))
            .mount(&server)
            .await;
        let cache = OrgPolicyCache::new();
        let client = reqwest::Client::new();
        assert_eq!(
            admit_turn(
                PolicyMode::Off,
                &cache,
                &client,
                &server.uri(),
                "org-c",
                "token",
                true
            )
            .await,
            Ok(None)
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty(),
            "off makes no call to the registry"
        );
        assert_eq!(
            admit_turn(
                PolicyMode::Shadow,
                &cache,
                &client,
                &server.uri(),
                "org-c",
                "token",
                true
            )
            .await,
            Ok(None),
            "shadow observes the cap and applies none"
        );
        assert!(
            cache.policy("org-c").is_some(),
            "shadow still reads the policy"
        );
    }

    #[tokio::test]
    async fn org_agent_policy_a_refused_bearer_is_not_a_registry_outage() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(ORG_AGENT_POLICY_PATH))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let cache = OrgPolicyCache::new();
        let client = reqwest::Client::new();
        let refused = ensure_at(
            &cache,
            &client,
            &server.uri(),
            "org-d",
            "bad-token",
            Instant::now(),
        )
        .await;
        assert!(refused.is_err());
        assert!(
            cache.policy("org-d").is_none(),
            "nothing is held for a refused caller"
        );
    }
}
