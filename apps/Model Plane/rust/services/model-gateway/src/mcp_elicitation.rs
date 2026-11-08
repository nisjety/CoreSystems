//! Held MCP calls waiting on the user's answer to a server's form (build plan EP-4 S4).
//!
//! When the governed caller allows it and `MCP_ELICITATION_ENABLED` is on, a `tools/call` whose
//! server sends `elicitation/create` is not failed: the gateway maps the form onto question items
//! (`mp_contracts::mcp_elicitation`), keeps the open session and its response stream here, and
//! answers `ProxyMcpTool` with the pending form. `AnswerMcpElicitation` later posts the owner's
//! answer to the same call and reads it on.
//!
//! Bounds, because every held call is an open connection to a third party: at most
//! [`MAX_HELD_PER_ORG`] held calls per org and [`MAX_HELD_PER_PROCESS`] per process; each waits
//! at most `MCP_ELICITATION_HOLD_SECONDS` (D5), after which the reaper answers `cancel` and drops
//! it; one call may ask at most [`MAX_ELICITATIONS_PER_CALL`] forms. The registry is memory only
//! and per replica: an answer reaching a replica that does not hold the call fails closed with
//! [`MCP_ELICITATION_EXPIRED`] (D16). Logs carry ids, counts and lengths, never a message, a
//! schema or an answer.

use std::sync::{Mutex, Once, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use futures::future::BoxFuture;
use mp_contracts::mcp_elicitation::{self as form, Decline};
use mp_contracts::model_plane::v1::{
    McpElicitationPending, QuestionAnswerItem, QuestionItem, QuestionOption,
};
use mp_contracts::user_question::{Answer, Item, Problem, MAX_TEXT};
use mp_ids::new_ulid;
use serde_json::Value;

use crate::mcp_http::{Interactive, McpHttpSession, McpMessageStream};
use crate::mcp_jsonrpc::{build_elicitation_result, McpCallOutcome};

/// Error prefix for an answer to a call this replica does not hold (expired, never held here,
/// or not the caller's).
pub const MCP_ELICITATION_EXPIRED: &str = "mcp_elicitation_expired";
/// Error prefix for a form refused because too many calls are already held.
pub const MCP_ELICITATION_CAPACITY: &str = "mcp_elicitation_capacity";
/// Error prefix for a call cancelled because its server kept asking.
pub const MCP_ELICITATION_LIMIT: &str = "mcp_elicitation_limit";
/// Error prefix for an answer that does not fit the form; the call stays held.
pub const MCP_ELICITATION_INVALID_ANSWER: &str = "mcp_elicitation_invalid_answer";

/// Most calls one org may hold at a time.
pub const MAX_HELD_PER_ORG: usize = 8;
/// Most calls this process may hold at a time.
pub const MAX_HELD_PER_PROCESS: usize = 64;
/// Most forms one call may ask; one more cancels the call.
pub const MAX_ELICITATIONS_PER_CALL: u32 = 3;

/// Default and bounds of `MCP_ELICITATION_HOLD_SECONDS`. The ceiling stays below a resume
/// question's one-hour lifetime, so a held call never outlives the question that answers it.
pub const DEFAULT_HOLD_SECONDS: u64 = 600;
pub const MIN_HOLD_SECONDS: u64 = 60;
pub const MAX_HOLD_SECONDS: u64 = 1800;

/// How often the reaper looks for expired calls.
const REAP_INTERVAL: Duration = Duration::from_secs(15);
/// How much longer than the hold a held stream's request may stay open, so the reaper (not the
/// transport timeout) is what ends an unanswered call.
const STREAM_MARGIN: Duration = Duration::from_secs(60);

/// `MCP_ELICITATION_ENABLED`: only the literal `true` turns it on.
#[must_use]
pub fn enabled_from_env_value(value: Option<&str>) -> bool {
    value == Some("true")
}

/// The process's `MCP_ELICITATION_ENABLED`, read once so it cannot change under a request.
#[must_use]
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        enabled_from_env_value(std::env::var("MCP_ELICITATION_ENABLED").ok().as_deref())
    })
}

