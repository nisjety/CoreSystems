//! The enforcement point of model entitlement (ADR-0005, WS10-S3).
//!
//! org-core stores and resolves which models an organisation, and each role in
//! it, may use. inference-core is the only Model Plane service that reads that
//! answer and the only one that applies it: every chat call, the catalogue and
//! the provider chain's fallback all ask the same [`EntitlementGate`], so there
//! is one cache, one fail-closed rule and no window in which two services
//! disagree. model-gateway asks through the `GetModelAccess` RPC.
//!
//! # The answer and how long it is held
//!
//! A caller's answer is fetched from org-core with this service's own
//! credential, held for [`TTL`], and refreshed on the next call after that.
//! Concurrent calls for one caller wait on one fetch (single flight). When a
//! refresh fails, the last answer keeps serving inside a bound that depends on
//! what it said: [`STALE_RESTRICTIVE`] when it restricted something,
//! [`STALE_UNRESTRICTED`] when it did not, so an outage neither undoes a
//! revocation nor hides one behind a long-lived open answer. Past the bound, or
//! with no answer ever seen, the call is refused (`model_entitlement_unavailable`):
//! under `enforce` an unresolved entitlement never falls back to unrestricted,
//! whatever the caller's zero-retention posture.
//!
//! # Modes
//!
//! `off` resolves nothing and makes no call. `shadow` resolves and counts what
//! `enforce` would have refused, and never refuses or filters. `enforce` applies
//! the answer. A service principal (no user) is exempt in every mode and counted
//! as `service_exempt`: platform jobs must not break on an organisation's list.
//!
//! # Zero retention
//!
//! The answer is model ids and actor ids, never content. It lives in memory only,
//! keyed by organisation and user, and is lost on restart.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use mp_contracts::model_entitlement::{
    after_failed_refresh, Availability, LastAnswer, Mode, Reason, ResolvedAccess, STALE_RESTRICTIVE,
    STALE_UNRESTRICTED, TTL,
};
use tracing::{info, warn};

use crate::auth::AuthenticatedPrincipal;
use crate::config::{EntitlementConfig, ORG_CORE_SERVICE_ID};
use crate::provider::{Bearer, PrivacyTier, ProviderError};

/// Counter: entitlement decisions by what was decided and the mode it was
/// decided under. No organisation, user or model label: the series stay few.
pub const MODEL_ENTITLEMENT_TOTAL: &str = "model_entitlement_total";

/// The whole budget of one org-core call: connect, send and read.
const FETCH_TIMEOUT: Duration = Duration::from_secs(1);

/// After a failed fetch, calls for the same caller reuse the failure for this
/// long instead of each waiting out its own timeout behind the lock.
const RETRY_BACKOFF: Duration = Duration::from_secs(1);

/// How many callers' answers are held. Past it the least recently used go.
const MAX_ENTRIES: usize = 10_000;

/// The size a full cache is evicted down to, so the scan is amortised.
const EVICT_TO: usize = MAX_ENTRIES * 9 / 10;

fn count(decision: &'static str, mode: Mode) {
    ::metrics::counter!(
        MODEL_ENTITLEMENT_TOTAL,
        "decision" => decision,
        "mode" => mode.as_str(),
    )
    .increment(1);
}

fn describe() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        ::metrics::describe_counter!(
            MODEL_ENTITLEMENT_TOTAL,
            "Model entitlement decisions by decision (resolved, stale, unavailable, would_unavailable, refuse, would_refuse, service_exempt) and mode (shadow, enforce)."
        );
    });
}

/// The source of the clock the cache measures age by. A function so a test can
/// move time without sleeping.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// What the gate decided for one caller.
#[derive(Debug, Clone)]
pub enum Resolved {
    /// The mode is `off`: nothing was resolved.
    Off,
    /// A service principal: exempt from the policy.
    Exempt,
    /// `shadow` mode could not read the answer. Nothing is applied; the miss
    /// was counted.
    Unreadable,
    /// The answer to apply (`enforce`) or to count against (`shadow`).
    Access(Arc<RequestAccess>),
}

/// One call's view of the policy. Built by the handler, carried on the provider
/// request and read by the chain at every point that could pick or serve a
/// model.
#[derive(Debug)]
pub struct RequestAccess {
    resolved: Arc<ResolvedAccess>,
    mode: Mode,
    org_id: String,
    // A would-be refusal is counted and logged once per call, however many
    // candidates, rungs and cache reads it touches.
    noted: AtomicBool,
}

