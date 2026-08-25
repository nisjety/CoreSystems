//! Contract-level tests: serde shape + ID roundtrip + envelope semantics.

use quarry_core::artifact::{self, ArtifactKind};
use quarry_core::cache::{CacheMode, CachePolicy};
use quarry_core::contracts::{
    ActionOutcome, ActionOutcomeStatus, AgentAction, AgentActionRequest, AgentConstraints,
    BrowserObservation, ChunkRef, DataPlaneIngestRequest, DataPlaneIngestResponse, DomSummary,
    EmbeddingStatus, ExtractionUsage, IndexStatus, InteractiveElement, BrowserTelemetry, SemanticLocator,
    SourceTrace, StructuredExtractRequest, StructuredExtractResponse,
};
use quarry_core::envelope::{Envelope, EnvelopeMeta};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::ids::kinds;
use quarry_core::policy::{BackoffKind, RunPolicy};
use quarry_core::zdr::ZdrMode;
use serde_json::json;

#[test]
fn run_id_roundtrip_and_prefix() {
    let id: kinds::RunKind = quarry_core::ids::Id::new();
    let s = id.to_string();
    assert!(s.starts_with("run_"), "expected run_ prefix, got {s}");
    let parsed: kinds::RunKind = s.parse().expect("parse run id");
    assert_eq!(id, parsed);
}

#[test]
fn wrong_kind_prefix_rejected() {
    let run: kinds::RunKind = quarry_core::ids::Id::new();
    let s = run.to_string();
    let as_job: Result<kinds::JobKind, _> = s.parse();
    assert!(as_job.is_err(), "run_ id must not parse as job_");
}

#[test]
fn error_code_http_status_matrix() {
    assert_eq!(ErrorCode::BadRequest.http_status(), 400);
    assert_eq!(ErrorCode::SecurityBlocked.http_status(), 403);
    assert_eq!(ErrorCode::NotFound.http_status(), 404);
    assert_eq!(ErrorCode::RateLimited.http_status(), 429);
    assert_eq!(ErrorCode::Timeout.http_status(), 504);
    assert_eq!(ErrorCode::DriverFailed.http_status(), 502);
    assert_eq!(ErrorCode::ActionUnknown.http_status(), 409);
    assert_eq!(ErrorCode::CheckpointLost.http_status(), 409);
    assert_eq!(ErrorCode::TargetRepairRequired.http_status(), 409);
    assert_eq!(ErrorCode::ChallengeDetected.http_status(), 403);
    assert_eq!(ErrorCode::RuntimeNotReady.http_status(), 503);
    assert_eq!(ErrorCode::Internal.http_status(), 500);
}

#[test]
fn error_retryable_flag_matches_semantic() {
    assert!(ErrorCode::Timeout.retryable());
    assert!(ErrorCode::RateLimited.retryable());
    assert!(!ErrorCode::SecurityBlocked.retryable());
    assert!(!ErrorCode::ActionUnknown.retryable());
    assert!(!ErrorCode::ChallengeDetected.retryable());
    assert!(!ErrorCode::RuntimeNotReady.retryable());
    assert!(!ErrorCode::BadRequest.retryable());
}

#[test]
fn envelope_ok_shape() {
    let env = Envelope::ok("req_123", json!({"hello": "world"}));
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["meta"]["request_id"], "req_123");
    assert!(v["error"].is_null());
    assert_eq!(v["data"]["hello"], "world");
}

#[test]
fn envelope_err_shape() {
    let err = QuarryError::new(ErrorCode::SecurityBlocked, "blocked host");
    let env: Envelope<serde_json::Value> = Envelope::err("req_x", err);
    let v = serde_json::to_value(&env).unwrap();
    assert_eq!(v["error"]["code"], "SECURITY_BLOCKED");
    assert_eq!(v["error"]["message"], "blocked host");
    assert_eq!(v["error"]["retryable"], false);
    assert!(v["data"].is_null());
}

#[test]
fn envelope_meta_serde_stable() {
    let m = EnvelopeMeta {
        request_id: "req_1".into(),
        page: None,
    };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["request_id"], "req_1");
    assert!(v.get("page").is_none(), "empty page must be skipped");
}

#[test]
fn default_run_policy_sane() {
    let p = RunPolicy::default();
    assert_eq!(p.concurrency.per_run, 8);
    assert_eq!(p.concurrency.per_domain, 2);
    assert!(matches!(p.retry.backoff, BackoffKind::Exp));
    assert!(p.delay.jitter);
}

