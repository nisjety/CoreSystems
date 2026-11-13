//! The organisation overlay refuses a hidden model on infer (GAP-W4-BYO-hidden-infer step 2), with
//! `INFERENCE_ORG_OVERLAY=true` and the platform overlay off.
//!
//! The switch is read once per process, so this binary turns it on before the first chain call. Only the outside
//! world is a double: session-core is a local `RoutingPolicy` server that answers each organisation's overlay, and
//! the Azure `OpenAI` resource is a mock whose request log proves no provider was called. The chain is the real one,
//! built by `FallbackChain::from_config`.
//!
//! The organisation `org-hides` hides `gpt-4o-mini`, the only model the resource serves. The organisation `org-open`
//! hides nothing.

use std::sync::Once;

use inference_core::config::InferenceConfig;
use inference_core::provider::fallback::{FallbackChain, HidingOverlay};
use inference_core::provider::{ChatMessage, InferRequest, ProviderError};
use mp_contracts::model_plane::v1::routing_policy_server::{RoutingPolicy, RoutingPolicyServer};
use mp_contracts::model_plane::v1::{
    GetOrgRoutingOverlayRequest, GetRoutingPolicyRequest, ListCandidatesRequest,
    ListCandidatesResponse, OrgRoutingOverlayMessage, PromoteCandidateRequest,
    PromoteCandidateResponse, ProposeCandidateRequest, ProposeCandidateResponse,
    RecordCandidateEvaluationRequest, RecordCandidateEvaluationResponse, RoutingPolicyMessage,
    SetOrgRoutingOverlayRequest, SetRoutingPolicyRequest,
};
use serde_json::json;
use tonic::{Request, Response, Status};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HIDDEN: &str = "gpt-4o-mini";

fn switch_on() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("INFERENCE_ORG_OVERLAY", "true"));
}

/// session-core: the platform policy is never set, and `org-hides` hides the served model.
struct SessionCore;

#[tonic::async_trait]
impl RoutingPolicy for SessionCore {
    async fn get_policy(
        &self,
        _: Request<GetRoutingPolicyRequest>,
    ) -> Result<Response<RoutingPolicyMessage>, Status> {
        Ok(Response::new(RoutingPolicyMessage {
            config_json: "{}".to_owned(),
            ..Default::default()
        }))
    }

    async fn set_policy(
        &self,
        _: Request<SetRoutingPolicyRequest>,
    ) -> Result<Response<RoutingPolicyMessage>, Status> {
        Err(Status::unimplemented("not used"))
    }

    async fn propose_candidate(
        &self,
        _: Request<ProposeCandidateRequest>,
    ) -> Result<Response<ProposeCandidateResponse>, Status> {
        Err(Status::unimplemented("not used"))
    }

    async fn record_candidate_evaluation(
        &self,
        _: Request<RecordCandidateEvaluationRequest>,
    ) -> Result<Response<RecordCandidateEvaluationResponse>, Status> {
        Err(Status::unimplemented("not used"))
    }

    async fn promote_candidate(
        &self,
        _: Request<PromoteCandidateRequest>,
    ) -> Result<Response<PromoteCandidateResponse>, Status> {
        Err(Status::unimplemented("not used"))
    }

    async fn list_candidates(
        &self,
        _: Request<ListCandidatesRequest>,
    ) -> Result<Response<ListCandidatesResponse>, Status> {
        Err(Status::unimplemented("not used"))
    }

    async fn get_org_routing_overlay(
        &self,
        request: Request<GetOrgRoutingOverlayRequest>,
    ) -> Result<Response<OrgRoutingOverlayMessage>, Status> {
        let org_id = request.into_inner().org_id;
        let config_json = if org_id == "org-hides" {
            json!({ "hidden": [HIDDEN] }).to_string()
        } else {
            "{}".to_owned()
        };
        Ok(Response::new(OrgRoutingOverlayMessage {
            org_id,
            config_json,
            version: 1,
            ..Default::default()
        }))
    }

