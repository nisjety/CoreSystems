//! The organisation overlay on the routes the caller does not name (GAP-W4-BYO-hidden-infer step 2, round 3 of
//! the verification): the intent layer, the tool ladder and the role table, with `INFERENCE_ORG_OVERLAY=true` and
//! the platform overlay off.
//!
//! The switch is read once per process, so this binary turns it on before the first chain call. Only the outside
//! world is a double: session-core is a local `RoutingPolicy` server that answers each organisation's overlay (and
//! refuses the platform policy, so the chain keeps its built-in table), and the Azure `OpenAI` resource is a mock
//! whose request log names every deployment a provider was asked for. The chain is the real one, built by
//! `FallbackChain::from_config` with the intent layer on.
//!
//! The resource deploys `gpt-4o-mini`, `gpt-5-mini` and `gpt-5-nano`. The built-in table resolves a simple
//! `verevon-balance` turn to `gpt-4o-mini`; a tool turn floors to complex (`claude-sonnet-4-6`, which this resource
//! cannot serve), so its tool ladder walks to `gpt-5-mini` and then `gpt-4o-mini`.
//!
//! - `org-hides-mini` hides `gpt-4o-mini`.
//! - `org-hides-5` hides `gpt-5-mini`.
//! - `org-open` hides nothing (the control: it proves each route really reaches the model the others hide).

use std::sync::Once;

use inference_core::config::InferenceConfig;
use inference_core::provider::fallback::FallbackChain;
use inference_core::provider::role::{RoleCall, ROLE_MODEL_ID};
use inference_core::provider::routing_policy::RolePurpose;
use inference_core::provider::{ChatMessage, InferRequest, ProviderError, ToolDefinition};
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

const MINI: &str = "gpt-4o-mini";
const FIVE: &str = "gpt-5-mini";
const NANO: &str = "gpt-5-nano";

fn switch_on() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("INFERENCE_ORG_OVERLAY", "true"));
}

struct SessionCore;

