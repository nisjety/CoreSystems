## EP-2-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/citation_anchors.rs: in `anchor_citations`'s `events.push(ChatEvent::Citation { ... origin: citation.origin.clone(), })` add `passage: citation.passage.clone(),` after the origin line; in tests `fn recorded(...)` RecordedCitation literal and in `a_connector_citation_is_never_anchored_by_number`'s RecordedCitation literal add `passage: None,` after `origin: ...,`

## EP-2-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/connector_reads.rs: in the fn building `ChatEvent::Citation { id: format!("conn-{}", ...) ... origin: "connector".to_owned(), }` (~line 602) add `passage: None,` after the origin line

## EP-2-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs: add `passage: None,` after `origin: "web".to_owned(),` in the two ChatEvent::Citation constructions (web_search citation `Some(ChatEvent::Citation { id: format!("web-{}-{}", rank + 1, hit.url) ...` ~7600 and `citations: vec![ChatEvent::Citation { id: format!("web-1-{}", hit.url) ...` ~8078); in the test match `ChatEvent::Artifact { id, kind, title, content, version, } =>` (~13852, asserting id "q3-rapport") add `..` after `version,`

## EP-2-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifacts.rs: in `artifact_event` add `provenance: None,` after `version,` inside `ChatEvent::Artifact { ... }`; in test `events_carry_the_wire_shape_the_client_parses` add `..` after `version,` in the `ChatEvent::Artifact { id, kind, title, content, version, }` pattern

## EP-2-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifact_revisions.rs: tests `fn published(...)` RecordedArtifact literal add `provenance: None,` after `version,`

## EP-2-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/result_validation.rs: test `vec![crate::sse_events::RecordedArtifact { ... }]` (~862) add `provenance: None,` after the version field

## EP-2-S4
shared-edit apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs: after `the_web_reader_goldens_are_rows_decided_as_named` add the same test named `the_cited_passage_goldens_are_rows_decided_as_named` with rows [("e_zdr_turn_kb_citation_passage_streams_not_persisted", "zdr"), ("e_waived_research_metadata_not_persisted", "waived")]

## EP-2-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts: add `type CitationLocator, type ResearchProvenance` to the `@/shared/api/chat-client` import; in `Citation` after `origin?: ...` add `/** The passage this source contributed (EP-2-S4), when the server sent one. */ quote?: string` and `/** Where a knowledge source's passage sits in Data Plane, in the wire shape. */ locator?: CitationLocator`; in `ChatArtifact` after `history?` add `/** A research report's provenance stamp (EP-2-S4), in the wire shape. */ provenance?: ResearchProvenance`

## EP-2-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.ts: `normalizeCitation` parameter type add `quote?: string; locator?: CitationLocator` and in the returned object add `...(event.quote ? { quote: event.quote } : {}), ...(event.locator ? { locator: event.locator } : {}),` after the origin spread; `normalizeArtifact` parameter add `provenance?: ResearchProvenance` and return `...(event.provenance ? { provenance: event.provenance } : {}),` after version (types imported from '@/shared/api/chat-client'). Without this the live stream drops the passage before the viewer sees it.

## EP-2-S4
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatArtifactPanel.tsx: import `{ ArtifactProvenance } from './ArtifactProvenance'`; in `ArtifactViewer`, right after the closing `</div>` that ends the Switch body and before `</section>`, add `<ArtifactProvenance provenance={(artifact() as { provenance?: unknown }).provenance} />`

## EP-2-S4
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after `'chat.inspect_run_request': exempt('no_owner_executor', 'read'),` add `'chat.open_cited_passage': exempt('no_owner_executor', 'read'),`

## EP-2-S4
shared-edit apps/Frontend Plane/verevonv3/src/styles/global.css (next to .verevon-chat-citation-chip rules): `.verevon-chat-cited-passage{position:fixed;inset:0 0 0 auto;width:min(420px,100vw);z-index:60;overflow:auto;padding:16px;background:var(--surface,#fff);box-shadow:-8px 0 24px rgb(0 0 0/.12)}` `.verevon-chat-cited-passage__header{display:flex;justify-content:space-between;gap:8px}` `.verevon-chat-cited-passage__quote,.verevon-chat-source-card__quote{margin:8px 0;padding-left:10px;border-left:3px solid currentColor;opacity:.85}` `.verevon-chat-citation-chip__passage,.verevon-chat-source-card__passage{font-size:12px}`

