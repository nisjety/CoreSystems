//! The `AttentionService` surface of Session Core (build plan WS16 S1): the closed attention state of
//! the caller's threads, and the mark-read that clears a thread's unseen marker.
//!
//! Both answers are owner-bound. The caller's verified identity must be the user named in the request,
//! so a service principal cannot read or mark another person's threads. The stream is the same owner's
//! live view of that state (WS16 S5), bound the same way.

use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1::{
    attention_service_server::AttentionService, session_core_server::SessionCore,
    stream_attention_response::Frame as StreamFrame, AttentionCounts, AttentionHeartbeat,
    AttentionItem, AttentionResync, AttentionSnapshot, AttentionState, AttentionThreadChanged,
    ConfirmAttentionWaitingRequest, ConfirmAttentionWaitingResponse, ListAttentionRequest,
    ListAttentionResponse, MarkThreadSeenRequest, MarkThreadSeenResponse, StreamAttentionRequest,
    StreamAttentionResponse, UpdateThreadPresentationRequest,
};
use mp_contracts::zdr_refusal::Refusal;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::attention;
use crate::attention_events::{self, ConfirmQuery};
use crate::attention_hub::{self, Body, Change, Keyed, Listing, Sequencer, StreamPermit, STREAMS};
use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::grpc::{record_metrics, SessionService};
use crate::retention_receipt::RunRetention;

/// The environment switch of the stream. Only the literal `true` turns it on, through the same parser
/// as the attention switch.
pub(crate) const STREAM_ENABLED_ENV: &str = "SESSION_CORE_ATTENTION_STREAM_ENABLED";
/// The environment setting of the tick: how often a live stream reads the owner's threads again.
pub(crate) const TICK_ENV: &str = "SESSION_CORE_ATTENTION_TICK_SECONDS";
const DEFAULT_TICK_SECONDS: u64 = 5;
const MIN_TICK_SECONDS: u64 = 1;
const MAX_TICK_SECONDS: u64 = 60;
/// Frames a stream may hold while its client reads them; the sender waits beyond that.
const STREAM_BUFFER: usize = 16;

#[tonic::async_trait]
impl AttentionService for SessionService {
    type StreamAttentionStream = ReceiverStream<Result<StreamAttentionResponse, Status>>;

