//! Run control across gateway replicas: a timeout-bounded request/reply to the replica that
//! owns a live chat stream (build plan WS04-S1).
//!
//! # Why this exists
//!
//! The cancel registry, the queued-input registry and the stream/turn records are per process:
//! the SSE connection pins a run to one replica, so a Stop or a queued message that lands on the
//! other replica answers 404 and the producer runs to the end. The fix is to ask the owner. This
//! module is the seam (a small message to whichever replica subscribed for a request id or a run
//! id, and the answer back) and the three things an owner does with it: stop the stream, take a
//! queued message into its [`QueuedInputRegistry`], and say whether it is still producing.
//!
//! # Contract
//!
//! - Core NATS request/reply, at-most-once. A lost message surfaces as [`RunControlError::Timeout`]
//!   or [`RunControlError::NoOwner`]; nothing retries on its own, so a caller never sees a success
//!   it did not get. The caller maps those to a retryable 503 or to the existing 404.
//! - Ids only on the wire, never a bearer: [`RunControlMsg`] carries the identity the owner
//!   scopes the entry by (org, user) plus ids and, for a queue, the user's text. The text is put
//!   on the bus only by the queue op, after its caller has passed the ZDR gate; this module does
//!   not know what a ZDR turn is and never sees one.
//! - Subjects are built from validated tokens ([`subject_token`]) so an id cannot rewrite the
//!   subject it is addressed to.
//!
//! # Transports
//!
//! [`Transport::Off`] (the default) refuses everything, so a gateway with the flag off behaves
//! exactly as before. [`Transport::Loopback`] is an in-process hub with the same wire encoding,
//! for the two-replica tests and for a dev stack without NATS. [`Transport::Nats`] is the real
//! bus; it is checked once at boot ([`RunControl::self_tested`]) and a failed check degrades to
//! off, so a flag turned on before the NATS ACL grants the subjects costs nothing but the
//! feature.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::cancel_registry::CancelRegistry;
use crate::gateway_metrics::{self, RunControlOutcome};
use crate::queued_input::{EnqueueOutcome, QueuedInputRegistry};

/// The switch that turns the bus on. Default off; only `true` enables it.
pub const BUS_ENV: &str = "MODEL_GATEWAY_RUN_CONTROL_BUS";

/// How long a request waits for the owner before it is a timeout.
pub const TIMEOUT_ENV: &str = "MODEL_GATEWAY_RUN_CONTROL_TIMEOUT_MS";

const DEFAULT_TIMEOUT_MS: u64 = 750;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 5_000;

/// Largest message or reply either side will build or accept. A queued message is capped at
/// `queued_input::MAX_QUEUED_CHARS` characters, so this leaves room for the envelope.
pub const MAX_PAYLOAD_BYTES: usize = 8 * 1024;

const MAX_TOKEN_LEN: usize = 64;

/// Bumped only for a change an older gateway would misread; an unknown version is dropped.
const PROTOCOL_VERSION: u8 = 1;

/// The strict parser of [`BUS_ENV`]: `true` in any case with surrounding whitespace, and
/// nothing else, so a typo never turns the bus on.
#[must_use]
pub fn enabled_from(value: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(value)
}

/// The request timeout from [`TIMEOUT_ENV`]: the default when it is unset or not a number,
/// otherwise clamped to 100-5000 ms so neither a typo nor a copy-paste can make a Stop hang or
/// give up instantly.
#[must_use]
pub fn timeout_from(value: Option<&str>) -> Duration {
    let millis = value
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .map_or(DEFAULT_TIMEOUT_MS, |ms| {
            ms.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS)
        });
    Duration::from_millis(millis)
}

/// `raw` when it is usable as one NATS subject token: 1-64 characters of `[A-Za-z0-9_-]`. A `.`
/// would add a token, `*` and `>` would turn the subject into a wildcard, and whitespace ends
/// it, so an id carrying any of them must never reach a subject.
#[must_use]
pub fn subject_token(raw: &str) -> Option<&str> {
    let usable = !raw.is_empty()
        && raw.len() <= MAX_TOKEN_LEN
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    usable.then_some(raw)
}

/// Whether `subject` is matched by the NATS subscription `pattern` (`*` is one token, a trailing
/// `>` is one or more).
#[must_use]
pub fn subject_matches(pattern: &str, subject: &str) -> bool {
    let mut pattern_tokens = pattern.split('.');
    let mut subject_tokens = subject.split('.');
    loop {
        match (pattern_tokens.next(), subject_tokens.next()) {
            (Some(">"), Some(_)) => return pattern_tokens.next().is_none(),
            (Some("*"), Some(_)) => {}
            (Some(p), Some(s)) if p == s => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// What a run-control request asks the owner to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunControlOp {
    Cancel,
    Queue,
    Probe,
}

impl RunControlOp {
    /// The token the op occupies in a request subject, and the `op` metric label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::Queue => "queue",
            Self::Probe => "probe",
        }
    }
}

/// One request to the replica that owns a stream or a run. No bearer, token or key has a place
/// here (`run_control_message_has_no_credential_field` pins the field set).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunControlMsg {
    pub v: u8,
    pub op: RunControlOp,
    /// The caller's identity, so the owner answers "not yours" exactly as it answers "not
    /// here" and a request id is no oracle for another tenant's run.
    pub org_id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run_id: String,
    /// The caller's id for this attempt, so an owner can drop a repeat of a message whose reply
    /// was lost.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub op_id: String,
    /// The queued message. Present only on a queue op, and only after the ZDR gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_thread_id: Option<String>,
}

impl RunControlMsg {
    #[must_use]
    pub fn new(op: RunControlOp, org_id: &str, user_id: &str) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            op,
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            request_id: String::new(),
            run_id: String::new(),
            op_id: String::new(),
            text: None,
            claimed_thread_id: None,
        }
    }
}

/// The owner's answer. `found` is false when the owner holds no matching entry for this caller;
/// `outcome` is an op-specific token (a queue outcome, for one) that the op's handler and its
/// caller agree on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunControlReply {
    pub v: u8,
    pub found: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// Whether the stream is conversation-only, as the owner's probe answer states it, so a message
    /// sent to another replica is stored under the scope its thread requires, with no guess. Absent
    /// from every other reply, and from a probe answer an older owner sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_only: Option<bool>,
}

impl RunControlReply {
    #[must_use]
    pub fn found() -> Self {
        Self {
            v: PROTOCOL_VERSION,
            found: true,
            thread_id: None,
            outcome: None,
            conversation_only: None,
        }
    }

    #[must_use]
    pub fn not_found() -> Self {
        Self {
            found: false,
            ..Self::found()
        }
    }
}

/// Why a request was not answered by an owner.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunControlError {
    #[error("the run-control bus is off")]
    Disabled,
    #[error("the id is not a valid run-control subject token, or the subject is a wildcard")]
    InvalidSubject,
    #[error("the run-control message is larger than {MAX_PAYLOAD_BYTES} bytes")]
    PayloadTooLarge,
    /// Nothing is subscribed for the stream: it ended, or it never ran on any replica.
    #[error("no replica owns the stream")]
    NoOwner,
    /// A subscriber exists but did not answer within the request timeout.
    #[error("the owning replica did not answer in time")]
    Timeout,
    #[error("the owning replica's reply was malformed")]
    BadReply,
    #[error("the run-control bus failed: {0}")]
    Unavailable(String),
}