impl RequestAccess {
    /// An answer to apply under `mode` (`shadow` or `enforce`).
    #[must_use]
    pub fn new(resolved: Arc<ResolvedAccess>, mode: Mode, org_id: impl Into<String>) -> Self {
        Self {
            resolved,
            mode,
            org_id: org_id.into(),
            noted: AtomicBool::new(false),
        }
    }

    /// What org-core resolved.
    #[must_use]
    pub fn resolved(&self) -> &ResolvedAccess {
        &self.resolved
    }

    /// Whether the policy is applied: the mode is `enforce`.
    #[must_use]
    pub fn enforced(&self) -> bool {
        self.mode == Mode::Enforce
    }

    /// The policy's verdict on `model` served by `provider`, whatever the mode.
    /// No side effects: for catalogue and diagnostic reads.
    ///
    /// # Errors
    /// The [`Reason`] the policy does not permit it.
    pub fn check(&self, model: &str, provider: &str) -> Result<(), Reason> {
        self.resolved.permits(model, provider)
    }

    /// The reason to skip or refuse `model` served by `provider`, applying the
    /// mode: under `enforce` the policy's verdict, under `shadow` never, though
    /// the first verdict that would have refused is counted and logged.
    #[must_use]
    pub fn refuses(&self, model: &str, provider: &str) -> Option<Reason> {
        let reason = self.check(model, provider).err()?;
        if self.enforced() {
            return Some(reason);
        }
        if !self.noted.swap(true, Ordering::Relaxed) {
            count("would_refuse", self.mode);
            info!(
                org_id = %self.org_id,
                revision = self.resolved.revision,
                model = %model,
                reason = reason.as_str(),
                "model entitlement would refuse this call; shadow mode serves it"
            );
        }
        None
    }

    /// The error that ends a call whose model, or whose every route, the policy
    /// refused. `model` is empty for an automatic request. Counted.
    #[must_use]
    pub fn refusal(&self, model: &str, reason: Reason) -> ProviderError {
        count("refuse", self.mode);
        info!(
            org_id = %self.org_id,
            revision = self.resolved.revision,
            reason = reason.as_str(),
            "model entitlement refused the call"
        );
        ProviderError::ModelNotEntitled {
            model: model.to_owned(),
            reason,
        }
    }

    /// The weakest privacy tier the policy lets a call be served at, when it is
    /// applied.
    #[must_use]
    pub fn min_privacy_tier(&self) -> PrivacyTier {
        if !self.enforced() {
            return PrivacyTier::Unspecified;
        }
        self.resolved
            .policy
            .as_ref()
            .map_or(PrivacyTier::Unspecified, |policy| {
                tier_from_label(&policy.min_privacy_tier)
            })
    }
}

/// The privacy tier a policy's label names; no label is no minimum. A label this
/// build does not know reads as the strongest tier, so a newer policy narrows
/// service rather than opening it.
#[must_use]
pub fn tier_from_label(label: &str) -> PrivacyTier {
    match label {
        "" => PrivacyTier::Unspecified,
        "global" => PrivacyTier::Global,
        "eu_resident" => PrivacyTier::EuResident,
        "zdr_contractual" => PrivacyTier::ZdrContractual,
        _ => PrivacyTier::Sovereign,
    }
}

/// Why an answer could not be read from org-core. Logged, never shown.
#[derive(Debug)]
struct FetchError(String);

/// The org-core machine route, `GET /internal/orgs/{org}/model-entitlement`.
struct OrgCoreSource {
    http: reqwest::Client,
    base: reqwest::Url,
    token: Bearer,
}

impl OrgCoreSource {
    fn new(base_url: &str, token: Bearer) -> Option<Self> {
        let mut base = reqwest::Url::parse(base_url.trim()).ok()?;
        if base.cannot_be_a_base() {
            return None;
        }
        base.set_query(None);
        let http = reqwest::Client::builder()
            // The whole call, DNS and connect included. The first call of a
            // burst pays a cold connection out of this same second, so idle
            // connections are held long (as the budget client's are).
            .timeout(FETCH_TIMEOUT)
            .pool_idle_timeout(Duration::from_secs(600))
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .ok()?;
        Some(Self { http, base, token })
    }