    async fn list_attention(
        &self,
        request: Request<ListAttentionRequest>,
    ) -> Result<Response<ListAttentionResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<ListAttentionResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.org_id.trim().is_empty() || req.user_id.trim().is_empty() {
                return Err(Status::invalid_argument("org_id and user_id are required"));
            }
            caller.authorize_org(&req.org_id)?;
            caller.authorize_user(&req.user_id)?;
            if !attention::enabled() {
                return Ok(Response::new(ListAttentionResponse {
                    enabled: false,
                    items: Vec::new(),
                    counts: None,
                    ..Default::default()
                }));
            }
            let mut page = attention::list(
                &self.pool,
                &req.org_id,
                &req.user_id,
                attention::page_size(req.limit),
            )
            .await?;
            page.stream_enabled = stream_enabled();
            Ok(Response::new(page))
        }
        .await;
        record_metrics("list_attention", started, result.is_ok());
        result
    }

    async fn mark_thread_seen(
        &self,
        request: Request<MarkThreadSeenRequest>,
    ) -> Result<Response<MarkThreadSeenResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<MarkThreadSeenResponse>, Status> = async {
            let caller = identity(&request)?;
            let req = request.into_inner();
            if req.org_id.trim().is_empty()
                || req.user_id.trim().is_empty()
                || req.thread_id.trim().is_empty()
            {
                return Err(Status::invalid_argument(
                    "org_id, user_id and thread_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            caller.authorize_user(&req.user_id)?;
            if !attention::enabled() {
                return Err(Status::failed_precondition("attention_disabled"));
            }
            // A zero-retention or waived caller keeps its read state on the client (WS16 S1).
            if !attention::mark_seen_permitted(RunRetention::from_caller(&caller).class) {
                return Err(Refusal::DURABLE_MUTATION_FORBIDDEN
                    .grpc_status(tonic::Code::FailedPrecondition));
            }
            // Marking read is UpdateThreadPresentation with unread false, so the owner check and the
            // presentation event are the ones every read-state change already has.
            let mut presentation = Request::new(UpdateThreadPresentationRequest {
                org_id: req.org_id.clone(),
                thread_id: req.thread_id.clone(),
                unread: Some(false),
                ..Default::default()
            });
            presentation.extensions_mut().insert(caller);
            SessionCore::update_thread_presentation(self, presentation).await?;
            Ok(Response::new(MarkThreadSeenResponse {
                thread_id: req.thread_id,
            }))
        }
        .await;
        record_metrics("mark_thread_seen", started, result.is_ok());
        result
    }

    async fn stream_attention(
        &self,
        request: Request<StreamAttentionRequest>,
    ) -> Result<Response<Self::StreamAttentionStream>, Status> {
        let started = Instant::now();
        let result: Result<Response<Self::StreamAttentionStream>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.org_id.trim().is_empty() || req.user_id.trim().is_empty() {
                return Err(Status::invalid_argument("org_id and user_id are required"));
            }
            caller.authorize_org(&req.org_id)?;
            caller.authorize_user(&req.user_id)?;
            // The live view of a thread is read state too, so it takes the posture gate of
            // MarkThreadSeen: a zero-retention or waived caller keeps its attention on the client.
            if !attention::mark_seen_permitted(RunRetention::from_caller(&caller).class) {
                return Err(Status::failed_precondition(
                    "attention_stream_not_permitted",
                ));
            }
            if !attention::enabled() || !stream_enabled() {
                return Err(Status::failed_precondition("attention_stream_disabled"));
            }
            let permit = STREAMS
                .acquire(&req.org_id, &req.user_id)
                .ok_or_else(|| Status::resource_exhausted("attention_stream_limit"))?;
            let (tx, rx) = mpsc::channel(STREAM_BUFFER);
            tokio::spawn(run_stream(
                self.pool.clone(),
                req.org_id,
                req.user_id,
                permit,
                tx,
            ));
            Ok(Response::new(ReceiverStream::new(rx)))
        }
        .await;
        record_metrics("stream_attention", started, result.is_ok());
        result
    }

    /// Whether one waiting item still waits on its owner (F-f05-03). A service asks it before it announces the
    /// item. Only the two answers leave: the RPC writes nothing and names no content.
    async fn confirm_attention_waiting(
        &self,
        request: Request<ConfirmAttentionWaitingRequest>,
    ) -> Result<Response<ConfirmAttentionWaitingResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<ConfirmAttentionWaitingResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            let state = confirm_admission(&caller, &req)?;
            if !attention::enabled() {
                metrics::counter!("mp_session_attention_confirm_total", "outcome" => "disabled")
                    .increment(1);
                return Ok(Response::new(switched_off()));
            }
            let query = ConfirmQuery {
                org_id: caller.org_id(),
                user_id: &req.user_id,
                thread_id: &req.thread_id,
                run_id: &req.run_id,
                source_id: &req.source_id,
                state,
            };
            let waiting = attention_events::still_waiting(&self.pool, &query)
                .await
                .map_err(|_| {
                    // The read error names no row and no text. An answer is never guessed, so the caller retries.
                    tracing::warn!(
                        run_id = %req.run_id,
                        thread_id = %req.thread_id,
                        "attention waiting confirmation failed"
                    );
                    Status::internal("attention waiting confirmation failed")
                })?;
            let outcome = if waiting { "waiting" } else { "not_waiting" };
            metrics::counter!("mp_session_attention_confirm_total", "outcome" => outcome)
                .increment(1);
            Ok(Response::new(ConfirmAttentionWaitingResponse {
                enabled: true,
                waiting,
            }))
        }
        .await;
        record_metrics("confirm_attention_waiting", started, result.is_ok());
        result
    }
}

impl Keyed for AttentionItem {
    fn key(&self) -> &str {
        &self.thread_id
    }
}

/// The answer of a confirm while the attention switch is off: no item is known to wait.
fn switched_off() -> ConfirmAttentionWaitingResponse {
    ConfirmAttentionWaitingResponse {
        enabled: false,
        waiting: false,
    }
}

/// The admission of a confirm (F-f05-03): the request names every identifier and a state that can be announced,
/// the org is the caller's, and the caller is a service. A person reads their own items through `ListAttention`,
/// so a user is refused here whatever their org. Returns the state the item was announced in. Pure, so each
/// refusal is tested without a database.
#[allow(clippy::result_large_err)]
fn confirm_admission(
    caller: &VerifiedIdentity,
    req: &ConfirmAttentionWaitingRequest,
) -> Result<AttentionState, Status> {
    let state = match AttentionState::try_from(req.state) {
        Ok(AttentionState::Unspecified) => {
            return Err(Status::invalid_argument("state is required"));
        }
        Ok(state) => state,
        Err(_) => {
            return Err(Status::invalid_argument(
                "state is not a known attention state",
            ));
        }
    };
    let named = [
        &req.org_id,
        &req.user_id,
        &req.thread_id,
        &req.run_id,
        &req.source_id,
    ];
    if named.iter().any(|value| value.trim().is_empty()) {
        return Err(Status::invalid_argument(
            "org_id, user_id, thread_id, run_id and source_id are required",
        ));
    }
    caller.authorize_org(&req.org_id)?;
    if !caller.is_service() {
        return Err(Status::permission_denied(
            "only a service may confirm an attention item",
        ));
    }
    Ok(state)
}