/// A subject a run-control message is addressed to, or that an owner listens on. Built only from
/// validated tokens, so an id cannot change which subject it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunControlSubject {
    subject: String,
    wildcard: bool,
}

impl RunControlSubject {
    /// `mp.v1.runctl.req.{request_id}.{op}`: one operation on the stream `request_id`.
    ///
    /// # Errors
    /// [`RunControlError::InvalidSubject`] when `request_id` is not a subject token.
    pub fn request(request_id: &str, op: RunControlOp) -> Result<Self, RunControlError> {
        let token = subject_token(request_id).ok_or(RunControlError::InvalidSubject)?;
        Ok(Self {
            subject: mp_events::subjects::runctl_request_subject(token, op.as_str()),
            wildcard: false,
        })
    }

    /// `mp.v1.runctl.req.{request_id}.*`: every operation on the stream, for its owner to
    /// listen on. Not a subject a request can be sent to.
    ///
    /// # Errors
    /// [`RunControlError::InvalidSubject`] when `request_id` is not a subject token.
    pub fn request_any_op(request_id: &str) -> Result<Self, RunControlError> {
        let token = subject_token(request_id).ok_or(RunControlError::InvalidSubject)?;
        Ok(Self {
            subject: mp_events::subjects::runctl_request_wildcard(token),
            wildcard: true,
        })
    }

    /// `mp.v1.runctl.run.{run_id}.cancel`: a durable cancel of the run, for whichever replica
    /// is producing it.
    ///
    /// # Errors
    /// [`RunControlError::InvalidSubject`] when `run_id` is not a subject token.
    pub fn run_cancel(run_id: &str) -> Result<Self, RunControlError> {
        let token = subject_token(run_id).ok_or(RunControlError::InvalidSubject)?;
        Ok(Self {
            subject: mp_events::subjects::runctl_run_cancel_subject(token),
            wildcard: false,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.subject
    }
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, RunControlError> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| RunControlError::Unavailable(e.to_string()))?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(RunControlError::PayloadTooLarge);
    }
    Ok(bytes)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return None;
    }
    serde_json::from_slice(bytes).ok()
}

fn decode_msg(bytes: &[u8]) -> Option<RunControlMsg> {
    decode::<RunControlMsg>(bytes).filter(|msg| msg.v == PROTOCOL_VERSION)
}

fn decode_reply(bytes: &[u8]) -> Result<RunControlReply, RunControlError> {
    decode::<RunControlReply>(bytes)
        .filter(|reply| reply.v == PROTOCOL_VERSION)
        .ok_or(RunControlError::BadReply)
}

/// What an owner does with a request. Synchronous on purpose: the registries it touches are
/// in-memory maps, and a handler that blocks would stall every later request to that stream.
type Handler = Arc<dyn Fn(RunControlMsg) -> RunControlReply + Send + Sync>;

struct LoopbackListener {
    id: u64,
    patterns: Vec<String>,
    handler: Handler,
}

#[derive(Default)]
struct HubState {
    next_id: u64,
    listeners: Vec<LoopbackListener>,
    muted: bool,
}

/// An in-process stand-in for the bus: every [`RunControl`] built over one hub is a "replica" of
/// the others. Messages cross it in their wire encoding, so a field that does not survive
/// serde fails here the way it would on NATS.
#[derive(Clone, Default)]
pub struct LoopbackHub {
    state: Arc<Mutex<HubState>>,
}

impl LoopbackHub {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// While muted, a request to a stream that has a listener is never answered, which is how a
    /// test stands in for an owner that died or hung after subscribing.
    pub fn mute(&self, muted: bool) {
        self.lock().muted = muted;
    }

    /// How many listeners are subscribed on the hub, for tests that prove a finished or
    /// ZDR stream left none.
    #[must_use]
    pub fn listener_count(&self) -> usize {
        self.lock().listeners.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn add(&self, patterns: Vec<String>, handler: Handler) -> u64 {
        let mut state = self.lock();
        state.next_id += 1;
        let id = state.next_id;
        state.listeners.push(LoopbackListener {
            id,
            patterns,
            handler,
        });
        id
    }

    fn remove(&self, id: u64) {
        self.lock().listeners.retain(|listener| listener.id != id);
    }

    async fn request(
        &self,
        subject: &str,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<RunControlReply, RunControlError> {
        let (handler, muted) = {
            let state = self.lock();
            let handler = state
                .listeners
                .iter()
                .find(|listener| {
                    listener
                        .patterns
                        .iter()
                        .any(|pattern| subject_matches(pattern, subject))
                })
                .map(|listener| listener.handler.clone());
            (handler, state.muted)
        };
        let Some(handler) = handler else {
            return Err(RunControlError::NoOwner);
        };
        if muted {
            tokio::time::sleep(timeout).await;
            return Err(RunControlError::Timeout);
        }
        let msg = decode_msg(payload).ok_or(RunControlError::BadReply)?;
        decode_reply(&encode(&handler(msg))?)
    }
}

/// How the bus carries a request.
#[derive(Clone)]
pub enum Transport {
    Off,
    Loopback(LoopbackHub),
    Nats(async_nats::Client),
}

/// What `/readyz` reports for the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunControlStatus {
    /// Not enabled; the gateway behaves as it did before run control existed.
    Off,
    Ok,
    /// Enabled, but the boot check failed (an ACL without the subjects, a broker that would not
    /// connect). It behaves as off, and `/readyz` says so.
    Degraded,
}

impl RunControlStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Ok => "ok",
            Self::Degraded => "degraded",
        }
    }
}

/// A subscription held for the stream's lifetime: dropping it stops the listening, so a stream
/// that ends leaves nothing subscribed.
#[must_use = "dropping the listener ends the subscription"]
pub struct RemoteListener {
    kind: ListenerKind,
}

enum ListenerKind {
    /// The bus is off; there is nothing to stop.
    Inert,
    Loopback {
        hub: LoopbackHub,
        id: u64,
    },
    /// Held for its `Drop`: the tasks are aborted with the listener.
    Nats {
        _tasks: ServingTasks,
    },
}

/// The tasks serving a NATS listener. Aborting one drops its subscription, which unsubscribes;
/// holding them in a type of their own means a `listen` that is cancelled half way (the attach
/// budget) cannot leave a subscribed task behind.
#[derive(Default)]
struct ServingTasks(Vec<JoinHandle<()>>);

impl Drop for ServingTasks {
    fn drop(&mut self) {
        self.0.iter().for_each(JoinHandle::abort);
    }
}

impl Drop for RemoteListener {
    fn drop(&mut self) {
        if let ListenerKind::Loopback { hub, id } = &self.kind {
            hub.remove(*id);
        }
    }
}

/// The gateway's handle on run control. Cheap to clone; one per [`crate::state::AppState`].
#[derive(Clone)]
pub struct RunControl {
    transport: Transport,
    timeout: Duration,
    status: RunControlStatus,
}

impl Default for RunControl {
    fn default() -> Self {
        Self::off()
    }
}

