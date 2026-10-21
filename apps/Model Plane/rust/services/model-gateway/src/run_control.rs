//! Run control across gateway replicas: a timeout-bounded request/reply to the replica that
//! owns a live chat stream (build plan WS04-S1).
//!
//! # Why this exists
//!
//! The cancel registry, the queued-input registry and the stream/turn records are per process:
//! the SSE connection pins a run to one replica, so a Stop or a queued message that lands on the
//! other replica answers 404 and the producer runs to the end. The fix is to ask the owner. This
//! module is only the seam: it carries a small message to whichever replica subscribed for a
//! request id or a run id and brings the answer back. What the owner does with it (cancel, queue,
//! probe) belongs to the slices that register the listeners.
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

use crate::gateway_metrics::{self, RunControlOutcome};

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
}

impl RunControlReply {
    #[must_use]
    pub fn found() -> Self {
        Self {
            v: PROTOCOL_VERSION,
            found: true,
            thread_id: None,
            outcome: None,
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
    /// Aborting a serving task drops its subscription, which unsubscribes.
    Nats(Vec<JoinHandle<()>>),
}

impl Drop for RemoteListener {
    fn drop(&mut self) {
        match &self.kind {
            ListenerKind::Inert => {}
            ListenerKind::Loopback { hub, id } => hub.remove(*id),
            ListenerKind::Nats(tasks) => tasks.iter().for_each(JoinHandle::abort),
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

    fn degraded() -> Self {
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
                let mut tasks = Vec::with_capacity(subjects.len());
                for subject in subjects {
                    match client.subscribe(subject.subject.clone()).await {
                        Ok(subscription) => tasks.push(tokio::spawn(serve(
                            client.clone(),
                            subscription,
                            handler.clone(),
                        ))),
                        Err(error) => {
                            tasks.iter().for_each(JoinHandle::abort);
                            return Err(RunControlError::Unavailable(error.to_string()));
                        }
                    }
                }
                if let Err(error) = client.flush().await {
                    tasks.iter().for_each(JoinHandle::abort);
                    return Err(RunControlError::Unavailable(error.to_string()));
                }
                ListenerKind::Nats(tasks)
            }
        };
        Ok(RemoteListener { kind })
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
}