#[tonic::async_trait]
impl RoutingPolicy for SessionCore {
    async fn get_policy(
        &self,
        _: Request<GetRoutingPolicyRequest>,
    ) -> Result<Response<RoutingPolicyMessage>, Status> {
        // No stored platform policy: the chain keeps its built-in table.
        Err(Status::not_found("no policy"))
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
        let config_json = match org_id.as_str() {
            "org-hides-mini" => json!({ "hidden": [MINI] }).to_string(),
            "org-hides-5" => json!({ "hidden": [FIVE] }).to_string(),
            _ => "{}".to_owned(),
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
            "model": "served",
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
        azure_openai_chat_deployments: vec![MINI.to_owned(), FIVE.to_owned(), NANO.to_owned()],
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
        verevon_intent_enabled: true,
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

fn request(org_id: &str, user_id: &str, model: &str, with_tools: bool) -> InferRequest {
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
        tools: if with_tools {
            vec![ToolDefinition {
                name: "lookup".to_owned(),
                description: "look something up".to_owned(),
                parameters_json: r#"{"type":"object","properties":{}}"#.to_owned(),
            }]
        } else {
            vec![]
        },
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

/// The deployment of every chat call the resource received, in order.
async fn deployments_asked(azure: &MockServer) -> Vec<String> {
    azure
        .received_requests()
        .await
        .expect("recorded")
        .iter()
        .filter_map(|request| {
            request
                .url
                .path()
                .strip_prefix("/openai/deployments/")
                .and_then(|rest| rest.split('/').next())
                .map(str::to_owned)
        })
        .collect()
}

// ---------- the intent layer ----------

/// Control: a simple `verevon-balance` turn resolves to `gpt-4o-mini` and is served there.
#[tokio::test]
async fn the_intent_layer_serves_its_choice_to_an_org_that_hides_nothing() {
    let (chain, azure) = chain().await;
    chain
        .infer(&request("org-open", "i1", "verevon-balance", false))
        .await
        .expect("served");
    assert_eq!(deployments_asked(&azure).await, [MINI]);
}

#[tokio::test]
async fn the_intent_layer_never_sends_a_model_the_org_hides() {
    let (chain, azure) = chain().await;
    let outcome = chain
        .infer(&request("org-hides-mini", "i2", "verevon-balance", false))
        .await;
    let asked = deployments_asked(&azure).await;
    assert!(
        !asked.iter().any(|d| d.eq_ignore_ascii_case(MINI)),
        "the intent layer sent the hidden model: {asked:?} ({outcome:?})"
    );
    if let Err(error) = &outcome {
        assert!(
            matches!(error, ProviderError::ModelHidden { model } if model == "verevon-balance"),
            "a refusal names the id the caller sent: {error:?}"
        );
    }
}

#[tokio::test]
async fn the_intent_layer_never_opens_a_stream_on_a_model_the_org_hides() {
    let (chain, azure) = chain().await;
    let outcome = chain
        .infer_stream(&request("org-hides-mini", "i3", "verevon-balance", false))
        .await;
    if let Ok(mut rx) = outcome {
        while rx.recv().await.is_some() {}
    }
    let asked = deployments_asked(&azure).await;
    assert!(
        !asked.iter().any(|d| d.eq_ignore_ascii_case(MINI)),
        "the intent layer streamed from the hidden model: {asked:?}"
    );
}

// ---------- the tool ladder ----------

/// Control: a tool turn floors to a Claude model this resource cannot serve, so the ladder's first servable rung
/// (`gpt-5-mini`) serves it.
#[tokio::test]
async fn the_tool_ladder_serves_its_first_rung_to_an_org_that_hides_nothing() {
    let (chain, azure) = chain().await;
    chain
        .infer(&request("org-open", "l1", "verevon-balance", true))
        .await
        .expect("served");
    assert_eq!(deployments_asked(&azure).await, [FIVE]);
}

#[tokio::test]
async fn the_tool_ladder_skips_the_rung_the_org_hides_and_serves_the_next() {
    let (chain, azure) = chain().await;
    chain
        .infer(&request("org-hides-5", "l2", "verevon-balance", true))
        .await
        .expect("the next rung serves");
    assert_eq!(deployments_asked(&azure).await, [MINI]);
}

#[tokio::test]
async fn the_streamed_tool_ladder_skips_the_rung_the_org_hides() {
    let (chain, azure) = chain().await;
    let mut rx = chain
        .infer_stream(&request("org-hides-5", "l3", "verevon-balance", true))
        .await
        .expect("the next rung streams");
    while rx.recv().await.is_some() {}
    assert_eq!(deployments_asked(&azure).await, [MINI]);
}

// ---------- the role table ----------

fn role_request(org_id: &str, user_id: &str) -> InferRequest {
    request(org_id, user_id, ROLE_MODEL_ID, false)
}

fn research_plan(turn_model: &str) -> RoleCall {
    RoleCall {
        purpose: Some(RolePurpose::ResearchPlan),
        fallback_model: turn_model.to_owned(),
    }
}

/// Control: with no table entry, the turn's own model serves the side task.
#[tokio::test]
async fn the_role_table_serves_the_turn_model_to_an_org_that_hides_nothing() {
    let (chain, azure) = chain().await;
    chain
        .infer_role_noted(&role_request("org-open", "r1"), &research_plan(FIVE), false)
        .await
        .expect("served");
    assert_eq!(deployments_asked(&azure).await, [FIVE]);
}

#[tokio::test]
async fn the_role_table_never_serves_a_turn_model_the_org_hides() {
    let (chain, azure) = chain().await;
    let outcome = chain
        .infer_role_noted(
            &role_request("org-hides-5", "r2"),
            &research_plan(FIVE),
            false,
        )
        .await;
    let asked = deployments_asked(&azure).await;
    assert!(
        !asked.iter().any(|d| d.eq_ignore_ascii_case(FIVE)),
        "the role table served the hidden turn model: {asked:?} ({outcome:?})"
    );
}

/// The role table's last resort (the policy's cheap fallback, `gpt-4o-mini`) is not asked of the admission check.
/// When the organisation hides it, nothing may be sent to it.
#[tokio::test]
async fn the_role_tables_cheap_fallback_is_never_sent_when_the_org_hides_it() {
    let (chain, azure) = chain().await;
    let outcome = chain
        .infer_role_noted(
            &role_request("org-hides-mini", "r3"),
            &research_plan(MINI),
            false,
        )
        .await;
    let asked = deployments_asked(&azure).await;
    assert!(
        !asked.iter().any(|d| d.eq_ignore_ascii_case(MINI)),
        "the role table's fallback sent the hidden model: {asked:?} ({outcome:?})"
    );
}