impl RunControl {
    /// The bus off: every request is refused as [`RunControlError::Disabled`] and every listen
    /// is a no-op.
    #[must_use]
    pub fn off() -> Self {
        Self {
            transport: Transport::Off,
            timeout: timeout_from(None),
            status: RunControlStatus::Off,
        }
    }

    pub(crate) fn degraded() -> Self {
        Self {
            status: RunControlStatus::Degraded,
            ..Self::off()
        }
    }

    /// Replicas over an in-process hub.
    #[must_use]
    pub fn loopback(hub: LoopbackHub, timeout: Duration) -> Self {
        Self {
            transport: Transport::Loopback(hub),
            timeout,
            status: RunControlStatus::Ok,
        }
    }

    /// Over a connected NATS client. Unchecked: [`self_tested`](Self::self_tested) proves the
    /// ACL lets it work.
    #[must_use]
    pub fn nats(client: async_nats::Client, timeout: Duration) -> Self {
        Self {
            transport: Transport::Nats(client),
            timeout,
            status: RunControlStatus::Ok,
        }
    }

    /// The boot wiring: off unless [`BUS_ENV`] is `true`. With a broker URL it connects its own
    /// client (the publisher's is private to it) and self-tests; without one, a dev stack with
    /// no NATS gets an in-process hub, which is the single replica it is. A broker that will
    /// not connect or fails its check leaves the bus degraded, never the gateway down.
    pub async fn from_env(nats_url: Option<&str>) -> Self {
        Self::configured(
            std::env::var(BUS_ENV).ok().as_deref(),
            std::env::var(TIMEOUT_ENV).ok().as_deref(),
            nats_url,
        )
        .await
    }

    async fn configured(bus: Option<&str>, timeout: Option<&str>, nats_url: Option<&str>) -> Self {
        if !enabled_from(bus) {
            return Self::off();
        }
        let timeout = timeout_from(timeout);
        let Some(url) = nats_url else {
            info!("run control: no NATS_URL, using the in-process hub");
            return Self::loopback(LoopbackHub::new(), timeout);
        };
        match crate::nats_connection::connect(url).await {
            Ok(client) => Self::nats(client, timeout).self_tested().await,
            Err(error) => {
                warn!(%error, "run control: NATS connect failed, the bus is degraded and behaves as off");
                Self::degraded()
            }
        }
    }

    /// This handle after a round trip over its own transport: it listens on a throwaway subject
    /// and requests it. A NATS user without the `mp.v1.runctl.>` publish and subscribe grants
    /// (or the inbox publish grant the reply needs) gets no answer, and the handle comes back
    /// degraded and off instead of failing every Stop later.
    pub async fn self_tested(self) -> Self {
        if matches!(self.transport, Transport::Off) {
            return self;
        }
        match self.self_test().await {
            Ok(()) => {
                info!("run control: bus self-test passed");
                self
            }
            Err(error) => {
                warn!(%error, "run control: bus self-test failed (is the runctl ACL deployed?), the bus is degraded and behaves as off");
                Self::degraded()
            }
        }
    }

    async fn self_test(&self) -> Result<(), RunControlError> {
        let token = format!("selftest-{:016x}", rand::random::<u64>());
        let _listener = self
            .listen(&[RunControlSubject::request_any_op(&token)?], |_| {
                RunControlReply::found()
            })
            .await?;
        let mut msg = RunControlMsg::new(RunControlOp::Probe, "selftest", "selftest");
        msg.request_id.clone_from(&token);
        let subject = RunControlSubject::request(&token, RunControlOp::Probe)?;
        let reply = self.send(&subject, &msg).await?;
        reply.found.then_some(()).ok_or(RunControlError::BadReply)
    }

    #[must_use]
    pub fn status(&self) -> RunControlStatus {
        self.status
    }

    /// Whether a request can reach the bus at all.
    #[must_use]
    pub fn enabled(&self) -> bool {
        !matches!(self.transport, Transport::Off)
    }

    /// Ask the replica listening on `subject`, and wait for it for the request timeout. Counts
    /// the request in `run_control_requests_total`.
    ///
    /// # Errors
    /// [`RunControlError`]; [`NoOwner`](RunControlError::NoOwner) and
    /// [`Timeout`](RunControlError::Timeout) are the answers a caller turns into a 404 and a
    /// retryable 503.
    pub async fn request(
        &self,
        subject: &RunControlSubject,
        msg: &RunControlMsg,
    ) -> Result<RunControlReply, RunControlError> {
        let result = self.send(subject, msg).await;
        let outcome = match &result {
            Ok(_) => Some(RunControlOutcome::Peer),
            Err(RunControlError::NoOwner) => Some(RunControlOutcome::NoOwner),
            Err(RunControlError::Timeout) => Some(RunControlOutcome::Timeout),
            Err(
                RunControlError::PayloadTooLarge
                | RunControlError::BadReply
                | RunControlError::Unavailable(_),
            ) => Some(RunControlOutcome::Error),
            // Nothing was sent, so there is no bus outcome to count.
            Err(RunControlError::Disabled | RunControlError::InvalidSubject) => None,
        };
        if let Some(outcome) = outcome {
            gateway_metrics::run_control_request(msg.op.as_str(), outcome);
        }
        result
    }

    async fn send(
        &self,
        subject: &RunControlSubject,
        msg: &RunControlMsg,
    ) -> Result<RunControlReply, RunControlError> {
        if subject.wildcard {
            return Err(RunControlError::InvalidSubject);
        }
        if !self.enabled() {
            return Err(RunControlError::Disabled);
        }
        let payload = encode(msg)?;
        match &self.transport {
            Transport::Off => Err(RunControlError::Disabled),
            Transport::Loopback(hub) => hub.request(&subject.subject, &payload, self.timeout).await,
            Transport::Nats(client) => {
                let request = async_nats::Request::new()
                    .payload(payload.into())
                    .timeout(Some(self.timeout));
                match client.send_request(subject.subject.clone(), request).await {
                    Ok(message) => decode_reply(&message.payload),
                    Err(error) => Err(match error.kind() {
                        async_nats::client::RequestErrorKind::TimedOut => RunControlError::Timeout,
                        async_nats::client::RequestErrorKind::NoResponders => {
                            RunControlError::NoOwner
                        }
                        _ => RunControlError::Unavailable(error.to_string()),
                    }),
                }
            }
        }
    }

    /// Answer requests addressed to `subjects` with `handler` until the returned listener is
    /// dropped. On NATS the subscriptions are on the server when this returns, so a caller that
    /// registers before it announces the stream cannot be asked too early. With the bus off it
    /// does nothing.
    ///
    /// # Errors
    /// [`RunControlError::Unavailable`] when the subscription cannot be made.
    pub async fn listen<F>(
        &self,
        subjects: &[RunControlSubject],
        handler: F,
    ) -> Result<RemoteListener, RunControlError>
    where
        F: Fn(RunControlMsg) -> RunControlReply + Send + Sync + 'static,
    {
        let handler: Handler = Arc::new(handler);
        let kind = match &self.transport {
            Transport::Off => ListenerKind::Inert,
            Transport::Loopback(hub) => ListenerKind::Loopback {
                id: hub.add(
                    subjects.iter().map(|s| s.subject.clone()).collect(),
                    handler,
                ),
                hub: hub.clone(),
            },
            Transport::Nats(client) => {
                let mut tasks = ServingTasks::default();
                for subject in subjects {
                    match client.subscribe(subject.subject.clone()).await {
                        Ok(subscription) => tasks.0.push(tokio::spawn(serve(
                            client.clone(),
                            subscription,
                            handler.clone(),
                        ))),
                        Err(error) => return Err(RunControlError::Unavailable(error.to_string())),
                    }
                }
                if let Err(error) = client.flush().await {
                    return Err(RunControlError::Unavailable(error.to_string()));
                }
                ListenerKind::Nats { _tasks: tasks }
            }
        };
        Ok(RemoteListener { kind })
    }
}