#[test]
fn cache_policy_defaults() {
    let p = CachePolicy::default();
    assert_eq!(p.mode, CacheMode::ReadWrite);
    assert_eq!(p.max_age_s, 3600);
    assert!(!p.vary_on.is_empty());
}

#[test]
fn artifact_object_key_deterministic() {
    let k1 = artifact::object_key("org_a", "run_01H000", "blake3:abc", ArtifactKind::Markdown);
    let k2 = artifact::object_key("org_a", "run_01H000", "blake3:abc", ArtifactKind::Markdown);
    assert_eq!(k1, k2);
    assert!(k1.contains("org=org_a"));
    assert!(k1.contains("run=run_01H000"));
    assert!(k1.ends_with("markdown.md"));
}

#[test]
fn visual_artifact_object_keys_are_contract_named() {
    let visual = artifact::object_key(
        "org_a",
        "run_01H000",
        "blake3:abc",
        ArtifactKind::VisualObservation,
    );
    let annotated = artifact::object_key(
        "org_a",
        "run_01H000",
        "blake3:abc",
        ArtifactKind::ScreenshotAnnotated,
    );
    let tiles = artifact::object_key("org_a", "run_01H000", "blake3:abc", ArtifactKind::Tiles);
    let change = artifact::object_key(
        "org_a",
        "run_01H000",
        "blake3:abc",
        ArtifactKind::VisualChange,
    );

    assert!(visual.ends_with("visual_observation.json"));
    assert!(change.ends_with("visual_change.json"));
    assert!(annotated.ends_with("screenshot_annotated.png"));
    assert!(tiles.ends_with("tiles.json"));
}

#[test]
fn page_hash_differs_on_url_or_fingerprint() {
    let a = artifact::page_hash("https://a.com", "blake3:xxx");
    let b = artifact::page_hash("https://b.com", "blake3:xxx");
    let c = artifact::page_hash("https://a.com", "blake3:yyy");
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert!(a.starts_with("blake3:"));
}

// ---------------------------------------------------------------------------
// Cross-plane contract schema tests
// ---------------------------------------------------------------------------

#[test]
fn browser_observation_serde_roundtrip() {
    let obs = BrowserObservation {
        run_id: quarry_core::ids::Id::new(),
        step: 3,
        url: "https://example.com".into(),
        title: Some("Example".into()),
        snapshot: None,
        dom_summary: Some(DomSummary {
            node_count: 42,
            interactive_elements: vec![InteractiveElement {
                tag: "button".into(),
                selector: "#submit".into(),
                selector_alternatives: vec!["button[name=\"submit\"]".into()],
                text: Some("Submit".into()),
                role: Some("button".into()),
                aria_label: Some("Submit".into()),
                accessible_name: Some("Submit".into()),
                placeholder: None,
                test_id: None,
                fingerprint: None,
                click_index: None,
            }],
            text_snippet: Some("Hello world".into()),
            click_map: None,
        }),
        screenshot_artifact_id: None,
        visual_observation_artifact_id: None,
        evidence_delta_artifact_id: None,
        console_summary: vec![],
        network_summary: vec![],
        egress_receipts: vec![],
        dialogs: vec![],
        policy_denials: vec!["blocked: private IP".into()],
        action_outcome: ActionOutcome::unknown("not_verified", "test observation"),
        observation_delta: None,
        challenge: None,
        extraction_profile: None,
        extraction_result: None,
        proof_bundle: None,
        target_resolution: None,
        telemetry: BrowserTelemetry::default(),
        observed_at: chrono::Utc::now(),
    };
    let json = serde_json::to_string(&obs).unwrap();
    let back: BrowserObservation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.step, 3);
    assert_eq!(back.url, "https://example.com");
    assert_eq!(back.policy_denials.len(), 1);
    assert_eq!(back.action_outcome.status, ActionOutcomeStatus::Unknown);
    assert!(back.dom_summary.is_some());
}

#[test]
fn action_outcome_defaults_to_unknown_for_older_wire_payloads() {
    let payload = serde_json::json!({
        "run_id": quarry_core::ids::Id::<quarry_core::ids::RunKind>::new(),
        "step": 1,
        "url": "https://example.com",
        "observed_at": chrono::Utc::now(),
    });
    let observation: BrowserObservation = serde_json::from_value(payload).unwrap();
    assert_eq!(
        observation.action_outcome.status,
        ActionOutcomeStatus::Unknown
    );
}

