## EP-2-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, right after the line `      DEEP_RESEARCH_SECOND_PASS: ${DEEP_RESEARCH_SECOND_PASS:-false}`, add:
      # EP-2 S7: org knowledge per sub-query in deep research; knowledge_only research allowed. Only the literal true.
      DEEP_RESEARCH_KNOWLEDGE_SOURCES: ${DEEP_RESEARCH_KNOWLEDGE_SOURCES:-false}
      # EP-2 S7: selected connected accounts in deep research (also needs CONNECTOR_ASK_READS_ENABLED). Only the literal true.
      DEEP_RESEARCH_CONNECTOR_SOURCES: ${DEEP_RESEARCH_CONNECTOR_SOURCES:-false}

## EP-2-S7
shared-edit apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs: in cases(), directly after the case ("retrieval_scope", with("retrieval_scope", json!("auto")), None,), insert:
        // e_zdr_token_knowledge_only_deep_research_refused (EP-2 S7): refused for `deep_research`
        // before any retrieval, whatever DEEP_RESEARCH_KNOWLEDGE_SOURCES says.
        (
            "retrieval_scope",
            json!({"content": "hello", "deep_research": true, "retrieval_scope": "knowledge_only"}),
            refused("deep_research"),
        ),
        // e_zdr_token_deep_research_connector_sources_refused (EP-2 S7)
        (
            "connector_scope",
            json!({"content": "hello", "deep_research": true, "connector_scope": ["conn_1"]}),
            refused("deep_research"),
        ),

## EP-2-S7
shared-edit apps/Model Plane/docs/ZDR.md: after the claim-ledger paragraph (EP-2-S6), add a paragraph: 'Knowledge and connector sources in deep research (EP-2-S7, behind DEEP_RESEARCH_KNOWLEDGE_SOURCES / DEEP_RESEARCH_CONNECTOR_SOURCES): a Data Plane retrieval per sub-query and, gated, one connected-app search per selected account; no new store. A ZDR turn is refused through deep_research before the scope or selection is read (knowledge_only included), and SourcePlan::for_turn returns web-only for any ZDR turn. Connector citations carry metadata only and never a quote; nothing new is persisted beyond the existing citations and report artifact.'

## EP-10-S8
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-thread-history.ts: in `export type ChatThreadTranscriptTurn = {` add the field `  /** The connected-app suggestion card (EP-10 S8), read back as a restored note. */
  connectorSuggestion?: unknown` (REQUIRED: chat-normalizers.ts now reads and writes turn.connectorSuggestion on the transcript type, so typecheck fails without it).

## EP-10-S8
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts: in the handlers type, next to onUserQuestion, add `  /** The connected-app suggestion card (EP-10 S8). Ungated control event. */
  onConnectorSuggestion?: (event: { suggestionId?: string; provider?: string; reason?: string; requestId?: string }) => void`; in dispatchEvent, after `case 'user_question': {...}`, add `    case 'connector_suggestion': {
      handlers.onConnectorSuggestion?.({ suggestionId: str(payload.suggestion_id), provider: str(payload.provider), reason: str(payload.reason), requestId: str(payload.request_id) })
      break
    }`. This is REQUIRED for gen-sse-event-taxonomy.py --check (producer->consumer coverage).

## EP-10-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/room_run.rs ROOM_EVENT_POLICY: after `("follow_ups", Disposition::Drop),` add `    ("connector_suggestion", Disposition::Drop),` (the leak-guard test reads every name in the taxonomy doc).