/// How long a stream waits for its run-control subscriptions before it goes on without them. A
/// slow broker costs a Stop from the other replica, never the user's turn.
const ATTACH_BUDGET: Duration = Duration::from_millis(250);

/// Where a queued message found the stream it was aimed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueRoute {
    /// This replica decided it (it owns the stream, or the message failed a bound that needs no
    /// owner).
    Local(EnqueueOutcome),
    /// Another replica owns the stream and decided it.
    Peer(EnqueueOutcome),
    /// No stream of this caller is live anywhere: the same answer for ended, never ran and
    /// somebody else's.
    NoOwner,
    /// The bus could not tell. The message may or may not have been queued, so the caller says
    /// so (a retryable 503) and persists nothing.
    Unavailable,
}

/// How long a probe answer is reused. The resume tail asks every 300 ms; a producer's liveness
/// does not need to be fresher than this to decide whether silence is a dead stream.
const PROBE_CACHE: Duration = Duration::from_secs(2);

/// Whether the producer of a stream is still running, for the resume tail: on this replica, or
/// (bus on) on another one, through a probe cached for [`PROBE_CACHE`]. Bound to the caller like
/// everything else here.
pub struct ProducerLiveness {
    cancels: CancelRegistry,
    control: RunControl,
    request_id: String,
    org_id: String,
    user_id: String,
    probed: Option<(tokio::time::Instant, bool)>,
}

impl ProducerLiveness {
    #[must_use]
    pub fn new(
        cancels: CancelRegistry,
        control: RunControl,
        request_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> Self {
        Self {
            cancels,
            control,
            request_id: request_id.to_owned(),
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            probed: None,
        }
    }

    /// The local registry answers first and costs nothing; only a stream that is not here is
    /// probed, and a probe answer (live or not) is reused for [`PROBE_CACHE`].
    pub async fn is_live(&mut self) -> bool {
        if self
            .cancels
            .is_live_for(&self.request_id, &self.org_id, &self.user_id)
        {
            return true;
        }
        if !self.control.enabled() {
            return false;
        }
        if let Some((at, live)) = self.probed {
            if at.elapsed() < PROBE_CACHE {
                return live;
            }
        }
        let live = self
            .control
            .probe(&self.request_id, &self.org_id, &self.user_id)
            .await;
        self.probed = Some((tokio::time::Instant::now(), live));
        live
    }
}

/// Where a Stop found the stream it was aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelRoute {
    /// This replica owns the stream and stopped it.
    Local,
    /// Another replica owns it and said it stopped it.
    Peer,
    /// No stream of this caller is live anywhere: it ended, never ran, or is somebody else's.
    /// The same answer for all three, so an id is no oracle.
    NoOwner,
    /// The bus could not tell: the owner did not answer in time or the bus failed. The caller
    /// must say so (a retryable 503), never claim a stop or a missing stream.
    Unavailable,
}

/// What the replica owning a stream does with a request: stop the stream by its request id or by
/// the id of the run it produces, take a queued message for it, or say it is still producing.
/// Every id that is not this stream's is "not here"; the registries themselves check the caller's
/// org and user, so a stranger gets the answer a missing stream gets.
fn stream_owner(
    cancels: CancelRegistry,
    queued_inputs: QueuedInputRegistry,
    request_id: String,
    run_id: String,
) -> impl Fn(RunControlMsg) -> RunControlReply + Send + Sync + 'static {
    move |msg| match msg.op {
        RunControlOp::Cancel => {
            let stopped = if !msg.request_id.is_empty() {
                msg.request_id == request_id
                    && cancels.cancel_for(&request_id, &msg.org_id, &msg.user_id)
            } else if !msg.run_id.is_empty() {
                msg.run_id == run_id && cancels.cancel_run_for(&run_id, &msg.org_id, &msg.user_id)
            } else {
                false
            };
            if stopped {
                RunControlReply::found()
            } else {
                RunControlReply::not_found()
            }
        }
        RunControlOp::Queue if msg.request_id == request_id => {
            let outcome = queued_inputs.enqueue_checked(
                &request_id,
                &msg.org_id,
                &msg.user_id,
                msg.text.as_deref().unwrap_or_default(),
                msg.claimed_thread_id.as_deref(),
                &msg.op_id,
            );
            queue_reply(&outcome)
        }
        RunControlOp::Probe
            if msg.request_id == request_id
                && cancels.is_live_for(&request_id, &msg.org_id, &msg.user_id) =>
        {
            // The thread and the stream's own scope come from one registration: a peer that stores a
            // message for this stream needs both, so the owner states them together.
            let stream = queued_inputs.stream_for(&request_id, &msg.org_id, &msg.user_id);
            RunControlReply {
                thread_id: stream.as_ref().map(|(thread_id, _)| thread_id.clone()),
                conversation_only: stream.map(|(_, conversation_only)| conversation_only),
                ..RunControlReply::found()
            }
        }
        _ => RunControlReply::not_found(),
    }
}

/// The reply to a queue request: the outcome as the caller reads it back. `found` is false only
/// when this caller has no stream here, which is what the caller turns into the 404.
fn queue_reply(outcome: &EnqueueOutcome) -> RunControlReply {
    match outcome {
        EnqueueOutcome::NoActiveStream => RunControlReply::not_found(),
        outcome => RunControlReply {
            outcome: serde_json::to_string(outcome).ok(),
            ..RunControlReply::found()
        },
    }
}

impl RunControl {
    /// Make the stream `request_id` (producing `run_id`, when it has one) reachable from the other
    /// replicas: subscribe to its request subjects (cancel, queue, probe) and its run's cancel
    /// subject, and hand the listener to its entry in `cancels`, so [`CancelRegistry::finish`]
    /// ends the subscription.
    /// Call it right after the stream registers and before the client hears `connected`; a Stop
    /// can then never arrive at a stream nobody is listening for.
    ///
    /// Bounded by 250 ms and best effort: on a failure the stream runs as it did before, local
    /// only. Returns whether the stream is now reachable. With the bus off it does nothing, and
    /// a ZDR turn never gets here (it leaves for its direct stream before it registers).
    pub async fn attach(
        &self,
        cancels: &CancelRegistry,
        queued_inputs: &QueuedInputRegistry,
        request_id: &str,
        run_id: &str,
    ) -> bool {
        if !self.enabled() {
            return false;
        }
        let mut subjects = Vec::with_capacity(2);
        match RunControlSubject::request_any_op(request_id) {
            Ok(subject) => subjects.push(subject),
            Err(_) => return false,
        }
        if !run_id.is_empty() {
            // A run id that is no subject token only costs the run-keyed Stop.
            if let Ok(subject) = RunControlSubject::run_cancel(run_id) {
                subjects.push(subject);
            }
        }
        let handler = stream_owner(
            cancels.clone(),
            queued_inputs.clone(),
            request_id.to_owned(),
            run_id.to_owned(),
        );
        match tokio::time::timeout(ATTACH_BUDGET, self.listen(&subjects, handler)).await {
            Ok(Ok(listener)) => cancels.attach_remote(request_id, listener),
            Ok(Err(error)) => {
                warn!(%error, "run control: could not listen for a stream, a Stop from another replica will not reach it");
                false
            }
            Err(_) => {
                warn!("run control: listening for a stream timed out, a Stop from another replica will not reach it");
                false
            }
        }
    }