    async fn fetch(&self, org_id: &str, user_id: &str) -> Result<ResolvedAccess, FetchError> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|()| FetchError("the org-core URL cannot carry a path".to_owned()))?
            .pop_if_empty()
            .extend(["internal", "orgs", org_id, "model-entitlement"]);
        url.query_pairs_mut().append_pair("user_id", user_id);
        let response = self
            .http
            .get(url)
            .header("accept", "application/json")
            .header("x-service-id", ORG_CORE_SERVICE_ID)
            .header("x-service-token", self.token.as_str())
            .send()
            .await
            .map_err(|error| {
                FetchError(format!(
                    "request failed (timeout: {}, connect: {})",
                    error.is_timeout(),
                    error.is_connect()
                ))
            })?;
        if !response.status().is_success() {
            return Err(FetchError(format!("status {}", response.status())));
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|_| FetchError("the body is not JSON".to_owned()))?;
        ResolvedAccess::from_body(&body)
            .ok_or_else(|| FetchError("the body is not an entitlement".to_owned()))
    }
}

struct Cached {
    access: Arc<ResolvedAccess>,
    at: Instant,
}

#[derive(Default)]
struct Slot {
    cached: Option<Cached>,
    failed_at: Option<Instant>,
}

/// One caller's slot. Its lock is the single flight: a second call for the same
/// caller waits here for the first fetch instead of starting another.
struct Entry {
    last_used_ms: AtomicU64,
    slot: tokio::sync::Mutex<Slot>,
}

type Key = (String, String);

/// Resolves, caches and applies the organisation's model entitlement.
pub struct EntitlementGate {
    mode: Mode,
    source: Option<OrgCoreSource>,
    entries: DashMap<Key, Arc<Entry>>,
    clock: Clock,
    started: Instant,
}

impl EntitlementGate {
    /// A gate for `config`, measuring age by the system clock.
    #[must_use]
    pub fn new(config: &EntitlementConfig) -> Self {
        Self::with_clock(config, Arc::new(Instant::now))
    }

    /// A gate that is off: nothing is resolved and no call is made.
    #[must_use]
    pub fn off() -> Self {
        Self::new(&EntitlementConfig::off())
    }

    /// A gate measuring age by `clock`. For tests that move time.
    #[must_use]
    pub fn with_clock(config: &EntitlementConfig, clock: Clock) -> Self {
        let source = match (&config.org_core_url, config.service_token.is_empty()) {
            (Some(url), false) => OrgCoreSource::new(url, config.service_token.clone()),
            _ => None,
        };
        if config.mode != Mode::Off {
            describe();
            if source.is_none() {
                warn!(
                    mode = config.mode.as_str(),
                    "model entitlement is on but ORG_CORE_URL or INFERENCE_ORG_CORE_SERVICE_TOKEN is \
                     not usable: no answer can be read, so enforce refuses every user call"
                );
            }
        }
        let started = clock();
        Self {
            mode: config.mode,
            source,
            entries: DashMap::new(),
            clock,
            started,
        }
    }

    /// The mode the gate was built with.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// What applies to the caller of a chat call.
    ///
    /// # Errors
    /// [`ProviderError::EntitlementUnavailable`], under `enforce` only, when no
    /// answer can be read or held within its bound.
    pub async fn resolve(&self, principal: &AuthenticatedPrincipal) -> Result<Resolved, ProviderError> {
        self.resolve_for(&principal.org_id, principal.entitlement_user_id())
            .await
    }

    /// [`Self::resolve`] for a caller named by organisation and user. `None` is a
    /// service principal.
    ///
    /// # Errors
    /// As [`Self::resolve`].
    pub async fn resolve_for(&self, org_id: &str, user_id: Option<&str>) -> Result<Resolved, ProviderError> {
        if self.mode == Mode::Off {
            return Ok(Resolved::Off);
        }
        let Some(user_id) = user_id else {
            count("service_exempt", self.mode);
            return Ok(Resolved::Exempt);
        };
        match self.answer(org_id, user_id).await {
            Some(resolved) => {
                count("resolved", self.mode);
                Ok(Resolved::Access(Arc::new(RequestAccess::new(
                    resolved,
                    self.mode,
                    org_id,
                ))))
            }
            None if self.mode == Mode::Enforce => {
                count("unavailable", self.mode);
                Err(ProviderError::EntitlementUnavailable)
            }
            None => {
                count("would_unavailable", self.mode);
                Ok(Resolved::Unreadable)
            }
        }
    }