/// Whether the attention stream is served on this replica.
fn stream_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(STREAM_ENABLED_ENV).ok().as_deref())
}

/// How often a live stream reads the owner's threads, from the environment.
fn tick_interval() -> Duration {
    tick_from(std::env::var(TICK_ENV).ok().as_deref())
}

/// The tick a setting asks for: five seconds when it is unset or unreadable, and between one and sixty
/// seconds otherwise.
fn tick_from(value: Option<&str>) -> Duration {
    let seconds = value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_TICK_SECONDS)
        .clamp(MIN_TICK_SECONDS, MAX_TICK_SECONDS);
    Duration::from_secs(seconds)
}

/// Runs one stream: a read every tick and a heartbeat every [`attention_hub::HEARTBEAT`], sending the
/// frames they produce, until the client goes or a send fails. The permit is held for the whole run.
async fn run_stream(
    pool: PgPool,
    org_id: String,
    user_id: String,
    _permit: StreamPermit,
    tx: mpsc::Sender<Result<StreamAttentionResponse, Status>>,
) {
    let mut sequencer = Sequencer::new(&attention_hub::EPOCH);
    let mut previous = None;
    let mut tick = tokio::time::interval(tick_interval());
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // The heartbeat starts a full period late, so the snapshot is always the first frame.
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + attention_hub::HEARTBEAT,
        attention_hub::HEARTBEAT,
    );
    loop {
        let bodies = tokio::select! {
            _ = tx.closed() => return,
            _ = tick.tick() => read_bodies(&pool, &org_id, &user_id, &mut previous).await,
            _ = heartbeat.tick() => vec![Body::Heartbeat],
        };
        for body in bodies {
            if tx.send(Ok(to_proto(sequencer.stamp(body)))).await.is_err() {
                return;
            }
        }
    }
}

/// The bodies for one tick. A read that fails tells the client to discard what it holds; the next
/// good read starts again from a snapshot.
async fn read_bodies(
    pool: &PgPool,
    org_id: &str,
    user_id: &str,
    previous: &mut Option<Listing<AttentionItem, AttentionCounts>>,
) -> Vec<Body<AttentionItem, AttentionCounts>> {
    match attention::list(pool, org_id, user_id, attention::MAX_PAGE).await {
        Ok(page) => {
            let next = Listing {
                items: page.items,
                counts: page.counts.unwrap_or_default(),
            };
            let bodies = attention_hub::diff(previous.as_ref(), next.clone());
            *previous = Some(next);
            bodies
        }
        Err(status) => {
            tracing::warn!(code = ?status.code(), "attention stream read failed");
            *previous = None;
            vec![Body::Resync]
        }
    }
}