    async fn set_org_routing_overlay(
        &self,
        _: Request<SetOrgRoutingOverlayRequest>,
    ) -> Result<Response<OrgRoutingOverlayMessage>, Status> {
        Err(Status::unimplemented("not used"))
    }
}

async fn session_core() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("grpc port");
    let address = listener.local_addr().expect("grpc address");
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(RoutingPolicyServer::new(SessionCore))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener)),
    );
    format!("http://{address}")
}

async fn azure() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/openai/deployments/[^/]+/chat/completions$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-1",
            "model": "gpt-4o-mini-2024-07-18",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3 }
        })))
        .mount(&server)
        .await;
    server
}

fn config(azure_endpoint: &str, session_core_url: &str) -> InferenceConfig {
    InferenceConfig {
        provider_order: vec!["azure".to_owned()],
        anthropic_api_key: None,
        openai_api_base: None,
        openai_api_key: None,
        codex_subscription_integration_core_url: None,
        codex_subscription_internal_api_key: None,
        codex_subscription_models: vec![],
        byo_gateway_enabled: false,
        provider_benching: false,
        same_tier_fallback: false,
        customer_gateway_internal_api_key: None,
        azure_openai_endpoint: Some(azure_endpoint.to_owned()),
        azure_openai_api_key: Some("test-key".to_owned()),
        azure_openai_api_version: "2025-01-01-preview".to_owned(),
        openai_chat_models: vec![],
        openai_embedding_models: vec![],
        azure_openai_chat_deployments: vec![HIDDEN.to_owned()],
        azure_openai_embedding_deployments: vec![],
        azure_anthropic_endpoint: None,
        azure_anthropic_api_key: None,
        azure_anthropic_deployments: vec![],
        azure_cohere_endpoint: None,
        azure_cohere_api_key: None,
        azure_cohere_api_version: "2024-05-01-preview".to_owned(),
        azure_cohere_deployment: "cohere-command-a-plus".to_owned(),
        azure_cohere_region: None,
        max_retries_per_provider: 1,
        cache_ttl_secs: 60,
        verevon_intent_enabled: false,
        cost_core_url: None,
        verevon_intent_budget_usd: 50.0,
        session_core_url: Some(session_core_url.to_owned()),
        router_policy_refresh_secs: 3600,
        azure_openai_region: None,
        azure_anthropic_region: None,
        azure_openai_zdr: None,
        azure_anthropic_zdr: None,
        azure_openai_deployment_type: None,
        allow_global_deployment: true,
        allow_global_residency_providers: true,
        allow_non_eu_embedding: false,
    }
}

fn request(org_id: &str, user_id: &str, model: &str) -> InferRequest {
    InferRequest {
        request_id: format!("r-{user_id}"),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        model: model.to_owned(),
        messages: vec![ChatMessage {
            compaction_summary: String::new(),
            role: "user".to_owned(),
            content: "hello".to_owned(),
            name: String::new(),
        }],
        ..InferRequest::default()
    }
}

async fn chain() -> (FallbackChain, MockServer) {
    switch_on();
    let azure = azure().await;
    let url = session_core().await;
    (
        FallbackChain::from_config(&config(&azure.uri(), &url)),
        azure,
    )
}

async fn provider_calls(azure: &MockServer) -> usize {
    azure.received_requests().await.expect("recorded").len()
}

#[tokio::test]
async fn a_named_model_the_org_hides_is_refused_before_any_provider() {
    let (chain, azure) = chain().await;
    let refused = chain.infer(&request("org-hides", "u1", HIDDEN)).await;
    assert!(
        matches!(&refused, Err(ProviderError::ModelHidden { model }) if model == HIDDEN),
        "{refused:?}"
    );
    // Any case of the id is the same model.
    let refused = chain
        .infer(&request("org-hides", "u2", "GPT-4o-Mini"))
        .await;
    assert!(
        matches!(refused, Err(ProviderError::ModelHidden { .. })),
        "{refused:?}"
    );
    assert_eq!(provider_calls(&azure).await, 0, "no provider was asked");
}

