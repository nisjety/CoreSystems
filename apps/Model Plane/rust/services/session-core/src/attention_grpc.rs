//! The `AttentionService` surface of Session Core (build plan WS16 S1): the closed attention state of
//! the caller's threads, and the mark-read that clears a thread's unseen marker.
//!
//! Both answers are owner-bound. The caller's verified identity must be the user named in the request,
//! so a service principal cannot read or mark another person's threads.

use std::time::Instant;

use mp_contracts::model_plane::v1::{
    attention_service_server::AttentionService, session_core_server::SessionCore,
    ListAttentionRequest, ListAttentionResponse, MarkThreadSeenRequest, MarkThreadSeenResponse,
    StreamAttentionRequest, StreamAttentionResponse, UpdateThreadPresentationRequest,
};
use mp_contracts::zdr_refusal::Refusal;
use tonic::{Request, Response, Status};

use crate::attention;
use crate::auth::{authorize_operation, identity};
use crate::grpc::{record_metrics, SessionService};
use crate::retention_receipt::RunRetention;

#[tonic::async_trait]
impl AttentionService for SessionService {
    type StreamAttentionStream = futures::stream::Empty<Result<StreamAttentionResponse, Status>>;

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
                }));
            }
            let page = attention::list(
                &self.pool,
                &req.org_id,
                &req.user_id,
                attention::page_size(req.limit),
            )
            .await?;
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
        _request: Request<StreamAttentionRequest>,
    ) -> Result<Response<Self::StreamAttentionStream>, Status> {
        Err(Status::unimplemented(
            "the attention stream is served from WS16 S5",
        ))
    }
}
