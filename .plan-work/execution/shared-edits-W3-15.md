## EP-2-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs: in the test helper `make_request` struct literal, directly after the line `            deep_research: false,` add `            research_phase: None,` and `            research_plan: None,` (InvokeRequest gained two fields; the literal will not compile otherwise).

## EP-2-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, directly after `      WIDE_RESEARCH_ENTRY_ENABLED: ${WIDE_RESEARCH_ENTRY_ENABLED:-false}` add `      # EP-2 S5: deep research shows an editable plan first (research_phase plan|run). Only the literal true.` and `      DEEP_RESEARCH_PLAN_TURN: ${DEEP_RESEARCH_PLAN_TURN:-false}`.

## EP-2-S5
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml: next to `      VITE_PLAN_EXECUTE_ENABLED: ${VITE_PLAN_EXECUTE_ENABLED:-false}` add `      VITE_DEEP_RESEARCH_PLAN_TURN: ${VITE_DEEP_RESEARCH_PLAN_TURN:-false}`.

## EP-2-S5
shared-edit apps/Frontend Plane/verevonv3/src/vite-env.d.ts: after `	readonly VITE_PLAN_EXECUTE_ENABLED?: string;` add `	readonly VITE_DEEP_RESEARCH_PLAN_TURN?: string;` (tab-indented like its neighbours).

## EP-2-S5
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: in the `useChatController()` destructure after `    skipPendingQuestion,` add `    pendingResearchPlan,` `    startResearchPlan,` `    cancelResearchPlan,`; on `<ComposerTrays ...>` after `onSkipQuestion={() => void skipPendingQuestion()}` add `                researchPlan={pendingResearchPlan()}` `                onStartResearchPlan={(edit) => void startResearchPlan(edit)}` `                onCancelResearchPlan={cancelResearchPlan}`.

## EP-2-S5
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after `  'chat.wide_research_start': exempt('no_owner_executor', 'external_write'),` add `  'chat.research_plan_start': exempt('no_owner_executor', 'external_write'),` and `  'chat.research_plan_cancel': exempt('ui_state_only', 'context_write'),`.

## EP-2-S5
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.test.ts: after the `// +1 after EP-10-S1 registered approvals.preview ...` comment add `    // +2 after EP-2-S5 registered chat.research_plan_start and chat.research_plan_cancel, the deep-research plan card.` and raise `toHaveLength(277)` by 2 (on top of whatever the other batch slices add).

## EP-2-S5
shared-edit apps/Model Plane/docs/ZDR.md section 5.5: after the line `The viewer's "more" read is Data Plane's ACL'd chunk read and stores nothing.` add a paragraph: `The deep-research plan turn (EP-2-S5, behind DEEP_RESEARCH_PLAN_TURN): the plan turn streams a research_plan event (sub-queries, up to three clarifying questions, the sub-query cap) and searches nothing; the assistant message metadata keeps the same plan under research_plan only on a persistent turn, erased with the message by the GDPR path. A ZDR turn is refused before either phase through deep_research (no new refusal key); a waived turn is refused deep research at the route, and session_flow::with_research_plan would store nothing for it anyway (D12). The run turn's answers are user text: redacted like the message when the turn redacts, bounded to 1,000 characters, and sent only to the synthesis inference.`

## EP-2-S9
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core environment block, directly after the line `WIDE_RESEARCH_FAN_OUT_CONCURRENCY: ${WIDE_RESEARCH_FAN_OUT_CONCURRENCY:-4}`, add:
      # Wave 3 EP-2 S9: the token budget all children of one fan-out call may spend (metered tokens; subscription rounds count 0). Only the literal true enforces it; off, a spent budget is logged and every item still starts.
      WIDE_RESEARCH_CHILD_BUDGET_ENFORCED: ${WIDE_RESEARCH_CHILD_BUDGET_ENFORCED:-false}
      WIDE_RESEARCH_CHILD_TOKEN_BUDGET: ${WIDE_RESEARCH_CHILD_TOKEN_BUDGET:-400000}

## EP-2-S9
shared-edit apps/Model Plane/deploy/.env.example (owner edits this file): add `WIDE_RESEARCH_CHILD_BUDGET_ENFORCED=false` and `WIDE_RESEARCH_CHILD_TOKEN_BUDGET=400000` beside WIDE_RESEARCH_FAN_OUT_CONCURRENCY, with the comment 'EP-2 S9 child token budget; only literal true enforces'.

