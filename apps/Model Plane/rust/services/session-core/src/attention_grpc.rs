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
    ListAttentionRequest, ListAttentionResponse, MarkThreadSeenRequest, MarkThreadSeenResponse,
    StreamAttentionRequest, StreamAttentionResponse, UpdateThreadPresentationRequest,
};
use mp_contracts::zdr_refusal::Refusal;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::attention;
use crate::attention_hub::{self, Body, Change, Keyed, Listing, Sequencer, StreamPermit, STREAMS};
use crate::auth::{authorize_operation, identity};
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
}

impl Keyed for AttentionItem {
    fn key(&self) -> &str {
        &self.thread_id
    }
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
}