    /// Stop this caller's stream `request_id`: here when this replica owns it, otherwise ask the
    /// replica that does. The local registry wins, so a Stop on the owner never touches the bus.
    pub async fn cancel_stream(
        &self,
        cancels: &CancelRegistry,
        request_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> CancelRoute {
        if cancels.cancel_for(request_id, org_id, user_id) {
            self.count_local(RunControlOp::Cancel);
            return CancelRoute::Local;
        }
        let Ok(subject) = RunControlSubject::request(request_id, RunControlOp::Cancel) else {
            return CancelRoute::NoOwner;
        };
        let mut msg = RunControlMsg::new(RunControlOp::Cancel, org_id, user_id);
        msg.request_id = request_id.to_owned();
        self.ask_to_cancel(&subject, &msg).await
    }

    /// Stop this caller's live stream for the durable run `run_id`, after the run itself was
    /// cancelled: here, or on whichever replica produces it. A run with no live stream anywhere
    /// is the normal answer ([`CancelRoute::NoOwner`]).
    pub async fn cancel_run(
        &self,
        cancels: &CancelRegistry,
        run_id: &str,
        org_id: &str,
        user_id: &str,
    ) -> CancelRoute {
        if cancels.cancel_run_for(run_id, org_id, user_id) {
            self.count_local(RunControlOp::Cancel);
            return CancelRoute::Local;
        }
        let Ok(subject) = RunControlSubject::run_cancel(run_id) else {
            return CancelRoute::NoOwner;
        };
        let mut msg = RunControlMsg::new(RunControlOp::Cancel, org_id, user_id);
        msg.run_id = run_id.to_owned();
        self.ask_to_cancel(&subject, &msg).await
    }

    async fn ask_to_cancel(
        &self,
        subject: &RunControlSubject,
        msg: &RunControlMsg,
    ) -> CancelRoute {
        match self.request(subject, msg).await {
            Ok(reply) if reply.found => CancelRoute::Peer,
            Ok(_)
            | Err(
                RunControlError::NoOwner
                | RunControlError::Disabled
                | RunControlError::InvalidSubject,
            ) => CancelRoute::NoOwner,
            Err(
                RunControlError::Timeout
                | RunControlError::BadReply
                | RunControlError::PayloadTooLarge
                | RunControlError::Unavailable(_),
            ) => CancelRoute::Unavailable,
        }
    }

    /// Queue the message `msg` (built by [`crate::queued_input::bus_message`], which is the ZDR
    /// and waived gate) for this caller's stream `msg.request_id`: here when this replica owns
    /// it, otherwise ask the replica that does. The local registry wins, so a message queued on
    /// the owner never touches the bus, and the text goes on the bus only for a stream that is not
    /// here.
    ///
    /// One request that timed out is repeated once with the same `op_id`: the owner either never
    /// got it or got it and lost the reply, and in both cases it queues the message exactly once.
    pub async fn queue_input(
        &self,
        queued_inputs: &QueuedInputRegistry,
        msg: &RunControlMsg,
    ) -> QueueRoute {
        let local = queued_inputs.enqueue_checked(
            &msg.request_id,
            &msg.org_id,
            &msg.user_id,
            msg.text.as_deref().unwrap_or_default(),
            msg.claimed_thread_id.as_deref(),
            &msg.op_id,
        );
        if local != EnqueueOutcome::NoActiveStream {
            self.count_local(RunControlOp::Queue);
            return QueueRoute::Local(local);
        }
        let Ok(subject) = RunControlSubject::request(&msg.request_id, RunControlOp::Queue) else {
            return QueueRoute::NoOwner;
        };
        let mut result = self.request(&subject, msg).await;
        if result == Err(RunControlError::Timeout) && !msg.op_id.is_empty() {
            result = self.request(&subject, msg).await;
        }
        match result {
            Ok(reply) if !reply.found => QueueRoute::NoOwner,
            Ok(reply) => reply
                .outcome
                .and_then(|outcome| serde_json::from_str(&outcome).ok())
                .map_or(QueueRoute::Unavailable, QueueRoute::Peer),
            Err(
                RunControlError::NoOwner
                | RunControlError::Disabled
                | RunControlError::InvalidSubject,
            ) => QueueRoute::NoOwner,
            // The message may be too long to put on the bus even though it passed the local
            // bounds (the bounds count characters, the bus counts bytes).
            Err(RunControlError::PayloadTooLarge) => QueueRoute::Local(EnqueueOutcome::TooLong {
                chars: msg.text.as_deref().map_or(0, |text| text.trim().chars().count()),
            }),
            Err(
                RunControlError::Timeout
                | RunControlError::BadReply
                | RunControlError::Unavailable(_),
            ) => QueueRoute::Unavailable,
        }
    }

    /// Whether the stream `request_id` of this caller is being produced on some other replica.
    /// Any failure to learn that is "no": the resume tail then falls back to its inactivity
    /// window, as it did before the bus existed.
    async fn probe(&self, request_id: &str, org_id: &str, user_id: &str) -> bool {
        let Ok(subject) = RunControlSubject::request(request_id, RunControlOp::Probe) else {
            return false;
        };
        let mut msg = RunControlMsg::new(RunControlOp::Probe, org_id, user_id);
        request_id.clone_into(&mut msg.request_id);
        matches!(self.request(&subject, &msg).await, Ok(reply) if reply.found)
    }

    /// Count a request this replica answered itself, so `run_control_requests_total` shows the
    /// local share next to the peer one. Counted only with the bus on: off must leave no trace.
    fn count_local(&self, op: RunControlOp) {
        if self.enabled() {
            gateway_metrics::run_control_request(op.as_str(), RunControlOutcome::Local);
        }
    }
}

async fn serve(
    client: async_nats::Client,
    mut subscription: async_nats::Subscriber,
    handler: Handler,
) {
    while let Some(message) = subscription.next().await {
        // A message with no reply subject is not a request; there is nobody to answer.
        let Some(reply_to) = message.reply else {
            continue;
        };
        let Some(msg) = decode_msg(&message.payload) else {
            warn!(
                subject = %message.subject,
                "run control: dropped a malformed or unsupported-version request"
            );
            continue;
        };
        let Ok(payload) = encode(&handler(msg)) else {
            continue;
        };
        if let Err(error) = client.publish(reply_to, payload.into()).await {
            warn!(%error, "run control: could not publish the reply");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::RetrievalScope;

    fn rc(hub: &LoopbackHub) -> RunControl {
        RunControl::loopback(hub.clone(), Duration::from_millis(200))
    }

    fn cancel_msg(request_id: &str) -> RunControlMsg {
        let mut msg = RunControlMsg::new(RunControlOp::Cancel, "org-a", "user-a");
        msg.request_id = request_id.to_owned();
        msg
    }

    /// An owner for `request_id` that answers only its own org and user, as the cancel and
    /// queue handlers will.
    async fn owner(control: &RunControl, request_id: &str) -> RemoteListener {
        control
            .listen(
                &[RunControlSubject::request_any_op(request_id).expect("subject")],
                |msg| {
                    if msg.org_id == "org-a" && msg.user_id == "user-a" {
                        RunControlReply::found()
                    } else {
                        RunControlReply::not_found()
                    }
                },
            )
            .await
            .expect("listen")
    }

    #[test]
    fn subject_tokens_accept_only_the_safe_alphabet() {
        for ok in ["a", "req-1", "01HXYZ_abc", &"x".repeat(64)] {
            assert_eq!(subject_token(ok), Some(ok), "{ok}");
        }
        for bad in [
            "",
            "a.b",
            "a*",
            "*",
            ">",
            "a b",
            "a\nb",
            "a/b",
            "é",
            &"x".repeat(65),
        ] {
            assert_eq!(subject_token(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_id_cannot_rewrite_the_subject_it_names() {
        for bad in ["a.b", "*", ">", "req-1.cancel", "a b", ""] {
            assert_eq!(
                RunControlSubject::request(bad, RunControlOp::Cancel),
                Err(RunControlError::InvalidSubject)
            );
            assert_eq!(
                RunControlSubject::request_any_op(bad),
                Err(RunControlError::InvalidSubject)
            );
            assert_eq!(
                RunControlSubject::run_cancel(bad),
                Err(RunControlError::InvalidSubject)
            );
        }
        assert_eq!(
            RunControlSubject::request("req-1", RunControlOp::Queue)
                .expect("subject")
                .as_str(),
            "mp.v1.runctl.req.req-1.queue"
        );
        assert_eq!(
            RunControlSubject::run_cancel("run_9")
                .expect("subject")
                .as_str(),
            "mp.v1.runctl.run.run_9.cancel"
        );
    }

    #[test]
    fn subject_matching_follows_the_nats_rules() {
        assert!(subject_matches(
            "mp.v1.runctl.>",
            "mp.v1.runctl.req.a.cancel"
        ));
        assert!(subject_matches(
            "mp.v1.runctl.req.a.*",
            "mp.v1.runctl.req.a.queue"
        ));
        assert!(subject_matches(
            "mp.v1.runctl.run.r.cancel",
            "mp.v1.runctl.run.r.cancel"
        ));
        assert!(!subject_matches(
            "mp.v1.runctl.req.a.*",
            "mp.v1.runctl.req.b.queue"
        ));
        assert!(!subject_matches(
            "mp.v1.runctl.req.a.*",
            "mp.v1.runctl.req.a.queue.x"
        ));
        assert!(!subject_matches("mp.v1.runctl.>", "mp.v1.runctl"));
        assert!(!subject_matches("mp.v1.runctl.>", "mp.v1.run.x.event"));
        assert!(!subject_matches(
            "mp.v1.runctl.req.a.*",
            "mp.v1.runctl.req.a"
        ));
    }

    #[test]
    fn only_the_literal_true_turns_the_bus_on() {
        assert!(enabled_from(Some("true")));
        assert!(enabled_from(Some(" TRUE ")));
        for off in [
            None,
            Some(""),
            Some("on"),
            Some("1"),
            Some("yes"),
            Some("false"),
            Some("tru"),
        ] {
            assert!(!enabled_from(off), "{off:?}");
        }
    }

    #[test]
    fn the_timeout_defaults_and_clamps() {
        assert_eq!(timeout_from(None), Duration::from_millis(750));
        assert_eq!(timeout_from(Some("abc")), Duration::from_millis(750));
        assert_eq!(timeout_from(Some("")), Duration::from_millis(750));
        assert_eq!(timeout_from(Some("-5")), Duration::from_millis(750));
        assert_eq!(timeout_from(Some("1")), Duration::from_millis(100));
        assert_eq!(timeout_from(Some(" 1200 ")), Duration::from_millis(1200));
        assert_eq!(timeout_from(Some("999999")), Duration::from_secs(5));
    }

    #[test]
    fn run_control_message_has_no_credential_field() {
        let mut msg = RunControlMsg::new(RunControlOp::Queue, "org", "user");
        msg.request_id = "r".into();
        msg.run_id = "run".into();
        msg.op_id = "op".into();
        msg.text = Some("hello".into());
        msg.claimed_thread_id = Some("thread".into());
        let value = serde_json::to_value(&msg).expect("serialize");
        let mut keys: Vec<_> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "claimed_thread_id",
                "op",
                "op_id",
                "org_id",
                "request_id",
                "run_id",
                "text",
                "user_id",
                "v"
            ]
        );
        let reply = serde_json::to_value(RunControlReply {
            thread_id: Some("t".into()),
            outcome: Some("queued".into()),
            ..RunControlReply::found()
        })
        .expect("serialize");
        let mut keys: Vec<_> = reply.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["found", "outcome", "thread_id", "v"]);
        // The owner's probe answer states the stream's scope, and only then does the key appear.
        let probe = serde_json::to_value(RunControlReply {
            thread_id: Some("t".into()),
            conversation_only: Some(true),
            ..RunControlReply::found()
        })
        .expect("serialize");
        let mut keys: Vec<_> = probe.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["conversation_only", "found", "thread_id", "v"]);
    }

    #[tokio::test]
    async fn loopback_request_reaches_the_owner_and_answers() {
        let hub = LoopbackHub::new();
        let (replica_a, replica_b) = (rc(&hub), rc(&hub));
        let _owner = owner(&replica_a, "req-1").await;

        let subject = RunControlSubject::request("req-1", RunControlOp::Cancel).expect("subject");
        let reply = replica_b
            .request(&subject, &cancel_msg("req-1"))
            .await
            .expect("owner answers");
        assert!(reply.found);
    }

    #[tokio::test]
    async fn the_owner_sees_the_callers_identity() {
        let hub = LoopbackHub::new();
        let (replica_a, replica_b) = (rc(&hub), rc(&hub));
        let _owner = owner(&replica_a, "req-1").await;

        let subject = RunControlSubject::request("req-1", RunControlOp::Cancel).expect("subject");
        let mut foreign = cancel_msg("req-1");
        foreign.org_id = "org-b".into();
        let reply = replica_b
            .request(&subject, &foreign)
            .await
            .expect("answered");
        assert!(
            !reply.found,
            "a foreign caller must get the same answer as no stream"
        );
    }

    #[tokio::test]
    async fn no_listener_is_no_owner() {
        let hub = LoopbackHub::new();
        let subject = RunControlSubject::request("req-1", RunControlOp::Cancel).expect("subject");
        assert_eq!(
            rc(&hub).request(&subject, &cancel_msg("req-1")).await,
            Err(RunControlError::NoOwner)
        );
    }

    #[tokio::test]
    async fn a_listener_for_another_stream_is_not_an_owner() {
        let hub = LoopbackHub::new();
        let control = rc(&hub);
        let _other = owner(&control, "req-2").await;
        let subject = RunControlSubject::request("req-1", RunControlOp::Cancel).expect("subject");
        assert_eq!(
            control.request(&subject, &cancel_msg("req-1")).await,
            Err(RunControlError::NoOwner)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_owner_that_never_answers_is_a_timeout_not_a_hang() {
        let hub = LoopbackHub::new();
        let control = RunControl::loopback(hub.clone(), Duration::from_millis(750));
        let _owner = owner(&control, "req-1").await;
        hub.mute(true);

        let subject = RunControlSubject::request("req-1", RunControlOp::Cancel).expect("subject");
        assert_eq!(
            control.request(&subject, &cancel_msg("req-1")).await,
            Err(RunControlError::Timeout)
        );
    }

    #[tokio::test]
    async fn dropping_the_listener_unsubscribes() {
        let hub = LoopbackHub::new();
        let control = rc(&hub);
        let listener = owner(&control, "req-1").await;
        let subject = RunControlSubject::request("req-1", RunControlOp::Cancel).expect("subject");
        assert!(control
            .request(&subject, &cancel_msg("req-1"))
            .await
            .is_ok());

        drop(listener);
        assert_eq!(
            control.request(&subject, &cancel_msg("req-1")).await,
            Err(RunControlError::NoOwner)
        );
    }

    #[tokio::test]
    async fn the_run_subject_reaches_a_run_listener() {
        let hub = LoopbackHub::new();
        let control = rc(&hub);
        let _listener = control
            .listen(
                &[RunControlSubject::run_cancel("run_9").expect("subject")],
                |msg| {
                    if msg.run_id == "run_9" {
                        RunControlReply::found()
                    } else {
                        RunControlReply::not_found()
                    }
                },
            )
            .await
            .expect("listen");
        let mut msg = RunControlMsg::new(RunControlOp::Cancel, "org-a", "user-a");
        msg.run_id = "run_9".into();
        let subject = RunControlSubject::run_cancel("run_9").expect("subject");
        assert!(
            control
                .request(&subject, &msg)
                .await
                .expect("answered")
                .found
        );
    }

    #[tokio::test]
    async fn a_wildcard_subject_cannot_be_requested() {
        let hub = LoopbackHub::new();
        let control = rc(&hub);
        let _owner = owner(&control, "req-1").await;
        let wildcard = RunControlSubject::request_any_op("req-1").expect("subject");
        assert_eq!(
            control.request(&wildcard, &cancel_msg("req-1")).await,
            Err(RunControlError::InvalidSubject)
        );
    }

    #[tokio::test]
    async fn an_oversized_message_is_refused_before_it_is_sent() {
        let hub = LoopbackHub::new();
        let control = rc(&hub);
        let _owner = owner(&control, "req-1").await;
        let mut msg = RunControlMsg::new(RunControlOp::Queue, "org-a", "user-a");
        msg.request_id = "req-1".into();
        msg.text = Some("x".repeat(MAX_PAYLOAD_BYTES));
        let subject = RunControlSubject::request("req-1", RunControlOp::Queue).expect("subject");
        assert_eq!(
            control.request(&subject, &msg).await,
            Err(RunControlError::PayloadTooLarge)
        );
    }

    #[tokio::test]
    async fn the_bus_off_refuses_requests_and_listens_to_nothing() {
        let control = RunControl::off();
        assert!(!control.enabled());
        assert_eq!(control.status(), RunControlStatus::Off);
        let subject = RunControlSubject::request("req-1", RunControlOp::Cancel).expect("subject");
        assert_eq!(
            control.request(&subject, &cancel_msg("req-1")).await,
            Err(RunControlError::Disabled)
        );
        drop(owner(&control, "req-1").await);
    }

    #[tokio::test]
    async fn an_unset_or_wrong_switch_leaves_the_bus_off() {
        for value in [None, Some(""), Some("on"), Some("1"), Some("false")] {
            let control = RunControl::configured(value, None, Some("nats://127.0.0.1:1")).await;
            assert!(!control.enabled(), "{value:?}");
            assert_eq!(control.status(), RunControlStatus::Off, "{value:?}");
        }
    }

    #[tokio::test]
    async fn the_switch_on_without_a_broker_uses_the_in_process_hub() {
        let control = RunControl::configured(Some("true"), Some("300"), None).await;
        assert!(control.enabled());
        assert_eq!(control.status(), RunControlStatus::Ok);
        assert_eq!(control.timeout, Duration::from_millis(300));
    }

    #[tokio::test]
    async fn a_broker_that_will_not_connect_degrades_to_off() {
        // Nothing listens on port 1.
        let control = RunControl::configured(Some("true"), None, Some("nats://127.0.0.1:1")).await;
        assert!(!control.enabled());
        assert_eq!(control.status(), RunControlStatus::Degraded);
    }

    #[tokio::test]
    async fn the_self_test_passes_over_a_working_transport_and_leaves_nothing_subscribed() {
        let hub = LoopbackHub::new();
        let control = rc(&hub).self_tested().await;
        assert_eq!(control.status(), RunControlStatus::Ok);
        assert!(control.enabled());
        assert!(hub.lock().listeners.is_empty());
    }

    #[test]
    fn a_future_protocol_version_is_dropped_not_misread() {
        let mut msg = cancel_msg("req-1");
        msg.v = PROTOCOL_VERSION + 1;
        let bytes = serde_json::to_vec(&msg).expect("serialize");
        assert!(decode_msg(&bytes).is_none());
        let mut reply = RunControlReply::found();
        reply.v = PROTOCOL_VERSION + 1;
        let bytes = serde_json::to_vec(&reply).expect("serialize");
        assert_eq!(decode_reply(&bytes), Err(RunControlError::BadReply));
        assert!(decode_msg(b"not json").is_none());
    }

    #[tokio::test]
    async fn an_attached_stream_is_stoppable_from_a_peer_until_it_finishes() {
        let hub = LoopbackHub::new();
        let (owner, peer) = (rc(&hub), rc(&hub));
        let (owner_cancels, peer_cancels) = (CancelRegistry::new(), CancelRegistry::new());
        let flag = owner_cancels.register_run("req-1", "org-a", "user-a", "run-1");
        assert!(owner.attach(&owner_cancels, &QueuedInputRegistry::new(), "req-1", "run-1").await);
        assert_eq!(hub.lock().listeners.len(), 1);

        assert_eq!(
            peer.cancel_stream(&peer_cancels, "req-1", "org-b", "user-a").await,
            CancelRoute::NoOwner
        );
        assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(
            peer.cancel_stream(&peer_cancels, "req-1", "org-a", "user-a").await,
            CancelRoute::Peer
        );
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));

        owner_cancels.finish("req-1");
        assert!(hub.lock().listeners.is_empty());
        assert_eq!(
            peer.cancel_stream(&peer_cancels, "req-1", "org-a", "user-a").await,
            CancelRoute::NoOwner
        );
    }

    #[tokio::test]
    async fn attach_with_the_bus_off_or_an_unsafe_id_subscribes_nothing() {
        let cancels = CancelRegistry::new();
        let _flag = cancels.register_run("req-1", "org-a", "user-a", "run-1");
        assert!(!RunControl::off().attach(&cancels, &QueuedInputRegistry::new(), "req-1", "run-1").await);

        let hub = LoopbackHub::new();
        let control = rc(&hub);
        let _flag = cancels.register("a.b", "org-a", "user-a");
        assert!(!control.attach(&cancels, &QueuedInputRegistry::new(), "a.b", "").await);
        // A run id that is no subject token costs only the run-keyed Stop.
        assert!(control.attach(&cancels, &QueuedInputRegistry::new(), "req-1", "run.1").await);
        assert_eq!(
            control
                .cancel_run(&CancelRegistry::new(), "run.1", "org-a", "user-a")
                .await,
            CancelRoute::NoOwner
        );
    }

    #[tokio::test]
    async fn the_owner_answers_only_for_its_own_ids() {
        let hub = LoopbackHub::new();
        let control = rc(&hub);
        let (cancels, queued_inputs) = (CancelRegistry::new(), QueuedInputRegistry::new());
        let flag = cancels.register_run("req-1", "org-a", "user-a", "run-1");
        queued_inputs.register("req-1", "org-a", "user-a", "thread-1", false, RetrievalScope::Auto);
        assert!(control.attach(&cancels, &queued_inputs, "req-1", "run-1").await);

        // A message whose ids are not the stream's it was delivered for is not honoured, for any op.
        for op in [RunControlOp::Cancel, RunControlOp::Queue, RunControlOp::Probe] {
            let mut msg = RunControlMsg::new(op, "org-a", "user-a");
            msg.request_id = "req-2".into();
            msg.run_id = "run-2".into();
            msg.text = Some("hello".into());
            let subject = RunControlSubject::request("req-1", op).expect("subject");
            assert!(!control.request(&subject, &msg).await.expect("answered").found, "{op:?}");
        }
        assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));
        assert!(queued_inputs.drain("req-1").is_empty());
    }

    /// The owner's probe answer states the stream's thread AND its own scope, for a conversation-only
    /// stream and an ordinary one alike; a caller that is not the stream's owner learns neither.
    #[tokio::test]
    async fn a_probe_answer_states_the_streams_thread_and_scope() {
        let hub = LoopbackHub::new();
        let (owner, peer) = (rc(&hub), rc(&hub));
        let (owner_cancels, owner_queue) = (CancelRegistry::new(), QueuedInputRegistry::new());
        let _flag = owner_cancels.register_run("req-1", "org-a", "user-a", "run-1");
        owner_queue.register("req-1", "org-a", "user-a", "thread-1", true, RetrievalScope::Auto);
        assert!(owner.attach(&owner_cancels, &owner_queue, "req-1", "run-1").await);
        let _ordinary = owner_cancels.register_run("req-2", "org-a", "user-a", "run-2");
        owner_queue.register("req-2", "org-a", "user-a", "thread-2", false, RetrievalScope::Auto);
        assert!(owner.attach(&owner_cancels, &owner_queue, "req-2", "run-2").await);

        let probe = |request_id: &str, user_id: &str| {
            let mut msg = RunControlMsg::new(RunControlOp::Probe, "org-a", user_id);
            msg.request_id = request_id.into();
            msg
        };
        let subject = |request_id: &str| {
            RunControlSubject::request(request_id, RunControlOp::Probe).expect("subject")
        };

        let reply = peer.request(&subject("req-1"), &probe("req-1", "user-a")).await.expect("answered");
        assert!(reply.found);
        assert_eq!(reply.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(reply.conversation_only, Some(true));
        let reply = peer.request(&subject("req-2"), &probe("req-2", "user-a")).await.expect("answered");
        assert_eq!(reply.thread_id.as_deref(), Some("thread-2"));
        assert_eq!(reply.conversation_only, Some(false), "an ordinary stream says so too");

        let stranger = peer.request(&subject("req-1"), &probe("req-1", "user-b")).await.expect("answered");
        assert!(!stranger.found);
        assert_eq!((stranger.thread_id, stranger.conversation_only), (None, None));
    }

    #[tokio::test]
    async fn a_queued_message_reaches_the_owner_and_comes_back_as_its_outcome() {
        let hub = LoopbackHub::new();
        let (owner, peer) = (rc(&hub), rc(&hub));
        let (owner_cancels, owner_queue) = (CancelRegistry::new(), QueuedInputRegistry::new());
        let peer_queue = QueuedInputRegistry::new();
        let _flag = owner_cancels.register_run("req-1", "org-a", "user-a", "run-1");
        owner_queue.register("req-1", "org-a", "user-a", "thread-1", true, RetrievalScope::WebOnly);
        assert!(owner.attach(&owner_cancels, &owner_queue, "req-1", "run-1").await);

        let mut msg = RunControlMsg::new(RunControlOp::Queue, "org-a", "user-a");
        msg.request_id = "req-1".into();
        msg.op_id = "op-1".into();
        msg.text = Some("  use EUR ".into());
        let route = peer.queue_input(&peer_queue, &msg).await;
        assert_eq!(
            route,
            QueueRoute::Peer(EnqueueOutcome::Queued {
                pending: 1,
                thread_id: "thread-1".into(),
                conversation_only: true,
                retrieval_scope: RetrievalScope::WebOnly,
            })
        );
        assert_eq!(owner_queue.drain("req-1"), ["use EUR"]);
        assert!(peer_queue.drain("req-1").is_empty(), "nothing is queued on the replica that asked");

        // A stranger is told what a missing stream is told.
        let mut stranger = msg.clone();
        stranger.user_id = "user-b".into();
        assert_eq!(peer.queue_input(&peer_queue, &stranger).await, QueueRoute::NoOwner);
        assert!(owner_queue.drain("req-1").is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_probe_is_cached_and_asks_the_owner_only_when_the_stream_is_not_local() {
        let hub = LoopbackHub::new();
        let (owner, peer) = (rc(&hub), rc(&hub));
        let (owner_cancels, owner_queue) = (CancelRegistry::new(), QueuedInputRegistry::new());
        let peer_cancels = CancelRegistry::new();
        let _flag = owner_cancels.register_run("req-1", "org-a", "user-a", "run-1");
        owner_queue.register("req-1", "org-a", "user-a", "thread-1", false, RetrievalScope::Auto);
        assert!(owner.attach(&owner_cancels, &owner_queue, "req-1", "run-1").await);

        let mut stranger =
            ProducerLiveness::new(peer_cancels.clone(), peer.clone(), "req-1", "org-b", "user-a");
        assert!(!stranger.is_live().await, "another tenant's probe is the answer for no stream");

        let mut remote = ProducerLiveness::new(peer_cancels, peer, "req-1", "org-a", "user-a");
        assert!(remote.is_live().await);
        // The owner finishes: the cached answer holds for two seconds, then the probe asks again.
        owner_cancels.finish("req-1");
        assert!(remote.is_live().await, "inside the cache window the answer is reused");
        tokio::time::advance(PROBE_CACHE + Duration::from_millis(1)).await;
        assert!(!remote.is_live().await);

        // On the owner the local registry answers and nothing is probed.
        let _flag = owner_cancels.register_run("req-1", "org-a", "user-a", "run-1");
        let mut local = ProducerLiveness::new(owner_cancels, RunControl::off(), "req-1", "org-a", "user-a");
        assert!(local.is_live().await);
    }
}