#[test]
fn agent_action_request_serde_roundtrip() {
    let req = AgentActionRequest {
        run_id: quarry_core::ids::Id::new(),
        lease_id: quarry_core::ids::Id::new(),
        action: AgentAction::Click {
            selector: "#btn".into(),
        },
        instruction: Some("click the submit button".into()),
        constraints: AgentConstraints {
            max_steps: 10,
            allowed_domains: vec!["example.com".into()],
            max_runtime_s: Some(30),
            max_cost_usd: Some(0.05),
        },
        zdr: ZdrMode::Off,
        extraction_profile: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: AgentActionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.constraints.max_steps, 10);
    assert_eq!(back.zdr, ZdrMode::Off);
    match &back.action {
        AgentAction::Click { selector } => assert_eq!(selector, "#btn"),
        _ => panic!("wrong action variant"),
    }
}

#[test]
fn agent_action_all_variants_serialize() {
    let actions = vec![
        AgentAction::Navigate {
            url: "https://x.com".into(),
        },
        AgentAction::Click {
            selector: "#a".into(),
        },
        AgentAction::ClickRef {
            snapshot_id: "snap_1".into(),
            generation: 2,
            ref_id: "@e1".into(),
        },
        AgentAction::ClickSemantic {
            snapshot_id: "snap_1".into(),
            generation: 2,
            locator: SemanticLocator::Role {
                role: "button".into(),
                name: Some("Continue".into()),
                exact: true,
            },
        },
        AgentAction::ClickPoint { x: 320.0, y: 240.0 },
        AgentAction::Type {
            selector: "#i".into(),
            text: "hi".into(),
        },
        AgentAction::TypeRef {
            snapshot_id: "snap_1".into(),
            generation: 2,
            ref_id: "@e2".into(),
            text: "hi".into(),
        },
        AgentAction::TypeSemantic {
            snapshot_id: "snap_1".into(),
            generation: 2,
            locator: SemanticLocator::Placeholder {
                placeholder: "Email".into(),
                exact: false,
            },
            text: "hi".into(),
        },
        AgentAction::Press {
            key: "Enter".into(),
        },
        AgentAction::Scroll {
            target: "#footer".into(),
        },
        AgentAction::MouseWheel {
            x: 320.0,
            y: 240.0,
            delta_x: 0.0,
            delta_y: 480.0,
        },
        AgentAction::Select {
            selector: "select".into(),
            value: "opt1".into(),
        },
        AgentAction::SelectRef {
            snapshot_id: "snap_1".into(),
            generation: 2,
            ref_id: "@e3".into(),
            value: "opt1".into(),
        },
        AgentAction::SelectSemantic {
            snapshot_id: "snap_1".into(),
            generation: 2,
            locator: SemanticLocator::TestId {
                test_id: "plan".into(),
                exact: true,
            },
            value: "opt1".into(),
        },
        AgentAction::Wait { ms: 100 },
        AgentAction::WaitFor {
            selector: ".done".into(),
            timeout_ms: 5000,
        },
        AgentAction::WaitForRef {
            snapshot_id: "snap_1".into(),
            generation: 2,
            ref_id: "@e4".into(),
            timeout_ms: 5000,
        },
        AgentAction::WaitForSemantic {
            snapshot_id: "snap_1".into(),
            generation: 2,
            locator: SemanticLocator::Text {
                text: "Done".into(),
                exact: true,
            },
            timeout_ms: 5000,
        },
        AgentAction::UploadRef {
            snapshot_id: "snap_1".into(),
            generation: 2,
            ref_id: "@e5".into(),
            artifact_id: quarry_core::ids::Id::new(),
            approval_grant_id: "grant_upload_approval".into(),
        },
        AgentAction::DownloadRef {
            snapshot_id: "snap_1".into(),
            generation: 2,
            ref_id: "@e6".into(),
            approval_grant_id: "grant_download_approval".into(),
        },
        AgentAction::Screenshot { full_page: true },
        AgentAction::Pdf,
        AgentAction::Evaluate {
            script: "1+1".into(),
        },
        AgentAction::Back,
        AgentAction::Forward,
        AgentAction::GetContent,
    ];
    for action in actions {
        let json = serde_json::to_string(&action).unwrap();
        let back: AgentAction = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2, "roundtrip failed for {json}");
    }
}