/// `MCP_ELICITATION_HOLD_SECONDS`: a whole number of seconds clamped to
/// [`MIN_HOLD_SECONDS`]..=[`MAX_HOLD_SECONDS`]; absent or unparseable reads the default.
#[must_use]
pub fn hold_seconds_from_env_value(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_HOLD_SECONDS)
        .clamp(MIN_HOLD_SECONDS, MAX_HOLD_SECONDS)
}

/// The process's hold window, read once.
#[must_use]
pub fn hold() -> Duration {
    static HOLD: OnceLock<Duration> = OnceLock::new();
    *HOLD.get_or_init(|| {
        Duration::from_secs(hold_seconds_from_env_value(
            std::env::var("MCP_ELICITATION_HOLD_SECONDS").ok().as_deref(),
        ))
    })
}

/// The request timeout of a tool call that may be held for `hold`.
#[must_use]
pub fn stream_timeout(hold: Duration) -> Duration {
    hold + STREAM_MARGIN
}

/// Whether a `ProxyMcpTool` call takes the interactive path: the caller must allow it AND the
/// gateway's switch must be on. With the switch off `allow_elicitation` is ignored.
#[must_use]
pub fn interactive(switch_on: bool, allow_elicitation: bool) -> bool {
    switch_on && allow_elicitation
}

/// What a held call's stream did after the client answered.
pub enum HeldTurn {
    Done(McpCallOutcome),
    Elicit { request_id: Value, params: Value },
}

/// The open half of a held call: answer the server, then read on. The HTTP session is the one
/// implementation; tests script it.
pub trait HeldTransport: Send {
    /// Post the client's JSON-RPC response to the server's question.
    fn respond<'a>(&'a mut self, payload: Value) -> BoxFuture<'a, Result<(), String>>;
    /// Read the call's stream on until it ends or the server asks again.
    fn read_on<'a>(&'a mut self) -> BoxFuture<'a, HeldTurn>;
}

struct HttpHeld {
    session: McpHttpSession,
    stream: Option<McpMessageStream>,
}

impl HeldTransport for HttpHeld {
    fn respond<'a>(&'a mut self, payload: Value) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move { self.session.respond(&payload).await })
    }

    fn read_on<'a>(&'a mut self) -> BoxFuture<'a, HeldTurn> {
        Box::pin(async move {
            let Some(stream) = self.stream.take() else {
                return HeldTurn::Done(McpCallOutcome::Err(
                    "the held call's stream is closed".to_owned(),
                ));
            };
            match self.session.read_on(stream).await {
                Interactive::Done(outcome) => HeldTurn::Done(outcome),
                Interactive::Elicit {
                    request_id,
                    params,
                    stream,
                } => {
                    self.stream = Some(stream);
                    HeldTurn::Elicit { request_id, params }
                }
            }
        })
    }
}

/// The transport of a call whose stream just stopped at a form.
#[must_use]
pub fn http_held(session: McpHttpSession, stream: McpMessageStream) -> Box<dyn HeldTransport> {
    Box::new(HttpHeld {
        session,
        stream: Some(stream),
    })
}

/// Who a held call belongs to and which server holds it open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldContext {
    pub org_id: String,
    pub user_id: String,
    pub server_id: String,
    pub server_label: String,
}

/// Where a call stands after a form was seen or answered.
#[derive(Debug)]
pub enum Settled {
    /// The call finished (or failed); the tool result.
    Done(McpCallOutcome),
    /// The call is held on this form.
    Pending(McpElicitationPending),
}

/// Why an answer was not delivered.
#[derive(Debug, PartialEq)]
pub enum AnswerError {
    /// No call with that id is held here for this org and user, or its window has passed.
    Expired,
    /// The action is not `accept`, `decline` or `cancel`.
    InvalidAction,
    /// The answers do not fit the form; the call stays held.
    InvalidAnswer(Problem),
}

struct HeldCall {
    // A Mutex only so the map is shareable across threads: the transport is used after its
    // entry was removed, never through the map.
    transport: Mutex<Box<dyn HeldTransport>>,
    request_id: Value,
    schema: Value,
    items: Vec<Item>,
    ctx: HeldContext,
    deadline: Instant,
    elicitations: u32,
}