## EP-2-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: under model-gateway environment after `MODEL_GATEWAY_WEB_SEARCH_BATCH: ${MODEL_GATEWAY_WEB_SEARCH_BATCH:-false}` add `      MODEL_GATEWAY_CITATION_PASSAGES: ${MODEL_GATEWAY_CITATION_PASSAGES:-false}`

## EP-2-S4
shared-edit apps/Model Plane/docs/ZDR.md: at the end of section 5.5 (after the learning-review [live-only] paragraph) add: 'Cited passages and the research stamp (EP-2-S4, behind MODEL_GATEWAY_CITATION_PASSAGES): a citation may carry `quote` (the passage, at most 400 characters) and, for org knowledge, `locator` (document_id and chunk_index); a deep-research report artifact may carry `provenance` (question, sub-queries, read sources, model, coverage). All three stream live on every turn and are stored in the assistant message metadata (citations, artifacts) only on a persistent turn, erased with the message by the GDPR path. A ZDR or waived turn stores neither (session_flow::persistable_citations / persistable_artifacts; goldens e_zdr_turn_kb_citation_passage_streams_not_persisted, e_waived_research_metadata_not_persisted). The viewer\'s "more" read is Data Plane\'s ACL\'d chunk read and stores nothing.'

## EP-2-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/org_agent_policy.rs (REQUIRED to compile, because the match on Kind is exhaustive): in fn token_of, replace the block from `let read = match def.kind {` down to the closing `};` with:
    // An integer key's token is its decimal form, held here so the match can lend it out.
    let count;
    let read = match def.kind {
        Kind::Enum => value.as_str(),
        Kind::Bool => value
            .as_bool()
            .map(|flag| if flag { "true" } else { "false" }),
        Kind::AllowList => value.get("mode").and_then(Value::as_str),
        Kind::Integer => {
            count = value.as_u64().map(|n| n.to_string());
            count.as_deref()
        }
    };
(the following `read.filter(...)` lines stay as they are).

## EP-2-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/org_agent_policy.rs (REQUIRED to compile): in `pub struct OrgPolicy`, after `pub restrictive: bool,`, add:
    /// The most items one wide-research run may fan out (`research.max_fan_out_items`, EP-2 S8). A value this build
    /// cannot read is 0: wide research off for the org.
    pub max_fan_out_items: u32,
In `OrgPolicy::from_settings`, inside `Self { ... }` after the `restrictive: ...,` field, add:
            max_fan_out_items: token(mp_contracts::org_agent_policy::RESEARCH_MAX_FAN_OUT_ITEMS)
                .parse()
                .unwrap_or(0),
Also add a test in its `mod tests`, for example: `assert_eq!(OrgPolicy::from_settings(0, "", &settings(json!({}))).max_fan_out_items, 8); assert_eq!(OrgPolicy::from_settings(1, "d", &settings(json!({"research.max_fan_out_items": 3}))).max_fan_out_items, 3); assert_eq!(OrgPolicy::from_settings(1, "d", &settings(json!({"research.max_fan_out_items": "x"}))).max_fan_out_items, 0);`

## EP-2-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs (REQUIRED to compile; this is the test helper make_request, a full InvokeRequest literal): after the line `agent_autonomy_cap: None,` (around :622) add `            wide_research: None,`

## EP-2-S8
shared-edit apps/Model Plane/api/contracts/org-agent-policy-schema.json (REQUIRED, because the mp-contracts golden test and the Go TestOrgAgentPolicySchemaMatchesTheGolden compare key by key): append to `settings`, after the reference_uploads row: {"key": "research.max_fan_out_items", "kind": "integer", "strictness_order": ["0","1","2","3","4","5","6","7","8","9","10","11","12","13","14","15","16"], "default": "8", "fail_closed_value": "0", "status": "enforced"}. Also add `integer` to the description's list of kinds if it has one. The file uses LF line endings.

## EP-2-S8
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `'chat.inspect_run_request': exempt('no_owner_executor', 'read'),` add `  'chat.wide_research_start': exempt('no_owner_executor', 'external_write'),`

## EP-2-S8
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.test.ts (owned by EP-10-S1): raise the expected action count by 1 (from 274 plus whatever else this batch adds) and add the comment line `    // +1 after EP-2-S8 registered chat.wide_research_start, the composer's wide-research entry.`