/// The wire form of one frame. A thread that left the listing is sent as an idle item, the one state the
/// listing itself never carries.
fn to_proto(
    frame: attention_hub::Frame<AttentionItem, AttentionCounts>,
) -> StreamAttentionResponse {
    let body = match frame.body {
        Body::Snapshot(listing) => StreamFrame::Snapshot(AttentionSnapshot {
            items: listing.items,
            counts: Some(listing.counts),
        }),
        Body::Changed { change, counts } => StreamFrame::ThreadChanged(AttentionThreadChanged {
            item: Some(match change {
                Change::Upsert(item) => item,
                Change::Remove(thread_id) => AttentionItem {
                    thread_id,
                    state: AttentionState::Idle as i32,
                    ..Default::default()
                },
            }),
            counts: Some(counts),
        }),
        Body::Resync => StreamFrame::Resync(AttentionResync {}),
        Body::Heartbeat => StreamFrame::Heartbeat(AttentionHeartbeat {}),
    };
    StreamAttentionResponse {
        epoch: frame.epoch,
        seq: frame.seq,
        frame: Some(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tick_defaults_to_five_seconds_and_stays_between_one_and_sixty() {
        assert_eq!(tick_from(None), Duration::from_secs(5));
        assert_eq!(tick_from(Some("soon")), Duration::from_secs(5));
        assert_eq!(tick_from(Some("0")), Duration::from_secs(1));
        assert_eq!(tick_from(Some(" 7 ")), Duration::from_secs(7));
        assert_eq!(tick_from(Some("600")), Duration::from_secs(60));
    }

    #[test]
    fn a_removed_thread_is_sent_as_an_idle_item_in_a_changed_frame() {
        let frame = to_proto(Sequencer::new("epoch-a").stamp(Body::Changed {
            change: Change::Remove("thread-1".to_owned()),
            counts: AttentionCounts::default(),
        }));
        assert_eq!((frame.epoch.as_str(), frame.seq), ("epoch-a", 1));
        let Some(StreamFrame::ThreadChanged(changed)) = frame.frame else {
            panic!("a removal is a changed frame");
        };
        let item = changed.item.expect("the removed item");
        assert_eq!(item.thread_id, "thread-1");
        assert_eq!(item.state, AttentionState::Idle as i32);
    }

    #[test]
    fn a_snapshot_keeps_the_listing_order_and_the_counts() {
        let counts = AttentionCounts {
            needs_you: 1,
            working: 2,
            unseen: 3,
        };
        let frame = to_proto(Sequencer::new("epoch-b").stamp(Body::Snapshot(Listing {
            items: vec![AttentionItem {
                thread_id: "thread-2".to_owned(),
                ..Default::default()
            }],
            counts: counts.clone(),
        })));
        let Some(StreamFrame::Snapshot(snapshot)) = frame.frame else {
            panic!("the first frame is a snapshot");
        };
        assert_eq!(snapshot.items[0].thread_id, "thread-2");
        assert_eq!(snapshot.counts, Some(counts));
    }

    fn confirm_request() -> ConfirmAttentionWaitingRequest {
        ConfirmAttentionWaitingRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            run_id: "run-1".to_owned(),
            source_id: "apr-1".to_owned(),
            state: AttentionState::NeedsApproval as i32,
        }
    }

    #[test]
    fn a_service_with_session_read_confirms_an_item_of_its_own_org() {
        let caller = VerifiedIdentity::service_for_test("org-1", &["session:read"], false);
        assert_eq!(
            confirm_admission(&caller, &confirm_request()).ok(),
            Some(AttentionState::NeedsApproval)
        );
    }

    #[test]
    fn a_user_is_refused_because_only_a_service_may_confirm() {
        let caller = VerifiedIdentity::user_for_test("org-1", "user-1");
        let status = confirm_admission(&caller, &confirm_request())
            .expect_err("a person reads through ListAttention");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn another_org_is_refused_even_for_a_service() {
        let caller = VerifiedIdentity::service_for_test("org-2", &["session:read"], false);
        let status = confirm_admission(&caller, &confirm_request())
            .expect_err("the org must be the caller's");
        assert_eq!(status.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn every_identifier_is_required_and_a_blank_one_is_invalid() {
        let caller = VerifiedIdentity::service_for_test("org-1", &["session:read"], false);
        let blank: [fn(&mut ConfirmAttentionWaitingRequest); 5] = [
            |req: &mut ConfirmAttentionWaitingRequest| req.org_id = " ".to_owned(),
            |req: &mut ConfirmAttentionWaitingRequest| req.user_id = String::new(),
            |req: &mut ConfirmAttentionWaitingRequest| req.thread_id = String::new(),
            |req: &mut ConfirmAttentionWaitingRequest| req.run_id = String::new(),
            |req: &mut ConfirmAttentionWaitingRequest| req.source_id = "\t".to_owned(),
        ];
        for clear in blank {
            let mut req = confirm_request();
            clear(&mut req);
            let status = confirm_admission(&caller, &req).expect_err("a blank id is refused");
            assert_eq!(status.code(), tonic::Code::InvalidArgument);
        }
    }

    #[test]
    fn the_state_must_be_one_the_contract_names() {
        let caller = VerifiedIdentity::service_for_test("org-1", &["session:read"], false);
        let mut req = confirm_request();
        req.state = AttentionState::Unspecified as i32;
        let status = confirm_admission(&caller, &req).expect_err("a state is required");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        req.state = 99;
        let status = confirm_admission(&caller, &req).expect_err("an unknown state is refused");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        for state in [
            AttentionState::NeedsInput,
            AttentionState::PlanReady,
            AttentionState::Completed,
        ] {
            req.state = state as i32;
            assert!(confirm_admission(&caller, &req).is_ok(), "{state:?}");
        }
    }

    #[test]
    fn the_switched_off_answer_says_enabled_false_and_waiting_false() {
        assert_eq!(
            switched_off(),
            ConfirmAttentionWaitingResponse {
                enabled: false,
                waiting: false,
            }
        );
    }
}