    /// The access to carry on a provider request, or `None` when nothing is
    /// applied (off, a service caller, an unreadable answer in shadow).
    ///
    /// # Errors
    /// As [`Self::resolve`].
    pub async fn access_for(
        &self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<Option<Arc<RequestAccess>>, ProviderError> {
        Ok(match self.resolve(principal).await? {
            Resolved::Access(access) => Some(access),
            Resolved::Off | Resolved::Exempt | Resolved::Unreadable => None,
        })
    }

    fn now(&self) -> Instant {
        (self.clock)()
    }

    fn entry(&self, key: Key) -> Arc<Entry> {
        let now_ms = self.stamp();
        if let Some(entry) = self.entries.get(&key) {
            entry.last_used_ms.store(now_ms, Ordering::Relaxed);
            return Arc::clone(&entry);
        }
        if self.entries.len() >= MAX_ENTRIES {
            self.evict_least_recently_used();
        }
        let entry = self.entries.entry(key).or_insert_with(|| {
            Arc::new(Entry {
                last_used_ms: AtomicU64::new(now_ms),
                slot: tokio::sync::Mutex::new(Slot::default()),
            })
        });
        Arc::clone(&entry)
    }

    fn stamp(&self) -> u64 {
        u64::try_from(self.now().saturating_duration_since(self.started).as_millis())
            .unwrap_or(u64::MAX)
    }

    /// Drop the least recently used callers down to [`EVICT_TO`]. One O(n)
    /// snapshot, amortised over the headroom it frees. A caller dropped while a
    /// call waits on its slot loses nothing: the waiter holds the entry.
    fn evict_least_recently_used(&self) {
        let mut stamps: Vec<(Key, u64)> = self
            .entries
            .iter()
            .map(|entry| (entry.key().clone(), entry.last_used_ms.load(Ordering::Relaxed)))
            .collect();
        if stamps.len() <= EVICT_TO {
            return;
        }
        stamps.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        for (key, _) in stamps.into_iter().skip(EVICT_TO) {
            self.entries.remove(&key);
        }
    }

    /// The caller's answer: a fresh one, one still inside its TTL, or the last
    /// one when a refresh failed inside its stale bound. `None` is unreadable.
    async fn answer(&self, org_id: &str, user_id: &str) -> Option<Arc<ResolvedAccess>> {
        let entry = self.entry((org_id.to_owned(), user_id.to_owned()));
        let mut slot = entry.slot.lock().await;
        let now = self.now();
        if let Some(cached) = &slot.cached {
            if now.saturating_duration_since(cached.at) < TTL {
                return Some(Arc::clone(&cached.access));
            }
        }
        let backing_off = slot
            .failed_at
            .is_some_and(|failed| now.saturating_duration_since(failed) < RETRY_BACKOFF);
        if !backing_off {
            let Some(source) = &self.source else {
                return self.after_failure(&slot, now, org_id);
            };
            match source.fetch(org_id, user_id).await {
                Ok(access) => {
                    let access = Arc::new(access);
                    slot.cached = Some(Cached {
                        access: Arc::clone(&access),
                        at: self.now(),
                    });
                    slot.failed_at = None;
                    return Some(access);
                }
                Err(FetchError(why)) => {
                    warn!(org_id = %org_id, why = %why, "model entitlement could not be read from org-core");
                    slot.failed_at = Some(self.now());
                }
            }
        }
        self.after_failure(&slot, now, org_id)
    }

    /// The last answer when it is inside its stale bound, else `None`.
    fn after_failure(&self, slot: &Slot, now: Instant, org_id: &str) -> Option<Arc<ResolvedAccess>> {
        let cached = slot.cached.as_ref()?;
        let age = now.saturating_duration_since(cached.at);
        let last = LastAnswer {
            restricted: cached.access.restricted(),
            age,
        };
        match after_failed_refresh(Some(last)) {
            Availability::UseLastAnswer => {
                count("stale", self.mode);
                warn!(
                    org_id = %org_id,
                    age_secs = age.as_secs(),
                    bound_secs = if last.restricted {
                        STALE_RESTRICTIVE.as_secs()
                    } else {
                        STALE_UNRESTRICTED.as_secs()
                    },
                    "serving the last model entitlement answer: org-core could not be read"
                );
                Some(Arc::clone(&cached.access))
            }
            Availability::Refuse => None,
        }
    }
}