## EP-10-S8
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/use-chat-controller.ts (or wherever onUserQuestion is wired): onConnectorSuggestion: (e) => { const s = normalizeConnectorSuggestion(e); if (s) set the streaming assistant turn's `connectorSuggestion = s` }; the card's onUse(connectionId) validates `chat.accept_connector_suggestion` {path:'use', suggestionId, provider, connectionId}, then sets the turn's connector scope to [connectionId] (chat.set_connector_scope) and re-sends the last user message once.

## EP-10-S8
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx: under an assistant turn with `turn.connectorSuggestion`, render `<ConnectorSuggestionCard suggestion={turn.connectorSuggestion} orgId={orgId} spaceScoped={!!spaceId} temporary={zdr/temporary} onUse={...} />`.

## EP-10-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml model-gateway environment block: `CONNECTOR_SUGGESTIONS_ENABLED: ${CONNECTOR_SUGGESTIONS_ENABLED:-false}`.

## EP-10-S9
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPage.tsx: in the destructuring of useChatController() that ends at line 199 (`} = useChatController()`, with `loadEarlier,` at line 181), add `connectTurn,`, `pendingConnectIntent,`, `resumeConnectIntent,` and `dismissConnectIntent,`; pass them with the other chat accessors to the component that renders the connector_suggestion part (ChatMessages.tsx is the likely host; EP-10-S8's file list does not name it).

## EP-10-S9
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ConnectorSuggestionCard.tsx (created by EP-10-S8; apply after S8 merges): (a) import { continueSavedConnectIntent, runConnectIntent, type ConnectionRef } from '@/shared/integrations/connect-intent'; keep S8's listConnections, startConnectSession, reserveDirectOauthWindow, runDirectOauthWindow and connectBundlesForSources (from '@/shared/integrations/connect-bundles'). (b) Connect click: first `const turn = props.connectTurn()` and return when it is null; then, synchronously in the click, `const reserved = reserveDirectOauthWindow()`; then `const outcome = await runConnectIntent({ ...turn, suggestionId: props.suggestion.suggestionId, provider, knownIds: new Set(connections.map((c) => c.id)), openConnect: async () => { const session = await startConnectSession(orgId, provider, { bundles: connectBundlesForSources(provider) }); await runDirectOauthWindow({ connectUrl: session.connectUrl || session.redirectUrl, sessionToken: session.sessionToken || session.id }, reserved) }, listConnections: () => listConnections(orgId), resume: (intent, connectionId) => props.resumeConnectIntent(intent, connectionId, providerLabel) })`. Wrap in try/catch: a rejected resume rethrows. (c) Outcomes: 'failed' shows outcome.error.message (the provider text, as InboxPage does); 'pending' shows 'Connected. Finish connecting when the chat is ready.'; 'resumed' needs no line. (d) Finish connecting: show when props.pendingConnectIntent()?.suggestionId === props.suggestion.suggestionId and this card is not running its own connect. Continue calls continueSavedConnectIntent({ threadId: intent.threadId, listConnections: () => listConnections(orgId), resume: (saved, id) => props.resumeConnectIntent(saved, id, providerLabel) }) inside try/catch (a failed list shows an error and changes nothing); 'not_connected' shows the Connect button; 'none' hides the state; 'pending' shows the (c) line; 'Not now' calls props.dismissConnectIntent(). (e) Render none of this where props.connectTurn() is null (temporary chat, Space thread, foreign thread, nothing asked yet).

## EP-10-S9
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ConnectorSuggestionCard.test.tsx (created by EP-10-S8): add the S9 cases: a storage that throws still runs the connect; a successful connect resends once (resumeConnectIntent called once); a reload with a saved intent for this suggestion shows Continue and sends nothing on render; two quick Continue clicks send once; a 16 minute old intent shows no Finish state.

## EP-9-E3
shared-edit apps/Model Plane/rust/services/execution-core/src/lib.rs: add `pub mod egress_proxy;` after `pub mod control_http_client;`

## EP-9-E3
shared-edit apps/Model Plane/rust/Cargo.toml: in [workspace] members add `    "tools/mp-netbridge",` after `    "services/execution-core",` (Cargo.lock regenerates)

## EP-9-E3
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs: (1) fn execute_code_interpreter gains a last param `egress: &crate::egress_proxy::CallEgress`, and its body calls `crate::code_interpreter::run_with_egress(tool_input, run_id, step_id, workspace_root, ro_inputs, skills, lease_id, egress)` instead of run_with_skills. (2) In the CODE_INTERPRETER_TOOL else-branch (the Space/no-thread call, just before `let execution = execute_code_interpreter(`), add `let egress = crate::egress_proxy::run_egress().call_egress(org_id, run_id);` and pass `&egress` as the new last argument. (3) In execute_thread_code_interpreter, pass `&crate::egress_proxy::CallEgress::off()` (thread leases are not tiered until D2-145/147/149 close).

## EP-9-E3
shared-edit apps/Model Plane/rust/services/execution-core/src/sandbox_lease.rs: in the Space acquire fn, right after the `.inspect_err(... "sandbox-manager AcquireLease failed")?;` and before `let lease = SandboxLease {`, insert:
    // EP-9 E3: the tier sandbox-manager recorded and the Space list the decision was signed over, for
    // this run's code_interpreter calls. Ignored unless the run registered for egress (switch on).
    crate::egress_proxy::run_egress().record_lease(
        org_id,
        run_id,
        crate::egress_proxy::lease_tier(response.egress_tier),
        &claims.egress_allowlist,
    );

## EP-9-E3
shared-edit apps/Model Plane/rust/services/execution-core/src/capability_client.rs: in `pub struct SpaceCapabilityClaims`, after `pub credential_mode: String,`, add:
    /// The Space egress allow-list Control signed over (EP-9 E2), present only at the `allowlist` tier;
    /// carried through unchanged so sandbox-manager's digest check sees the list Control bound.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress_allowlist: Vec<String>,
In the test literal `SpaceCapabilityClaims {` (~line 367), add `egress_allowlist: Vec::new(),`.

## EP-9-E3
shared-edit apps/Model Plane/go/services/sandbox-manager/internal/authz/capability_verifier.go: (1) In CapabilityExpectation, after `Now       time.Time`, add the field `GrantEgressAtOff bool` with the comment `// GrantEgressAtOff is SANDBOX_MANAGER_EGRESS_TIERS (EP-9 E3, D-E2): an egress-capable claim the decision does not grant verifies at off instead of being refused, as an ungranted process claim does.` (2) In Verify, change the call to `verifiedEgressTier(decision, claims, expect.GrantEgressAtOff)`. (3) Change the signature to `func verifiedEgressTier(decision spaceCapabilityDecision, claims SpaceCapabilityClaims, grantAtOff bool) (string, error)`. (4) In the v1 branch, change the condition to `claims.Egress != egressDisabledByDefault && !hasPermission(decision.Permissions, "space:egress") && !grantAtOff`. (5) In the v2 tail, change it to `if claimRank > 0 && granted == EgressTierOff && !grantAtOff {`. (6) In the doc comment, replace the final sentence with `An egress-capable claim with no tier granted is refused outright, as in v1, unless grantAtOff (D-E2): then it verifies at off.` Add a capability_verifier_test.go case: a v2 'registries' claim with no tier permission errors with grantAtOff=false and returns off, nil with grantAtOff=true. Run gofmt.

## EP-9-E3
shared-edit apps/Model Plane/go/services/sandbox-manager/internal/lease/lease.go: (1) Lease struct: after AudienceRevision add `EgressTierPermitted string` with the comment `// EgressTierPermitted is the egress tier this lease permits (migration 0008, EP-9 E3), decided once at AcquireLease. Set by Create; not read back by scanLease, so a lease read from the table carries "", which every reader treats as off.` (2) SpaceGrant: add `EgressTierPermitted string // empty is recorded as off`. (3) In Create, before building l: `egressTier := grant.EgressTierPermitted; if egressTier == "" { egressTier = "off" }`; set `EgressTierPermitted: egressTier,` in the Lease literal. Append `, egress_tier_permitted` to the INSERT column list after created_at, `, $14` to VALUES, and `l.EgressTierPermitted` as the last arg (positions 0-12 unchanged, so lease_test.go still holds; add one assertion that args[13] == "off" for SpaceGrant{}).

## EP-9-E3
shared-edit apps/Model Plane/go/services/sandbox-manager/internal/server/memory_store.go: in MemoryLeaseStore.Create's lease.Lease literal, after `AudienceRevision:   grant.RecipientAudienceRevision,`, add `EgressTierPermitted: grant.EgressTierPermitted,` (gofmt).

## EP-9-E3
shared-edit apps/Model Plane/go/services/sandbox-manager/cmd/main.go: after the `if capabilityVerifier != nil { ... }` block, add `// EP-9 E3 (D-E2): grant an egress-capable lease at off and record the tier instead of refusing it.` followed by `server = server.WithEgressTiers(egressTiersEnabled(os.Getenv("SANDBOX_MANAGER_EGRESS_TIERS")))`. At the end of the file add `// egressTiersEnabled reads SANDBOX_MANAGER_EGRESS_TIERS strictly: only the literal "true" enables it.` and `func egressTiersEnabled(value string) bool { return value == "true" }` (gofmt).

## EP-9-E3
shared-edit apps/Model Plane/rust/services/execution-core/Dockerfile: builder stage, change to `cargo build --release --bin execution-core --bin mp-netbridge` and append `&& cp target/release/mp-netbridge /usr/local/bin/mp-netbridge`. Runtime stage, after the execution-core COPY, add `COPY --from=builder /usr/local/bin/mp-netbridge /usr/local/bin/mp-netbridge` (egress_proxy::NETBRIDGE_BIN; without it the host claims off).

## EP-9-E3
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core environment, add `EXECUTION_SANDBOX_EGRESS: ${EXECUTION_SANDBOX_EGRESS:-off}`; in the sandbox-manager environment, add `SANDBOX_MANAGER_EGRESS_TIERS: ${SANDBOX_MANAGER_EGRESS_TIERS:-false}`. The owner adds both to .env.example.

## EP-9-E3
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md §G1: AllowDomains is now enforced by execution-core's per-call proxy (egress_proxy.rs). The HTTPS_PROXY variable is no longer trusted.

## EP-9-E3
shared-edit apps/Model Plane/ZDR.md: add a row: sandbox egress (EP-9 E3) is held at off for ZDR and waived runs (egress_proxy::run_ceiling). The proxy keeps host counts only, in the tool result. No refusal code.

## EP-12-S14
shared-edit apps/Model Plane/rust/services/model-gateway/src/state.rs (LF): (1) in the mp_contracts::model_plane::v1 import list, after the line `    thread_export_service_client::ThreadExportServiceClient,` add `    thread_share_service_client::ThreadShareServiceClient,`; (2) in struct AppState after `    pub thread_export_client: ThreadExportServiceClient<Channel>,` add `    /// Session Core's ThreadShareService on the session_client endpoint (EP-12 S14).` and `    pub thread_share_client: ThreadShareServiceClient<Channel>,`; (3) in the default constructor after `            thread_export_client: ThreadExportServiceClient::new(session_channel.clone()),` add `            thread_share_client: ThreadShareServiceClient::new(session_channel.clone()),`; (4) in the env constructor after the `let thread_export_client = ThreadExportServiceClient::new(Self::lazy_channel("SESSION_CORE_URL", "SESSION_CORE_ADDR", "http://localhost:9091")?);` block add the same block named `thread_share_client` using ThreadShareServiceClient; (5) in BOTH assignment lists (two occurrences of `            state.thread_export_client = thread_export_client;`) add `            state.thread_share_client = thread_share_client;` after it.

## EP-12-S14
shared-edit apps/Model Plane/deploy/docker-compose.yml (LF): after `      MODEL_GATEWAY_THREAD_EXPORT: ${MODEL_GATEWAY_THREAD_EXPORT:-false}` add `      # EP-12 S14: the owner's share snapshot routes (/v1/threads/:id/snapshots, /v1/snapshots/:id). Needs SESSION_THREAD_SHARES_ENABLED. Only the literal true.` and `      MODEL_GATEWAY_THREAD_SHARES: ${MODEL_GATEWAY_THREAD_SHARES:-false}`; after `      SESSION_THREAD_EXPORT_ENABLED: ${SESSION_THREAD_EXPORT_ENABLED:-false}` add `      # EP-12 S14: the ThreadShareService snapshot store. Only the literal true; stays off until the share-model ADR (OD-5) is accepted.` and `      SESSION_THREAD_SHARES_ENABLED: ${SESSION_THREAD_SHARES_ENABLED:-false}`.

## EP-12-S14
shared-edit apps/Model Plane/docs/ZDR.md (LF): in the boundary table, after the row starting `| **Artifact revisions (`artifact_revisions`, session-core, EP-3 S2)** |` add: `| **Share snapshots (`thread_share_snapshots`, session-core, EP-12 S14)** | No snapshot of a ZDR or waived caller's thread, nor of any thread holding a non-persistent run | Built dark behind `SESSION_THREAD_SHARES_ENABLED` and `MODEL_GATEWAY_THREAD_SHARES` (both off; flip waits on the share-model ADR, OD-5). Taking one refuses a ZDR or waived caller with `zdr_feature_not_supported` (`conversation_share`) at the gateway and again in session-core, and refuses a Space thread or a thread with any non-persistent run (`thread_not_exportable`) inside the export transaction. Revoke deletes the body and digest; rows go with their thread (ON DELETE CASCADE and `delete_thread_rows`) and with the org purge. Owner-only reads. |`

## EP-12-S15
shared-edit apps/Model Plane/rust/services/session-core/src/thread_export.rs: inside `impl ThreadExportService for ThreadExportGrpc`, after the closing brace of `async fn export_thread(...)` and before the impl's closing brace, add (LF file):

    async fn import_thread(
        &self,
        request: Request<pb::ImportThreadRequest>,
    ) -> Result<Response<pb::ImportThreadResponse>, Status> {
        crate::thread_import::serve(&self.pool, request).await
    }
(required to compile: the proto adds rpc ImportThread to ThreadExportService)

## EP-12-S15
shared-edit apps/Model Plane/docs/ZDR.md: in the feature table beside the Skill import row add: | **Conversation import (EP-12 S15)** | Refused for ZDR and waived callers | session-core ImportThread refuses a zero-retention or waived (DEC-32) caller with zdr_feature_not_supported, feature conversation_import, before any read or write; model-gateway POST /v1/threads/import refuses a ZDR token before reading the body (no posture-matrix row). Store: imported turns are new messages rows of a new caller-owned thread (memory off, user role, metadata.imported), erased with the thread and the org. Built dark behind SESSION_THREAD_IMPORT_ENABLED (off) and MODEL_GATEWAY_THREAD_EXPORT (off). |

## EP-12-S15
shared-edit apps/Model Plane/deploy/docker-compose.yml: session-core environment: `SESSION_THREAD_IMPORT_ENABLED: ${SESSION_THREAD_IMPORT_ENABLED:-false}`