## EP-2-S9
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `  'chat.wide_research_start': exempt('no_owner_executor', 'external_write'),` add `  'chat.wide_research_retry_item': exempt('no_owner_executor', 'external_write'),`

## EP-2-S9
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.test.ts: after the comment line `// +1 after EP-10-S1 registered approvals.preview, the owner's write-approval preview.` add `// +1 after EP-2-S9 registered chat.wide_research_retry_item, the wide-research grid's per-item retry.` and raise the `expect(ids).toHaveLength(...)` count by 1 (277 -> 278 on this base; add 1 to whatever the merged count is).

## EP-2-S9
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts: in `export type WideResearchStart` add `/** A one-item retry (EP-2 S9): the child run whose item runs again; execution-core reads the item back. */ retryOfChildRunId?: string` and `/** The fan-out batch the retry joins. */ batchId?: string`; in `wideResearchWire` return `{ retry_of_child_run_id: start.retryOfChildRunId, ...(start.batchId ? { batch_id: start.batchId } : {}) }` (and no items/item_cap) when `start.retryOfChildRunId` is non-empty; widen its return type accordingly.

## EP-2-S9
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx line ~1092: change `<ChatWideResearchPanel runId={liveRunId()} />` to `<ChatWideResearchPanel runId={liveRunId()} threadId={state.threadId ?? null} />`. Add `onRetryItem` only once the composer send path carries `wideResearch` (S8's WideResearchStart is not mounted on this base). It should send an agentic turn with `wideResearch: { retryOfChildRunId, batchId }` and a goal like 'Prøv igjen: <goalSummary>'.

## EP-2-S6
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts: in type ResearchProvenance add optional `passes?: number; ledger_digest?: string; claims_by_status?: Record<string, number>; research_ledger?: unknown` and make sources `{ number: number; url: string; title: string; pass?: number }[]`; in parseResearchProvenance keep them: in the sources flatMap return `[{ number, url, title: str(source?.title) ?? '', ...(typeof source?.pass === 'number' ? { pass: source.pass } : {}) }]`, and in the returned object add `...(typeof record.passes === 'number' ? { passes: record.passes } : {}), ...(str(record.ledger_digest) ? { ledger_digest: str(record.ledger_digest) } : {}), ...(objectValue(record.claims_by_status) ? { claims_by_status: objectValue(record.claims_by_status) as Record<string, number> } : {}), ...(record.research_ledger !== undefined ? { research_ledger: record.research_ledger } : {})`. Without this edit the parser drops these fields and the frontend never shows pass grouping or claim counts. The existing `toEqual(stamp)` test in chat-client.cited-passage.test.ts still passes because its stamp has none of these fields.

## EP-2-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: add `import { researchPassFacts } from './ArtifactProvenance'`; after `const sourceBadgeRead = ...` (~line 732) add `const secondPassNumbers = createMemo(() => { for (const item of [...artifactItems()].reverse()) { const facts = researchPassFacts((item.artifact as { provenance?: unknown }).provenance); if (facts) return facts.secondPassNumbers } return [] as number[] })`; change line ~1029 to `<SourcesPanel grounding={latestGrounding()} sources={evidenceSources()} secondPassNumbers={secondPassNumbers()} />`

## EP-2-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment block, beside DEEP_RESEARCH_* / MODEL_GATEWAY_CITATION_PASSAGES, add `DEEP_RESEARCH_SECOND_PASS: ${DEEP_RESEARCH_SECOND_PASS:-false}`

## EP-2-S6
shared-edit apps/Model Plane/docs/ZDR.md: in the deep-research / message-metadata rows add 'research_ledger (EP-2-S6): claim ledger in artifacts[].provenance.research_ledger of the assistant message metadata; deep research is refused under ZDR; a waived turn streams it on the report frame and session_flow::persistable_artifacts strips it with the stamp (D12); erased by the message GDPR path; never indexed or fed to memory.'

## EP-10-S4
shared-edit apps/Model Plane/rust/services/session-core/Cargo.toml: under [dependencies] next to `mp-contracts = { path = "../../crates/mp-contracts" }` add `mp-scrub = { path = "../../crates/mp-scrub" }`. The edited approval reason is scrubbed with mp_scrub::scrub_string (K3-16), and Cargo.lock gains the edge.

## EP-10-S4
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs: three pb::ConsumeApprovalResponse literals with no default spread do not compile with the new fields 8/9: the mock consume_approval `Ok(Response::new(pb::ConsumeApprovalResponse { ... authority: Some(mock_snapshot()),` (~line 13232), fn consume_of (~17558) and the descriptor-based consume helper (~26461). Add `..Default::default()` after the `authority:` line in each.

## EP-10-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs: two ConsumeApprovalResponse literals without a spread (~22810 in the winner mock's `Some(first) =>` arm, after `authority: None,`; ~22956 after `authority: Some(mp_contracts::model_plane::v1::ApprovedAuthority::default()),`). Add `..Default::default()` to each.

## EP-10-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: add `APPROVAL_EDIT_ENABLED: ${APPROVAL_EDIT_ENABLED:-false}` to the environment of the session-core, model-gateway and execution-core service blocks.

## EP-3-S3
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: in the artifacts tab (Match activeTab() === 'artifacts'), replace `<ArtifactsPanel items={artifactItems()} />` with `<ArtifactsPanel items={artifactItems()} history={{ threadId: state.threadId ?? '', temporary: temporaryChat() || isActiveThreadTemporary() }} />`. Leave the empty fallback `<ArtifactsPanel items={[]} />` unchanged.

## EP-3-S3
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after `'chat.open_cited_passage': exempt('no_owner_executor', 'read'),` add `  'chat.list_artifact_revisions': exempt('no_owner_executor', 'read'),`

## EP-3-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: add `pub mod artifact_edit_routes;` in alphabetical order (before `pub mod artifact_revisions;`).

## EP-3-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/state.rs: (1) after the field `pub artifact_revisions_mode: crate::artifact_revisions::ArtifactRevisionsMode,` add `/// `MODEL_GATEWAY_ARTIFACT_EDITING`, read once at startup after the revision mode (EP-3 S4). Off answers the edit routes 404.` + `pub artifact_editing_enabled: bool,`; (2) in `AppState::new()` after `artifact_revisions_mode: crate::artifact_revisions::ArtifactRevisionsMode::Off,` add `artifact_editing_enabled: false,`; (3) in from_env right after `let artifact_revisions_mode = crate::artifact_revisions::ArtifactRevisionsMode::from_env()?;` add `// Editing over a shadow or absent revision table would save text no turn reads: refuse the boot.` + `let artifact_editing_enabled = crate::artifact_edit_routes::editing_from_env(artifact_revisions_mode)?;`; (4) in BOTH from_env branches after `state.artifact_revisions_mode = artifact_revisions_mode;` add `state.artifact_editing_enabled = artifact_editing_enabled;`.

## EP-3-S4
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after `'chat.save_thread_snapshot': exempt(...)` add `'chat.save_artifact_revision': exempt('no_owner_executor', 'context_write'),` and `'chat.restore_artifact_revision': exempt('no_owner_executor', 'context_write'),`; then regenerate the agent-action-admission golden.

## EP-3-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml (model-gateway block, under line `MODEL_GATEWAY_ARTIFACT_REVISIONS: ${MODEL_GATEWAY_ARTIFACT_REVISIONS:-off}`): add `MODEL_GATEWAY_ARTIFACT_EDITING: ${MODEL_GATEWAY_ARTIFACT_EDITING:-false}`; the owner adds the .env.example line.

## EP-3-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs (invoke stream, right after the connector_selection match block that ends with the `Err(ConnectorSelectionRefusal::Malformed)` arm and before `let attachment_policy = crate::attachment_policy::AttachmentPolicy::from_env();`): insert
    // Typed context references (Wave 3 EP-3 S5), checked before any run or model call. With the switch off the
    // field is never read, so the turn is the one it was before.
    let context_refs = match crate::context_refs::resolve(
        crate::context_refs::enabled(),
        req.context_refs.as_ref(),
        &crate::context_refs::TurnFacts {
            zdr: turn_posture.zdr(),
            waived: turn_posture.is_waived(),
            agentic: features.iter().any(|feature| feature == "agentic"),
            knowledge: !conversation_only && retrieval_scope != crate::normalize::RetrievalScope::WebOnly,
            space_id: req.space_context.as_ref().or(req.space_append_context.as_ref()).map(|context| context.space_id.as_str()).filter(|id| !id.is_empty()),
        },
    ) {
        Ok(resolved) => resolved,
        Err(crate::context_refs::ContextRefRefusal::Zdr) => {
            return zdr_refusal_stream(&request_id, crate::context_refs::ContextRefRefusal::zdr_refusal());
        }
        Err(crate::context_refs::ContextRefRefusal::Typed { code, message, .. }) => {
            return error_stream(&request_id, code, message, false);
        }
    };

## EP-3-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs (same fn, the `crate::session_flow::prepare_managed_run_authenticated_with_receipt(` call that starts `let session_run = match`): wrap it as `let session_run = match crate::session_flow::with_user_context_refs(context_refs.as_ref(), crate::session_flow::prepare_managed_run_authenticated_with_receipt( ...unchanged args... )).await {` (one extra closing paren before `.await`)

## EP-3-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs (fetch_skill_context call in the same fn): replace the argument `&req.skill_ids,` with `&crate::context_refs::merged_skill_ids(&req.skill_ids, context_refs.as_ref()),`

## EP-3-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs (same fn): just before the comment `// The tool phase and the first inference call run INSIDE this task` add `let context_ref_documents = crate::context_refs::register_turn_documents(&session_run.run_id, context_refs.as_ref());`; and as the first line inside the following `tokio::spawn(async move {` next to `let _active_stream = active_stream;` add `let _context_ref_documents = context_ref_documents;`

## EP-3-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs (test helper make_request, InvokeRequest literal ~line 623): after `wide_research: None,` add `context_refs: None,`

## EP-3-S5
shared-edit apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs (cases() vec, after the row `("wide_research", with("wide_research", Value::Null), None),`): add `// EP-3 S5: a ZDR turn carrying context references is refused before any network call.` then `("context_refs", with("context_refs", json!([{"kind": "document", "id": "doc-1"}])), refused("context_references")),` and `("context_refs", with("context_refs", json!([])), None),`

## EP-3-S5
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts: after `| 'run_inspection'` add `| 'context_references'`; after the `run_inspection: {...}` copy row add `context_references: { no: 'referanser til dokumenter, samtaler eller utvalg', en: 'references to documents, threads or selections' },`

## EP-3-S5
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts: in ChatTurn after `retrievalScope?: import('@/shared/actions/chat-source-scope').RetrievalScope` (the field documented 'Per-turn retrieval-source filter this turn was SENT with') add `/** The context chips this turn was sent with (EP-3 S5): kind, id and label only. */` + `contextRefs?: import('@/shared/actions/chat-context-refs').ChatContextRef[]`; in the send-options type after `connectorScope?: string[]` (line ~715) add `/** See \`ChatInvokeRequest.contextRefs\`. Per-turn, like \`connectorScope\`. */` + `contextRefs?: import('@/shared/actions/chat-context-refs').ChatContextRef[]`

## EP-3-S5
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.ts (messageToTurn): after `retrievalScope: msg.retrievalScope,` add `contextRefs: msg.contextRefs,`

## EP-3-S5
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts: (a) in sendContent's userTurn literal after `retrievalScope: options.retrievalScope ?? retrievalScope(),` add `contextRefs: options.contextRefs,`; (b) in the request object after `connectorScope: options.connectorScope,` (~3307) add `contextRefs: options.contextRefs,`; (c) in the composer-submit sendContent options after `connectorScope: payload.connectorScope,` (~3683) add `contextRefs: payload.contextRefs,`

## EP-3-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml (model-gateway env, after the WIDE_RESEARCH_ENTRY_ENABLED line): `# EP-3 S5: typed context references on the send (document, skill, own Space). Only the literal true.` + `MODEL_GATEWAY_CONTEXT_REFS_ENABLED: ${MODEL_GATEWAY_CONTEXT_REFS_ENABLED:-false}`

## EP-3-S5
shared-edit apps/Model Plane/docs/ZDR.md feature/refusal table: add a row `| **Context references on the send (EP-3 S5)** | No reference resolved on a ZDR or temporary turn | Built dark behind MODEL_GATEWAY_CONTEXT_REFS_ENABLED (off). A ZDR turn carrying any context_refs is refused before any network call with zdr_feature_not_supported, feature context_references (whatever the switch says). metadata.context_refs stores {kind,id,label} only, never on a ZDR turn (no message is written). |`

