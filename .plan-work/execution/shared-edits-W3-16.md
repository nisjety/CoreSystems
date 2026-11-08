## EP-10-S5
shared-edit apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs fn consume_response: in the `unwrap_or_else(|| store::ConsumedCall { ... })` fallback literal add `revision: 0,` and `original_action_fingerprint: String::new(),` after `offered_json: String::new(),`. In the returned `proto::ConsumeApprovalResponse` replace the two-line comment `// EP-10-S5 reads the revision back ...` and `revision: 0,` / `original_action_fingerprint: String::new(),` with `revision: call.revision,` and `original_action_fingerprint: call.original_action_fingerprint,`.

## EP-10-S5
shared-edit apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs tests fn consumed_with_call: in the `store::ConsumedCall { ... }` literal add `revision: 0,` and `original_action_fingerprint: String::new(),` after the `offered_json: ...to_owned(),` field.

## EP-10-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs fn consume_for_resume: in the final `Ok(super::resume::Consumed { ... })` literal add `revision: consumed.revision,` and `original_action_fingerprint: consumed.original_action_fingerprint,` after `segment,`.

## EP-10-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs fn released_call: change the `let release = super::resume::ApprovedRelease::new(gate, &step_id, &call.name, &call.arguments_json).map_err(|_| tonic::Status::failed_precondition("not_releasable"))?;` statement to end with `?.with_edit(consumed.edit_note());`. This gives the stage-1 segment its unnamed note.

## EP-10-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs fn restore_continuation: in the `Ok(restored) => Ok(Ok(Continuation { release: Some(release), ...` arm change `release: Some(release),` to `release: Some(release.with_edit(restored.edit.clone())),`. The field must stay before `seed: RoundSeed::InPlace(Box::new(restored))`, so the note names the fields from the fold's draft.

## EP-10-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs fn connector_step_note: change `let note = format!("[connector_write op={operation} content_redacted=true]");` to `let note = format!("[connector_write op={operation}{} content_redacted=true]", super::edit_step_note(&outcome.output));`. The suffix is empty unless the result opens with an edit note, so the step record stays byte-identical with the switch off.

## EP-10-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs test literal `let consumed = runtime_loop::resume::Consumed {` (about line 26582): add `revision: 0,` and `original_action_fingerprint: String::new(),`.

## EP-10-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs test literal `let restored = runtime_loop::resume::Restored {` (about line 18085): add `edit: None,`.