#[test]
fn data_plane_ingest_request_serde_roundtrip() {
    let req = DataPlaneIngestRequest {
        run_id: quarry_core::ids::Id::new(),
        org_id: "org_test".into(),
        source_url: "https://example.com/page".into(),
        title: Some("Test Page".into()),
        markdown: Some("# Hello".into()),
        html_ref: None,
        raw_ref: None,
        chunks: vec![ChunkRef {
            start: 0,
            end: 7,
            text: "# Hello".into(),
        }],
        metadata: json!({"lang": "en"}),
        fingerprint: "blake3:abc123".into(),
        zdr: ZdrMode::Off,
        retention_policy: Some("30d".into()),
        privacy_policy: Some(quarry_core::privacy::PrivacyPolicy {
            purpose_id: Some("support".into()),
            retention_policy: Some("30d".into()),
            ..quarry_core::privacy::PrivacyPolicy::default()
        }),
        source_trace: Some(SourceTrace {
            source_url: "https://example.com/page".into(),
            fetched_at: chrono::Utc::now(),
            fingerprint: "blake3:abc123".into(),
            field_traces: vec![],
        }),
        initiator_user_id: Some("user_test".into()),
        visibility: Some("private".into()),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: DataPlaneIngestRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.org_id, "org_test");
    assert_eq!(back.chunks.len(), 1);
    assert!(back.source_trace.is_some());
    assert_eq!(back.initiator_user_id.as_deref(), Some("user_test"));
    assert_eq!(back.visibility.as_deref(), Some("private"));
}

#[test]
fn data_plane_ingest_request_omits_absent_ownership_fields() {
    // System/connector ingest: no initiator, no visibility override. The wire
    // shape must omit both keys so older documents-api deployments keep parsing.
    let req = DataPlaneIngestRequest {
        run_id: quarry_core::ids::Id::new(),
        org_id: "org_test".into(),
        source_url: "https://example.com/page".into(),
        title: None,
        markdown: None,
        html_ref: None,
        raw_ref: None,
        chunks: vec![],
        metadata: json!({}),
        fingerprint: "blake3:abc123".into(),
        zdr: ZdrMode::Off,
        retention_policy: None,
        privacy_policy: None,
        source_trace: None,
        initiator_user_id: None,
        visibility: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("initiator_user_id"));
    assert!(!json.contains("visibility"));
    let back: DataPlaneIngestRequest = serde_json::from_str(&json).unwrap();
    assert!(back.initiator_user_id.is_none());
    assert!(back.visibility.is_none());
}

#[test]
fn data_plane_ingest_response_serde_roundtrip() {
    let resp = DataPlaneIngestResponse {
        document_id: "doc_123".into(),
        index_status: IndexStatus::Indexed,
        knowledge_unit_count: 5,
        embedding_status: EmbeddingStatus::Embedded,
        retrievable_after: Some(chrono::Utc::now()),
        trace_id: "trace_abc".into(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: DataPlaneIngestResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.document_id, "doc_123");
    assert_eq!(back.index_status, IndexStatus::Indexed);
    assert_eq!(back.embedding_status, EmbeddingStatus::Embedded);
}

#[test]
fn structured_extract_request_serde_roundtrip() {
    let req = StructuredExtractRequest {
        source_artifact_ref: quarry_core::ids::Id::new(),
        markdown: Some("# Product\nPrice: $10".into()),
        structured_output_schema: Some(json!({
            "type": "object",
            "properties": { "price": { "type": "number" } }
        })),
        source_trace_required: true,
        max_cost_usd: Some(0.01),
        max_tokens: Some(1000),
        zdr: ZdrMode::On,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: StructuredExtractRequest = serde_json::from_str(&json).unwrap();
    assert!(back.source_trace_required);
    assert_eq!(back.zdr, ZdrMode::On);
    assert!(back.structured_output_schema.is_some());
}

#[test]
fn structured_extract_response_serde_roundtrip() {
    let resp = StructuredExtractResponse {
        artifact_id: quarry_core::ids::Id::new(),
        data: json!({"price": 10}),
        schema_valid: true,
        usage: ExtractionUsage {
            input_tokens: 500,
            output_tokens: 50,
            cost_usd: 0.002,
        },
        source_trace: None,
        model: "claude-sonnet-4-6".into(),
        provider: "anthropic".into(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: StructuredExtractResponse = serde_json::from_str(&json).unwrap();
    assert!(back.schema_valid);
    assert_eq!(back.usage.input_tokens, 500);
    assert_eq!(back.model, "claude-sonnet-4-6");
}

#[test]
fn zdr_on_serializes_correctly() {
    let req = AgentActionRequest {
        run_id: quarry_core::ids::Id::new(),
        lease_id: quarry_core::ids::Id::new(),
        action: AgentAction::GetContent,
        instruction: None,
        constraints: AgentConstraints {
            max_steps: 1,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        },
        zdr: ZdrMode::On,
        extraction_profile: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains(r#""zdr":"on""#));
}