#[tokio::test]
async fn the_same_model_is_served_to_an_org_that_does_not_hide_it() {
    let (chain, azure) = chain().await;
    let served = chain
        .infer(&request("org-open", "u3", HIDDEN))
        .await
        .expect("served");
    assert_eq!(served.content, "ok");
    assert_eq!(provider_calls(&azure).await, 1);
}

#[tokio::test]
async fn a_streamed_named_model_the_org_hides_is_refused_before_any_provider() {
    let (chain, azure) = chain().await;
    let refused = chain
        .infer_stream(&request("org-hides", "u4", HIDDEN))
        .await;
    assert!(
        matches!(refused, Err(ProviderError::ModelHidden { .. })),
        "{:?}",
        refused.err()
    );
    assert_eq!(provider_calls(&azure).await, 0);
}

/// An automatic call names no model; the resource's default would serve it. That default is the model the
/// organisation hides, so it must not be served (the platform overlay refuses the same case, V2-inf-1 sibling).
#[tokio::test]
async fn an_automatic_call_is_never_served_by_a_model_the_org_hides() {
    let (chain, azure) = chain().await;
    let outcome = chain.infer(&request("org-hides", "u5", "")).await;
    assert!(
        outcome.as_ref().map_or(true, |served| !served
            .model_used
            .eq_ignore_ascii_case(HIDDEN)
            && !served.model_used.starts_with(HIDDEN)),
        "served by a model the organisation hides: {outcome:?}"
    );
    assert_eq!(
        provider_calls(&azure).await,
        0,
        "the hidden default was sent to the provider"
    );
}

/// The stream path refuses an automatic call whose provider default the organisation hides, before any stream is
/// opened: the walk judges the default it would really serve, as the unary path does.
#[tokio::test]
async fn an_automatic_stream_is_never_opened_on_a_model_the_org_hides() {
    let (chain, azure) = chain().await;
    let refused = chain.infer_stream(&request("org-hides", "u6", "")).await;
    assert!(
        matches!(refused, Err(ProviderError::ModelHidden { .. })),
        "{:?}",
        refused.err()
    );
    assert_eq!(
        provider_calls(&azure).await,
        0,
        "the hidden default was opened on the stream path"
    );
}

/// An automatic call is still served the provider default to an organisation that hides nothing: the overlay narrows
/// only what it hides.
#[tokio::test]
async fn an_automatic_call_is_still_served_to_an_org_that_hides_nothing() {
    let (chain, azure) = chain().await;
    let served = chain
        .infer(&request("org-open", "u7", ""))
        .await
        .expect("served");
    assert_eq!(served.content, "ok");
    assert_eq!(served.model_used, HIDDEN);
    assert_eq!(provider_calls(&azure).await, 1);
}

/// The access verdict reads the organisation's overlay through `hiding_overlay` (V3-inf-1): the model `org-hides`
/// hides is reported as hidden in any case, and the organisation that hides nothing is not. An automatic call names no
/// model, so the verdict leaves it to the walk, which judges the default it resolves to.
#[tokio::test]
async fn the_access_verdict_reports_the_model_the_org_hides_and_no_other() {
    let (chain, _azure) = chain().await;
    assert_eq!(
        chain.hiding_overlay("org-hides", HIDDEN).await,
        Some(HidingOverlay::Organisation)
    );
    assert_eq!(
        chain.hiding_overlay("org-hides", "GPT-4o-Mini").await,
        Some(HidingOverlay::Organisation)
    );
    assert_eq!(chain.hiding_overlay("org-open", HIDDEN).await, None);
    assert_eq!(chain.hiding_overlay("org-hides", "").await, None);
}