## EP-10-S6
shared-edit apps/Frontend Plane/verevonv3/src/shared/chat-nodes/types.ts: line 45 change to `import type { Approval, ApprovalDecision, ApprovalEdit } from '@/shared/api/orchestration-client'`; in ConversationNodeContext directly after `onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void` add `  /** Sends an approval with the person's edits (EP-10-S6). Absent: the card offers no edit mode. */\n  onApprovalEditedDecision?: (approvalId: string, edit: ApprovalEdit) => Promise<void>` (without it ChatMessages.tsx's context literal fails typecheck as an excess property).

## EP-10-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: in the controller destructure next to `handleApprovalDecision,` (~line 123) add `handleEditedApprovalDecision,`; on the MessageBlock after the `onApprovalDecision={(approvalId, decision) => void handleApprovalDecision(row().turn.id, approvalId, decision)}` prop (~1227) add `onApprovalEditedDecision={(approvalId, edit) => handleEditedApprovalDecision(row().turn.id, approvalId, edit)}`; on <ComposerTrays> after its onApprovalDecision prop (~1481) add `onApprovalEditedDecision={(turnId, approvalId, edit) => handleEditedApprovalDecision(turnId, approvalId, edit)}`.

## EP-10-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ComposerTrays.tsx: line 9 import add `ApprovalEdit`; in props after `onApprovalDecision: ...` (line 37) add `onApprovalEditedDecision?: (turnId: string, approvalId: string, edit: ApprovalEdit) => Promise<void>`; on the <ApprovalRequests> in the approval Match (~line 81) add prop `onDecideEdited={props.onApprovalEditedDecision ? (approvalId, edit) => { const entry = tray().approvals.find((candidate) => candidate.approval.id === approvalId); return entry && props.onApprovalEditedDecision ? props.onApprovalEditedDecision(entry.turnId, approvalId, edit) : Promise.resolve() } : undefined}`.

## EP-10-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs: in `approval_preview` replace `Ok(Json(json!({ "data": { "preview": approval_preview_value(preview) } })))` with building `let mut value = approval_preview_value(preview); value["can_edit"] = json!(approval_edit_enabled() && !preview.editable_fields.is_empty() && !preview.raw_only && preview.kind == "mail_send");` and returning `{data:{preview: value}}`; add a test `preview_can_edit_follows_the_edit_switch` (off -> false). The S4 gateway never added `can_edit`, so until this lands edit mode never appears.

## EP-3-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs (a), CRLF file: in invoke_stream_sse, inside the `&crate::context_refs::TurnFacts {` literal (about line 2178), add after the line `            knowledge: !conversation_only && retrieval_scope != crate::normalize::RetrievalScope::WebOnly,` (12 spaces) this line: `            selection_edit: crate::selection_guard::enabled_for(state.artifact_revisions_mode),`. Required: TurnFacts gained this field and the literal names every field.

## EP-3-S6
shared-edit sse.rs (b), CRLF: insert before the line `    // A summary written this turn, persisted with the answer so later turns` (about line 3289, right after the `load_recent_thread_messages(...)` call's `.await;`) this block at 4-space indent:
    // EP-3 S6: a selection edit names the version it revises. It is admitted against the heads this turn has just seeded, refused before any model call when the head moved, and held for this run's tool rounds (see the spawn below).
    let selection_hold = match crate::selection_guard::admit(
        &session_run.run_id,
        context_refs.as_ref(),
        &state.artifact_versions,
        &session_run.thread_id,
    ) {
        Ok(hold) => hold,
        Err(refusal) => {
            timing.fail();
            return prepared_direct_failure_stream(
                &state,
                &session_run,
                &model_bearer,
                &request_id,
                refusal.code,
                refusal.message,
                refusal.retryable,
            )
            .await;
        }
    };

## EP-3-S6
shared-edit sse.rs (c), CRLF: in the tokio::spawn block (about line 4005), after the line `        let _context_ref_documents = context_ref_documents;` add `        let _selection_hold = selection_hold;` so the hold lives exactly as long as the turn.

## EP-3-S6
shared-edit sse.rs (d), CRLF: in tool_phase_explanation (about line 918), insert before the line `        _ => ("audit_persistence_failed", "Tool action could not be durably audited"),` this arm: `        crate::selection_guard::REJECTED_REASON => (\n            crate::selection_guard::REJECTED_REASON,\n            "Endringen ble ikke lagret: den skrev om tekst utenfor det valgte avsnittet to ganger. Dette utkastet er ikke publisert. Prøv igjen og be bare om endring i det valgte.",\n        ),`. Without it selection_edit_rejected maps to audit_persistence_failed.

## EP-3-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/artifact_revision_routes.rs, CRLF: in capabilities_for, replace the one line `        selection_edit: false,` (8 spaces) with `        selection_edit: crate::selection_guard::enabled_for(mode) && !zdr,`. The composer then offers the edit only when the turn would honour it.

## EP-3-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: after line 437 (`      MODEL_GATEWAY_CONTEXT_REFS_ENABLED: ${MODEL_GATEWAY_CONTEXT_REFS_ENABLED:-false}`) add `      MODEL_GATEWAY_SELECTION_EDIT_ENABLED: ${MODEL_GATEWAY_SELECTION_EDIT_ENABLED:-false}` (6 spaces). Default false; never flip it.

## EP-3-S6
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: MODEL_ACCESS is typed Readonly<Record<ActionId, ModelAccessEntry>>, so tsc fails until this entry exists. After the line `  'chat.restore_artifact_revision': exempt('no_owner_executor', 'context_write'),` add `  'chat.edit_artifact_selection': exempt('agent_cannot_self_govern', 'context_write'),`.

## EP-3-S6
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/chat-context-refs.ts (LF). (a) Replace the ChatContextRef type (its doc comment through its closing brace) with: `/** The range an artifact_selection names (EP-3 S6): the revision the person saw, character offsets, and the quote's hash. */` `export type ChatArtifactSelectionRef = { artifactId: string; version: number; start: number; end: number; quoteSha256: string }` (write it multi-line) then `/** One chip as the send carries it and as metadata.context_refs stores it after the turn. */` `export type ChatContextRef = { kind: ChatContextRefKind; id: string; label: string; /** Present for an artifact_selection the panel sends. Never stored: the gateway keeps only kind, id and label. */ selection?: ChatArtifactSelectionRef }`. (b) After the isKind function add:
const SHA256_HEX = /^[0-9a-f]{64}$/

function normalizeArtifactSelection(value: unknown, id: string): ChatArtifactSelectionRef | undefined {
  if (!value || typeof value !== 'object') return undefined
  const { artifactId, version, start, end, quoteSha256 } = value as Record<string, unknown>
  const whole = (n: unknown): n is number => typeof n === 'number' && Number.isInteger(n)
  if (artifactId !== id || !whole(version) || !whole(start) || !whole(end)) return undefined
  if (typeof quoteSha256 !== 'string' || !SHA256_HEX.test(quoteSha256)) return undefined
  if (version < 1 || start < 0 || end <= start) return undefined
  return { artifactId: id, version, start, end, quoteSha256 }
}
(c) In normalizeContextRefs change `const { kind, id, label } = item as Record<string, unknown>` to `const { kind, id, label, selection } = item as Record<string, unknown>`, and replace the line `refs.push({ kind, id: trimmed, label: capContextRefLabel(typeof label === 'string' ? label : '') })` with: `const chip: ChatContextRef = { kind, id: trimmed, label: capContextRefLabel(typeof label === 'string' ? label : '') }` then `if (kind === 'artifact_selection') { const range = normalizeArtifactSelection(selection, trimmed); if (range) chip.selection = range }` then `refs.push(chip)`. Without (c) the range is dropped before the send and the gateway refuses the turn as context_refs_invalid.

## EP-3-S6
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx (LF): (a) after line 43 (`} from './ChatArtifactPanel'`) add `import type { ArtifactSelectionRequest } from './ArtifactSelection'`. (b) before line 264 (`  // Temporary chat writes no thread, so it cannot be turned on for a project.`) add:
  // The Output panel's "Ask for a change" (EP-3 S6) sends one composer turn that names the selected range. The panel has hashed the quote, so the selected text itself is not sent.
  const askAboutArtifactSelection = (request: ArtifactSelectionRequest) => submitWithWorkPeriod({
    actions: [],
    attachments: [],
    text: request.instruction,
    tools: [],
    contextRefs: [{
      kind: 'artifact_selection',
      id: request.artifactId,
      label: i18n.tr(`Utvalg i ${request.title}`, `Selection in ${request.title}`),
      selection: {
        artifactId: request.artifactId,
        version: request.version,
        start: request.start,
        end: request.end,
        quoteSha256: request.quoteSha256,
      },
    }],
  })
(c) on line 1052 change `<ArtifactsPanel items={artifactItems()} history={{ threadId: state.threadId ?? '', temporary: temporaryChat() || isActiveThreadTemporary() }} />` to add ` onAskAboutSelection={askAboutArtifactSelection}` before the `/>`. Without (b) and (c) the panel never offers the Ask action.

## EP-3-S6
shared-edit apps/Frontend Plane/verevonv3/src/styles/global.css (the chat artifact styles live here): add rules for `.verevon-chat-artifact-ask` (a bordered block with a gap, textarea full width) and `.verevon-chat-artifact-ask__actions` (a flex row with a gap), following the `.verevon-chat-artifact-action` rules. The popover is unstyled until this lands.

## EP-3-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/context_refs.rs: (a) add a field after `pub waived: bool,` in `TurnFacts`: `/// The thread reference tools (EP-3 S7) are switched on, so a thread reference can be read.\n    pub thread_references: bool,`. (b) Replace the whole `ContextRefKind::Thread => { return Err(...thread_refs_unavailable...) }` arm, and the 3-line OD-7 comment above it, with: `// OD-7: a thread is read lazily by the thread reference tools, never injected. A waived turn reads no other conversation (recall excludes waived content, DEC-32), and a conversation-only or web-only turn is offered no tool that could read it.\n            ContextRefKind::Thread => {\n                if !facts.thread_references {\n                    return Err(ContextRefRefusal::typed(UNSUPPORTED_CODE, "References to other conversations are not available yet. Remove them and send again.", "thread_refs_unavailable"));\n                }\n                if facts.waived {\n                    return Err(ContextRefRefusal::typed(UNSUPPORTED_CODE, "Other conversations cannot be referenced on this connection. Remove them and send again.", "thread_refs_waived"));\n                }\n                if !facts.knowledge {\n                    return Err(ContextRefRefusal::typed(UNSUPPORTED_CODE, "Referenced conversations cannot be used with a conversation-only or web-only source filter. Switch the source filter back to include knowledge.", "thread_refs_need_knowledge"));\n                }\n            }`. (c) Update the module doc bullet for `thread`. (d) Add a test `thread_ref_admitted_with_thread_tools_on_except_waived`: thread_references:true admits; with waived:true or knowledge:false the reason is thread_refs_waived or thread_refs_need_knowledge. The existing `thread_ref_refused_while_thread_tools_off` keeps passing because Default sets the field to false.

## EP-3-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs: (1) in the `crate::context_refs::TurnFacts { ... }` literal inside the `context_refs` resolve call, add after `waived: turn_posture.is_waived(),` the line `thread_references: crate::thread_reference_tools::enabled(),`. (2) Directly after `let context_ref_documents = crate::context_refs::register_turn_documents(&session_run.run_id, context_refs.as_ref());` add `let context_ref_threads = crate::thread_reference_tools::register_turn_threads(&session_run.run_id, context_refs.as_ref());`. (3) Next to `let _context_ref_documents = context_ref_documents;`, inside the spawned task, add `let _context_ref_threads = context_ref_threads;` so the registry entry lives for the whole turn.

## EP-3-S7
shared-edit apps/Model Plane/rust/crates/mp-content-safety/src/trust.rs: in TOOL_TRUST after `("recall_conversations", TrustClass::PriorConversation),` add `// EP-3 S7: the thread reference tools read an earlier conversation the person attached.\n    ("thread_outline", TrustClass::PriorConversation),\n    ("thread_search", TrustClass::PriorConversation),\n    ("thread_read", TrustClass::PriorConversation),`. Extend the parity contract case the table's doc comment names.

## EP-3-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/audit.rs: in tool_data_category, extend the customer_private arm with `| "thread_outline" | "thread_search" | "thread_read"`, and add the three names to the `reads_of_the_users_own_content_are_never_unclassified` list.

## EP-3-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml (model-gateway environment, after MODEL_GATEWAY_CONTEXT_REFS_ENABLED): `      # EP-3 S7: read-only thread_outline/thread_search/thread_read over attached threads (needs CONTEXT_REFS too). Only the literal true.\n      MODEL_GATEWAY_THREAD_REFERENCES_ENABLED: ${MODEL_GATEWAY_THREAD_REFERENCES_ENABLED:-false}`

## EP-3-S7
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md (section 3 table, after the 'Run control across gateway replicas' row): `| Thread references (read-only thread_outline / thread_search / thread_read, EP-3 S7) | **session-core** (ReadThreadForReference, SearchMessages RECALL) | Postgres (read only, no new store) | model-gateway offers the tools only for threads attached as context_refs on that run; thread_ref is a position, never an id | — | single owner; built dark behind MODEL_GATEWAY_THREAD_REFERENCES_ENABLED (off) |`

## EP-3-S7
shared-edit apps/Model Plane/docs/ZDR.md (after the 'Context references on the send (EP-3 S5)' row): `| **Thread references (EP-3 S7)** | Tools never offered on a ZDR or waived turn; ZDR and waived threads unreadable | Built dark behind MODEL_GATEWAY_THREAD_REFERENCES_ENABLED (off). Session Core ReadThreadForReference refuses a ZDR credential (DURABLE_MUTATION_FORBIDDEN) and any thread holding a zdr or zdr_waived run (thread_reference_retention); search uses the RECALL purpose. A call that arrives anyway answers thread_reference: unavailable. No new store. |`. No zdr-posture-matrix row (the rule forbids feature-refusal rows).

## EP-3-S8
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/artifacts.rs: in `get_composer_capabilities`, replace the final line `relay(&state, &user, &headers, &url).await` with `let response = relay(&state, &user, &headers, &url).await;` followed by `super::documents::with_promote_capability(response, state.space_features.chat_artifact_promote).await` (until then `with_promote_capability` is unused and the SPA never sees artifact_promote).

## EP-3-S8
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `'chat.restore_artifact_revision': exempt('no_owner_executor', 'context_write'),` add `  'chat.save_artifact_to_knowledge': exempt('no_owner_executor', 'external_write'),` (registry test expects MODEL_ACCESS access 'none').

## EP-3-S8
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml: in the gateway environment block next to line 227 `MODEL_ENTITLEMENT_ADMIN_ENABLED: ${MODEL_ENTITLEMENT_ADMIN_ENABLED:-false}` add `      CHAT_ARTIFACT_PROMOTE_ENABLED: ${CHAT_ARTIFACT_PROMOTE_ENABLED:-false}`.

## EP-5-S2
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, right after the line `      SKILL_BUNDLE_MOUNT_ENABLED: ${SKILL_BUNDLE_MOUNT_ENABLED:-false}` (about line 354), add `      # EP-5 S2: the typed deck tool in the Ask loop (manifest owned by the gateway, server-authored build). Off by` / `      # default; only the literal `true`.` / `      MODEL_GATEWAY_DECK_TOOL: ${MODEL_GATEWAY_DECK_TOOL:-false}`

## EP-5-S2
shared-edit apps/Model Plane/go/services/capability-core/internal/registry/capability_tools_seed_test.go: in readSeedRows, add `readMigration(t, "0033_deck_tool_class.up.sql") +` before the 0034 line and add 0033 to the comment list `0029, 0031, 0033 and 0034`.

## EP-5-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/room_run.rs: in BUILTIN_TOOL_CLASSES, add `("deck", Some(StepClass::Artifact)),` after the update_artifact row. Its test needs a row for every advertised name once the switch is on.

## EP-5-S2
shared-edit apps/Model Plane/rust/crates/mp-contracts/tests/prompt_tool_manifest.rs: add `const DECK_TOOL: &str = "services/model-gateway/src/deck_tool.rs";` and `(DECK_TOOL, "fn tool_definition()")` to the tool-builder list next to `(RECALL_CONVERSATIONS, "pub fn tool_definition()")`, so the deck tool's text is pinned.

## EP-5-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: add line `pub mod deck_templates;` between `pub mod dataplane;` and `pub mod deep_research;` (required for the build; CRLF)

## EP-5-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs: in FIELD_TREATMENTS after the `("context_refs", refused_feature("context_references")),` row add `    // EP-5 S7: a deck template is a skill bundle; a ZDR turn never mounts one, so a pick is refused like a skill.` and `    ("deck_template", refused_feature("skills")),`; in invoke_field_names destructuring after `context_refs: _,` add `deck_template: _,`; in its returned vec after `"context_refs",` add `"deck_template",`; in refusal() right after the `if crate::context_refs::requested(...) { return feature(crate::context_refs::ZDR_FEATURE); }` block add `if crate::deck_templates::zdr_refused(crate::deck_templates::enabled(), request.deck_template.as_ref()) { return feature(crate::deck_templates::ZDR_FEATURE); }` (CRLF)

## EP-5-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs: in the test make_request literal after `context_refs: None,` (around line 626) add `deck_template: None,` (CRLF)

## EP-5-S7
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after `'chat.pick_context_ref': exempt('no_owner_executor', 'read'),` add `'chat.list_deck_templates': exempt('no_owner_executor', 'read'),` and `'chat.apply_deck_template': exempt('no_owner_executor', 'context_write'),`

## EP-5-S7
shared-edit apps/Model Plane/docs/ZDR.md: add a row 'deck template list route and invoke deck_template (EP-5 S7): org config, no conversation content; refused for ZDR tokens with feature skills; no store'

## EP-5-S7
shared-edit apps/Frontend Plane/verevonv3 chat CSS (global.css beside .verevon-chat-artifact-office): styles for .verevon-deck-template-picker, __option, __name, __swatches, __swatch (small square, border token, no raw colours), __fonts, __error

## EP-7-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: add `pub mod thread_goal_routes;` immediately before `pub mod thread_search_routes;`

## EP-7-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/state.rs: (1) in the `mp_contracts::model_plane::v1::{...}` import, after `thread_fork_service_client::ThreadForkServiceClient,` add `thread_goal_service_client::ThreadGoalServiceClient,`; (2) in `pub struct AppState`, after `pub thread_fork_client: ThreadForkServiceClient<Channel>,` add `/// Session Core's ThreadGoalService on the session_client endpoint (EP-7 S1).\n    pub thread_goal_client: ThreadGoalServiceClient<Channel>,`; (3) in the test/default constructor after `thread_fork_client: ThreadForkServiceClient::new(session_channel.clone()),` add `thread_goal_client: ThreadGoalServiceClient::new(session_channel.clone()),`; (4) in from_env after the `let thread_fork_client = ThreadForkServiceClient::new(Self::lazy_channel("SESSION_CORE_URL", "SESSION_CORE_ADDR", "http://localhost:9091")?);` block add the same block as `let thread_goal_client = ThreadGoalServiceClient::new(Self::lazy_channel(...same three args...)?);` and add `state.thread_goal_client = thread_goal_client;` after each `state.thread_fork_client = thread_fork_client;` (two sites, ~931 and ~1002), or put `thread_goal_client,` wherever the struct is built field by field

## EP-7-S1
shared-edit apps/Model Plane/rust/services/model-gateway/tests/support/offline_downstreams.rs: after `state.thread_fork_client = ThreadForkServiceClient::new(offline_channel());` add `state.thread_goal_client = mp_contracts::model_plane::v1::thread_goal_service_client::ThreadGoalServiceClient::new(offline_channel());` (only if that contract test requires every session client to be offline)

## EP-7-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs: (1) at the end of FIELD_TREATMENTS after the context_refs row add `// EP-7 S1: a goal is user content in a durable store, so a ZDR turn naming one is refused.\n    ("goal_id", refused_feature("thread_goal")),`; (2) in invoke_field_names' destructure after `context_refs: _,` add `goal_id: _,` and in its vec after `"context_refs",` add `"goal_id",`; (3) in the refusal function after the context_refs check add `if request.goal_id.as_deref().is_some_and(|id| !id.trim().is_empty()) {\n        return feature(crate::thread_goal_routes::ZDR_FEATURE);\n    }`

## EP-7-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs: in the test helper make_request's InvokeRequest literal, after `context_refs: None,` add `goal_id: None,`

## EP-7-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/run_context.rs: in compose()'s `RunContext { ... }` literal, after `agent_roles: Vec::new(),` add `// Never composed here: the Do launch adds the thread goal after `prepare` (EP-7 S1).\n        goal_recitation: String::new(),`

## EP-7-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs: in DURABLE_MUTATIONS add `("thread_goal_routes.rs", "set_thread_goal"),` and `("thread_goal_routes.rs", "update_thread_goal"),` (only if read_sources includes that file)

## EP-7-S1
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs: append after the context_references Feature (last entry of FEATURES) `// EP-7 S1: a thread goal is user content in a new durable store (D7).\n    Feature {\n        key: "thread_goal",\n        label: "persistent goals",\n    },`. Then regenerate the golden with MP_UPDATE_GOLDEN=1 and confirm it is byte-identical to my hand edit (sha ae4473ce0a3ddd3d6d104aad3f90509014da63b60e2db27766983956708437d7)

## EP-7-S1
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/run_context.rs: add a `ZdrGoalRecitation` refusal (token run_context_zdr_goal_recitation, checked in the zdr block: `if !context.goal_recitation.is_empty() { return Err(RunContextRefusal::ZdrGoalRecitation); }`) and a `GoalRecitationTooLarge` refusal with `pub const MAX_GOAL_RECITATION_BYTES: usize = 8_192;` (the gateway's thread_goal_routes::MAX_RECITATION_BYTES), checked after the profile bound

## EP-7-S1
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts: (1) in ChatInvokeRequest after the `contextRefs?:` field add `/** The thread goal a Continue turn works toward (EP-7 S1). Sent as `goal_id` only when set, never on a zero-retention turn. */\n  goalId?: string`; (2) in the body builder after `...(contextRefs.length > 0 ? { context_refs: contextRefs } : {}),` add `// The goal of a Continue turn (EP-7 S1): omitted on every other turn, so an ordinary body is unchanged.\n    ...(request.goalId && !request.zdr ? { goal_id: request.goalId } : {}),`

## EP-7-S1
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after `'chat.pick_context_ref': ...` add `'chat.set_goal': exempt('agent_cannot_self_govern', 'context_write'),\n  'chat.update_goal': exempt('agent_cannot_self_govern', 'context_write'),\n  'chat.clear_goal': exempt('agent_cannot_self_govern', 'context_write'),\n  'chat.continue_goal': exempt('no_owner_executor', 'external_write'),`

## EP-7-S1
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: (1) in the useChatController() destructure after `cancelResearchPlan,` add `threadGoal, goalPending, goalError, goalEntryAvailable, saveGoal, setGoalPaused, clearGoal, continueGoal,`; (2) in selectDockedTrays({...}) add `goal: threadGoal(),`; (3) on <ComposerTrays> after `onCancelResearchPlan={cancelResearchPlan}` add `goalEntry={goalEntryAvailable()} goalPending={goalPending()} goalError={goalError()} goalBusy={isStreaming()} onSaveGoal={saveGoal} onContinueGoal={() => void continueGoal()} onPauseGoal={(paused) => void setGoalPaused(paused)} onClearGoal={() => void clearGoal()}`

## EP-7-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core environment block add `SESSION_CORE_THREAD_GOALS_ENABLED: ${SESSION_CORE_THREAD_GOALS_ENABLED:-false}`; in the model-gateway block add `MODEL_GATEWAY_THREAD_GOALS_ENABLED: ${MODEL_GATEWAY_THREAD_GOALS_ENABLED:-false}`. The owner adds both to .env.example