impl HeldCall {
    fn into_transport(self) -> Box<dyn HeldTransport> {
        self.transport
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// A form accepted for display.
struct MappedForm {
    message: String,
    schema: Value,
    items: Vec<Item>,
}

/// Check the request's params and map its schema. Only the form mode exists in the revision
/// this client advertises; a server that sends another mode is declined.
fn map_form(params: &Value) -> Result<MappedForm, Decline> {
    if params
        .get("mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode != "form")
    {
        return Err(Decline {
            reason: "unsupported_mode",
        });
    }
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if message.chars().count() > MAX_TEXT {
        return Err(Decline {
            reason: "message_too_long",
        });
    }
    let schema = params.get("requestedSchema").cloned().unwrap_or(Value::Null);
    let items = form::items_from_schema(&schema)?;
    Ok(MappedForm {
        message: message.to_owned(),
        schema,
        items,
    })
}

fn question_item(item: &Item) -> QuestionItem {
    QuestionItem {
        id: item.id.clone(),
        kind: item.kind.clone(),
        prompt: item.prompt.clone(),
        options: item
            .options
            .iter()
            .map(|option| QuestionOption {
                id: option.id.clone(),
                label: option.label.clone(),
                description: option.description.clone(),
            })
            .collect(),
        allow_other: item.allow_other,
    }
}

/// The wire answers as the shared contract's answers.
#[must_use]
pub fn answers_from_proto(answers: &[QuestionAnswerItem]) -> Vec<Answer> {
    answers
        .iter()
        .map(|answer| Answer {
            item_id: answer.item_id.clone(),
            option_ids: answer.option_ids.clone(),
            text: answer.text.clone(),
        })
        .collect()
}

fn cancel_result(request_id: &Value) -> Value {
    build_elicitation_result(request_id, "cancel", None)
}

/// The in-memory store of held calls.
pub struct ElicitationRegistry {
    held: DashMap<String, HeldCall>,
    /// Serialises the cap check with the insert, so concurrent forms cannot overshoot a cap.
    admit_lock: Mutex<()>,
    hold: Duration,
}

impl ElicitationRegistry {
    #[must_use]
    pub fn new(hold: Duration) -> Self {
        Self {
            held: DashMap::new(),
            admit_lock: Mutex::new(()),
            hold,
        }
    }

    /// The process's registry; its reaper starts with the first use inside a runtime.
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<ElicitationRegistry> = OnceLock::new();
        static REAPER: Once = Once::new();
        let registry = REGISTRY.get_or_init(|| Self::new(hold()));
        if tokio::runtime::Handle::try_current().is_ok() {
            REAPER.call_once(|| registry.spawn_reaper());
        }
        registry
    }

    /// The hold window of this registry.
    #[must_use]
    pub fn hold(&self) -> Duration {
        self.hold
    }

    /// How many calls are held right now.
    #[must_use]
    pub fn held_count(&self) -> usize {
        self.held.len()
    }

    fn admit(&self, id: String, call: HeldCall) -> Result<(), HeldCall> {
        let _guard = self
            .admit_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let in_org = self
            .held
            .iter()
            .filter(|entry| entry.ctx.org_id == call.ctx.org_id)
            .count();
        if self.held.len() >= MAX_HELD_PER_PROCESS || in_org >= MAX_HELD_PER_ORG {
            return Err(call);
        }
        self.held.insert(id, call);
        Ok(())
    }

    /// A call's stream stopped at its first form: hold it, or settle it when the form is
    /// declined or no capacity is left.
    pub async fn begin(
        &self,
        transport: Box<dyn HeldTransport>,
        request_id: Value,
        params: Value,
        ctx: HeldContext,
    ) -> Settled {
        self.settle(transport, request_id, params, ctx, 1).await
    }

    /// Handle the form a call is stopped at, `count` being how many forms the call has asked
    /// so far. A form that does not map is declined to the server and the stream read on.
    async fn settle(
        &self,
        mut transport: Box<dyn HeldTransport>,
        mut request_id: Value,
        mut params: Value,
        ctx: HeldContext,
        mut count: u32,
    ) -> Settled {
        loop {
            if count > MAX_ELICITATIONS_PER_CALL {
                // The call ends here either way; a failed cancel changes nothing.
                let _ = transport.respond(cancel_result(&request_id)).await;
                tracing::info!(server_id = %ctx.server_id, count, "mcp elicitation: call cancelled, too many forms");
                return Settled::Done(McpCallOutcome::Err(format!(
                    "{MCP_ELICITATION_LIMIT}: the server asked more than {MAX_ELICITATIONS_PER_CALL} questions in one call"
                )));
            }
            let mapped = match map_form(&params) {
                Ok(mapped) => mapped,
                Err(decline) => {
                    tracing::info!(server_id = %ctx.server_id, reason = decline.reason, "mcp elicitation: form declined");
                    let decline_result = build_elicitation_result(&request_id, "decline", None);
                    if let Err(error) = transport.respond(decline_result).await {
                        return Settled::Done(McpCallOutcome::Err(error));
                    }
                    match transport.read_on().await {
                        HeldTurn::Done(outcome) => return Settled::Done(outcome),
                        HeldTurn::Elicit {
                            request_id: next_id,
                            params: next_params,
                        } => {
                            request_id = next_id;
                            params = next_params;
                            count += 1;
                            continue;
                        }
                    }
                }
            };
            let id = format!("el_{}", new_ulid());
            let pending = McpElicitationPending {
                elicitation_id: id.clone(),
                server_id: ctx.server_id.clone(),
                server_label: ctx.server_label.clone(),
                message: mapped.message,
                requested_schema_json: mapped.schema.to_string(),
                items: mapped.items.iter().map(question_item).collect(),
                expires_at_unix: chrono::Utc::now()
                    .timestamp()
                    .saturating_add(i64::try_from(self.hold.as_secs()).unwrap_or(i64::MAX)),
            };
            let call = HeldCall {
                transport: Mutex::new(transport),
                request_id,
                schema: mapped.schema,
                items: mapped.items,
                ctx,
                deadline: Instant::now() + self.hold,
                elicitations: count,
            };
            let server_id = call.ctx.server_id.clone();
            return match self.admit(id.clone(), call) {
                Ok(()) => {
                    tracing::info!(elicitation_id = %id, %server_id, count, items = pending.items.len(), "mcp elicitation: call held");
                    Settled::Pending(pending)
                }
                Err(refused) => {
                    let request_id = refused.request_id.clone();
                    let mut transport = refused.into_transport();
                    let _ = transport.respond(cancel_result(&request_id)).await;
                    tracing::warn!(%server_id, held = self.held.len(), "mcp elicitation: no capacity, form cancelled");
                    Settled::Done(McpCallOutcome::Err(format!(
                        "{MCP_ELICITATION_CAPACITY}: too many forms are waiting for an answer"
                    )))
                }
            };
        }
    }

    /// Deliver the owner's answer to a held call and read the call on. Only the org and user the
    /// call was held for can reach it; anyone else is told it expired, which reveals nothing.
    ///
    /// # Errors
    /// [`AnswerError`]: the call is not held here for this caller, the action is unknown, or the
    /// answers do not fit the form (the call then stays held for a corrected answer).
    pub async fn answer(
        &self,
        elicitation_id: &str,
        org_id: &str,
        user_id: &str,
        action: &str,
        answers: &[Answer],
    ) -> Result<Settled, AnswerError> {
        if !matches!(action, "accept" | "decline" | "cancel") {
            return Err(AnswerError::InvalidAction);
        }
        let now = Instant::now();
        let Some((_, held)) = self.held.remove_if(elicitation_id, |_, held| {
            held.ctx.org_id == org_id && held.ctx.user_id == user_id && held.deadline > now
        }) else {
            return Err(AnswerError::Expired);
        };
        let content = if action == "accept" {
            match form::content_from_answers(&held.schema, &held.items, answers) {
                Ok(content) => Some(content),
                Err(problem) => {
                    self.held.insert(elicitation_id.to_owned(), held);
                    return Err(AnswerError::InvalidAnswer(problem));
                }
            }
        } else {
            None
        };
        let request_id = held.request_id.clone();
        let ctx = held.ctx.clone();
        let count = held.elicitations;
        let mut transport = held.into_transport();
        let result = build_elicitation_result(&request_id, action, content.as_ref());
        if let Err(error) = transport.respond(result).await {
            return Ok(Settled::Done(McpCallOutcome::Err(error)));
        }
        tracing::info!(%elicitation_id, action, answers = answers.len(), "mcp elicitation: answered");
        Ok(match transport.read_on().await {
            HeldTurn::Done(outcome) => Settled::Done(outcome),
            HeldTurn::Elicit { request_id, params } => {
                self.settle(transport, request_id, params, ctx, count + 1)
                    .await
            }
        })
    }

    fn take_expired(&self, now: Instant) -> Vec<(String, HeldCall)> {
        let expired: Vec<String> = self
            .held
            .iter()
            .filter(|entry| entry.deadline <= now)
            .map(|entry| entry.key().clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|id| self.held.remove_if(&id, |_, held| held.deadline <= now))
            .collect()
    }

    /// Cancel every call whose window has passed by `now` (D5): the server gets a `cancel`
    /// result and the connection is dropped. Returns how many were cancelled.
    pub async fn reap(&self, now: Instant) -> usize {
        let expired = self.take_expired(now);
        let count = expired.len();
        for (id, held) in expired {
            let request_id = held.request_id.clone();
            let mut transport = held.into_transport();
            if let Err(error) = transport.respond(cancel_result(&request_id)).await {
                tracing::debug!(elicitation_id = %id, error_len = error.len(), "mcp elicitation: cancel not delivered");
            }
            tracing::info!(elicitation_id = %id, "mcp elicitation: expired, cancelled");
        }
        count
    }

    fn spawn_reaper(&'static self) {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(REAP_INTERVAL);
            loop {
                tick.tick().await;
                self.reap(Instant::now()).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;

    use serde_json::json;

    use super::*;

    #[derive(Clone, Default)]
    struct Sent(Arc<Mutex<Vec<Value>>>);

    impl Sent {
        fn all(&self) -> Vec<Value> {
            self.0.lock().expect("sent").clone()
        }

        fn last_action(&self) -> Option<String> {
            self.all()
                .last()
                .and_then(|payload| payload.pointer("/result/action"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        }
    }

    struct Scripted {
        sent: Sent,
        turns: VecDeque<HeldTurn>,
    }

    impl HeldTransport for Scripted {
        fn respond<'a>(&'a mut self, payload: Value) -> BoxFuture<'a, Result<(), String>> {
            self.sent.0.lock().expect("sent").push(payload);
            Box::pin(async { Ok(()) })
        }

        fn read_on<'a>(&'a mut self) -> BoxFuture<'a, HeldTurn> {
            let turn = self.turns.pop_front().unwrap_or_else(|| {
                HeldTurn::Done(McpCallOutcome::Err("stream ended".to_owned()))
            });
            Box::pin(async move { turn })
        }
    }

    fn scripted(turns: Vec<HeldTurn>) -> (Box<dyn HeldTransport>, Sent) {
        let sent = Sent::default();
        (
            Box::new(Scripted {
                sent: sent.clone(),
                turns: turns.into(),
            }),
            sent,
        )
    }

    fn form() -> Value {
        json!({
            "message": "Hvilket prosjekt?",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "enum": ["alpha", "beta", "gamma"] },
                    "notify": { "type": "boolean" }
                }
            }
        })
    }

    fn elicit() -> HeldTurn {
        HeldTurn::Elicit {
            request_id: json!(7),
            params: form(),
        }
    }

    fn ctx(org: &str) -> HeldContext {
        HeldContext {
            org_id: org.to_owned(),
            user_id: "u1".to_owned(),
            server_id: "srv".to_owned(),
            server_label: "Fixture".to_owned(),
        }
    }

    fn answers() -> Vec<Answer> {
        vec![
            Answer {
                item_id: "project".to_owned(),
                option_ids: vec!["o1".to_owned()],
                text: String::new(),
            },
            Answer {
                item_id: "notify".to_owned(),
                option_ids: vec![form::BOOLEAN_YES.to_owned()],
                text: String::new(),
            },
        ]
    }

    fn registry() -> ElicitationRegistry {
        ElicitationRegistry::new(Duration::from_secs(DEFAULT_HOLD_SECONDS))
    }

    async fn held(reg: &ElicitationRegistry, org: &str, turns: Vec<HeldTurn>) -> (String, Sent) {
        let (transport, sent) = scripted(turns);
        match reg.begin(transport, json!(5), form(), ctx(org)).await {
            Settled::Pending(pending) => (pending.elicitation_id, sent),
            Settled::Done(outcome) => panic!("not held: {outcome:?}"),
        }
    }

    #[test]
    fn switch_and_hold_parse_strictly() {
        assert!(enabled_from_env_value(Some("true")));
        for value in [None, Some("TRUE"), Some("1"), Some("yes"), Some(" true")] {
            assert!(!enabled_from_env_value(value), "{value:?}");
        }
        assert_eq!(hold_seconds_from_env_value(None), DEFAULT_HOLD_SECONDS);
        assert_eq!(hold_seconds_from_env_value(Some("ten")), DEFAULT_HOLD_SECONDS);
        assert_eq!(hold_seconds_from_env_value(Some("5")), MIN_HOLD_SECONDS);
        assert_eq!(hold_seconds_from_env_value(Some("99999")), MAX_HOLD_SECONDS);
        assert_eq!(hold_seconds_from_env_value(Some("900")), 900);
        assert!(!interactive(false, true));
        assert!(!interactive(true, false));
        assert!(interactive(true, true));
    }

    #[tokio::test]
    async fn a_mapped_form_is_held_with_its_items() {
        let reg = registry();
        let (transport, sent) = scripted(vec![]);
        let Settled::Pending(pending) = reg.begin(transport, json!(5), form(), ctx("org-1")).await
        else {
            panic!("not held");
        };
        assert!(pending.elicitation_id.starts_with("el_"));
        assert_eq!(pending.server_label, "Fixture");
        assert_eq!(pending.message, "Hvilket prosjekt?");
        assert_eq!(pending.items.len(), 2);
        assert!(pending.expires_at_unix > chrono::Utc::now().timestamp());
        assert!(sent.all().is_empty(), "nothing is sent until the owner answers");
        assert_eq!(reg.held_count(), 1);
    }

    #[tokio::test]
    async fn answer_posts_result_and_reads_on() {
        let reg = registry();
        let (id, sent) = held(
            &reg,
            "org-1",
            vec![HeldTurn::Done(McpCallOutcome::Ok("{\"content\":[]}".to_owned()))],
        )
        .await;
        let settled = reg
            .answer(&id, "org-1", "u1", "accept", &answers())
            .await
            .expect("delivered");
        assert!(matches!(settled, Settled::Done(McpCallOutcome::Ok(_))));
        let posted = sent.all();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0]["id"], 5);
        assert_eq!(
            posted[0]["result"],
            json!({ "action": "accept", "content": { "project": "beta", "notify": true } })
        );
        assert_eq!(reg.held_count(), 0);
        // The call is gone: a second answer reads as expired.
        assert_eq!(
            reg.answer(&id, "org-1", "u1", "accept", &answers())
                .await
                .expect_err("spent"),
            AnswerError::Expired
        );
    }

    #[tokio::test]
    async fn an_answer_that_does_not_fit_keeps_the_call_held() {
        let reg = registry();
        let (id, sent) = held(&reg, "org-1", vec![]).await;
        let refused = reg
            .answer(&id, "org-1", "u1", "accept", &answers()[..1])
            .await
            .expect_err("incomplete");
        assert!(matches!(refused, AnswerError::InvalidAnswer(_)));
        assert!(sent.all().is_empty());
        assert_eq!(reg.held_count(), 1);
    }

    #[tokio::test]
    async fn expired_entry_is_cancelled() {
        let reg = registry();
        let (id, sent) = held(&reg, "org-1", vec![]).await;
        assert_eq!(reg.reap(Instant::now()).await, 0, "not expired yet");
        let later = Instant::now() + reg.hold() + Duration::from_secs(1);
        assert_eq!(reg.reap(later).await, 1);
        assert_eq!(sent.last_action().as_deref(), Some("cancel"));
        assert_eq!(reg.held_count(), 0);
        assert_eq!(
            reg.answer(&id, "org-1", "u1", "accept", &answers())
                .await
                .expect_err("expired"),
            AnswerError::Expired
        );
    }

    #[tokio::test]
    async fn per_org_cap_refuses_a_ninth() {
        let reg = registry();
        for _ in 0..MAX_HELD_PER_ORG {
            held(&reg, "org-1", vec![]).await;
        }
        let (transport, sent) = scripted(vec![]);
        match reg.begin(transport, json!(5), form(), ctx("org-1")).await {
            Settled::Done(McpCallOutcome::Err(error)) => {
                assert!(error.starts_with(MCP_ELICITATION_CAPACITY), "{error}");
            }
            other => panic!("a ninth call was held: {other:?}"),
        }
        assert_eq!(sent.last_action().as_deref(), Some("cancel"));
        // Another org still has room.
        held(&reg, "org-2", vec![]).await;
        assert_eq!(reg.held_count(), MAX_HELD_PER_ORG + 1);
    }

    #[tokio::test]
    async fn wrong_user_cannot_answer() {
        let reg = registry();
        let (id, sent) = held(
            &reg,
            "org-1",
            vec![HeldTurn::Done(McpCallOutcome::Ok("{}".to_owned()))],
        )
        .await;
        for (org, user) in [("org-1", "u2"), ("org-2", "u1")] {
            assert_eq!(
                reg.answer(&id, org, user, "accept", &answers())
                    .await
                    .expect_err("not theirs"),
                AnswerError::Expired
            );
        }
        assert!(sent.all().is_empty());
        assert_eq!(reg.held_count(), 1, "the owner can still answer");
        assert!(reg.answer(&id, "org-1", "u1", "decline", &[]).await.is_ok());
        assert_eq!(sent.last_action().as_deref(), Some("decline"));
    }

    #[tokio::test]
    async fn an_unknown_action_is_refused_before_anything_is_sent() {
        let reg = registry();
        let (id, sent) = held(&reg, "org-1", vec![]).await;
        assert_eq!(
            reg.answer(&id, "org-1", "u1", "approve", &[])
                .await
                .expect_err("unknown"),
            AnswerError::InvalidAction
        );
        assert!(sent.all().is_empty());
        assert_eq!(reg.held_count(), 1);
    }

    #[tokio::test]
    async fn a_fourth_elicitation_cancels_the_call() {
        let reg = registry();
        // Form 1 held at begin; every answer is followed by another form.
        let (mut id, sent) = held(&reg, "org-1", vec![elicit(), elicit(), elicit()]).await;
        for _ in 1..MAX_ELICITATIONS_PER_CALL {
            match reg
                .answer(&id, "org-1", "u1", "accept", &answers())
                .await
                .expect("delivered")
            {
                Settled::Pending(pending) => id = pending.elicitation_id,
                Settled::Done(outcome) => panic!("ended early: {outcome:?}"),
            }
        }
        match reg
            .answer(&id, "org-1", "u1", "accept", &answers())
            .await
            .expect("delivered")
        {
            Settled::Done(McpCallOutcome::Err(error)) => {
                assert!(error.starts_with(MCP_ELICITATION_LIMIT), "{error}");
            }
            other => panic!("a fourth form was held: {other:?}"),
        }
        assert_eq!(sent.last_action().as_deref(), Some("cancel"));
        assert_eq!(reg.held_count(), 0);
    }

    #[tokio::test]
    async fn a_sensitive_form_is_declined_and_the_call_read_on() {
        let reg = registry();
        let (transport, sent) = scripted(vec![HeldTurn::Done(McpCallOutcome::Ok(
            "{\"content\":[]}".to_owned(),
        ))]);
        let params = json!({
            "message": "Logg inn",
            "requestedSchema": {
                "type": "object",
                "properties": { "password": { "type": "string" } }
            }
        });
        let settled = reg.begin(transport, json!(5), params, ctx("org-1")).await;
        assert!(matches!(settled, Settled::Done(McpCallOutcome::Ok(_))));
        assert_eq!(sent.last_action().as_deref(), Some("decline"));
        assert_eq!(reg.held_count(), 0);
    }

    #[tokio::test]
    async fn a_non_form_mode_is_declined() {
        let reg = registry();
        let (transport, sent) = scripted(vec![]);
        let mut params = form();
        params["mode"] = json!("url");
        let _ = reg.begin(transport, json!(5), params, ctx("org-1")).await;
        assert_eq!(sent.last_action().as_deref(), Some("decline"));
        assert_eq!(reg.held_count(), 0);
    }
}