## EP-2-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service environment, next to MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE, add:
      # EP-2 S8: the composer's wide-research entry (estimate route + wide_research on a Do run). Only the literal true.
      WIDE_RESEARCH_ENTRY_ENABLED: ${WIDE_RESEARCH_ENTRY_ENABLED:-false}

## EP-2-S8
shared-edit apps/Model Plane/docs/ZDR.md: in the 'Features refused with a typed code' row, add that a wide-research run (InvokeRequest.wide_research) and POST /v1/research/wide/estimate are refused under ZDR with zdr_feature_not_supported, feature agentic (EP-2 S8); the estimate stores nothing.

## EP-6-S3
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/cron-client.ts: in `export interface CronScheduleFire` (about line 147), after the line `  withheld_effects: number` and before the closing brace, add `  /** The fire's key: the schedule slot's time, or manual:<request id> for a Run now (EP-6-S3). */` and `  fire_key?: string`. AutomationsList.tsx and ScheduledRunsPanel.test.tsx read fire.fire_key, so the typecheck needs this line.

## EP-6-S3
shared-edit apps/Frontend Plane/verevonv3/src/features/agents/components/AgentsHubSummary.tsx: line 159, replace `automationStateOf(schedule, null, now)` with `automationStateOf(schedule, now)`. automationStateOf lost its lastFire parameter; without this the typecheck fails on arity.

## EP-10-S1
shared-edit apps/Model Plane/rust/services/execution-core/src/browser_events.rs: inside `impl OrchestrationCoreService for NeverResolvingOrchestration`, right after the `get_quarantine_preview` method (8-space indent), add:
        async fn get_approval_preview(
            &self,
            _: tonic::Request<mp_contracts::model_plane::v1::GetApprovalPreviewRequest>,
        ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetApprovalPreviewResponse>, tonic::Status> {
            Err(tonic::Status::unimplemented("get_approval_preview is not served by this double"))
        }

## EP-10-S1
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs: inside `impl OrchestrationCoreService for MockOrchestration` (~line 12592), right after its `get_quarantine_preview` method, add the same `get_approval_preview` method as above. Without it the execution-core tests will not compile once the proto has the new RPC.

## EP-10-S1
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `'approvals.decide': exempt('irreversible_destructive', 'destructive'),` add `'approvals.preview': exempt('approval_authority', 'read'),`. The map must cover every action id (exhaustive by type), so typecheck fails without this row.

## EP-10-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service environment block add `APPROVAL_WRITE_PREVIEW_ENABLED: ${APPROVAL_WRITE_PREVIEW_ENABLED:-false}`. The owner adds the same name to apps/Model Plane/deploy/.env.example. I did not touch either file.

## EP-1-S2
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/orchestration-client.ts: in normalizeApprovalCause, after `stepUpRequired: cause.step_up_required === true,` add `    assistKind: str(cause.assist_kind),` / `    assistOrigin: str(cause.assist_origin),` / `    host: str(cause.host),`

## EP-1-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs: in fn approval_cause_value json!, after `"rule_id": empty_to_null(&cause.rule_id),` add `        "assist_kind": empty_to_null(&cause.assist_kind),` / `        "assist_origin": empty_to_null(&cause.assist_origin),` / `        "host": empty_to_null(&cause.host),` (owner-only approval projection; cause already allowlists these through keep_browser_assist_facts)

## EP-1-S2
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core environment, after `      EXECUTION_DO_BROWSER_ENABLED: ${EXECUTION_DO_BROWSER_ENABLED:-0}` add `      EXECUTION_BROWSER_HUMAN_ASSIST: ${EXECUTION_BROWSER_HUMAN_ASSIST:-false}`

## EP-1-S3
shared-edit apps/Frontend Plane/verevonv3/src/styles/global.css: next to `.verevon-chat-run-panel__control-error` (around line 24101), add `.verevon-chat-run-panel__give-back-note { display: grid; gap: 4px; margin-top: 8px; font-size: 12px; } .verevon-chat-run-panel__give-back-note textarea { width: 100%; resize: vertical; font: inherit; }`.

## EP-1-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core service environment, next to EXECUTION_BROWSER_TAKEOVER_PAUSE, add `EXECUTION_BROWSER_ASSIST_TTL_SECONDS: ${EXECUTION_BROWSER_ASSIST_TTL_SECONDS:-1800}`

