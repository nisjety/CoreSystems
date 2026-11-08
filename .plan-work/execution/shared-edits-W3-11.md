## EP-14-S5
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/lib.rs: add line `pub mod run_timing;` right after `pub mod run_origin;` (line 28). Needed by session-core, model-gateway and the run_ledger_proto_roundtrip test.

## EP-14-S5
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs: in FEATURES, after the `Feature { key: "conversation_import", label: "conversation import", },` entry (about line 371), add `Feature { key: "run_inspection", label: "run timing and request inspection", },` with the comment `// EP-14 S5/S7: the run timing and request-inspection reads refuse a ZDR caller.` Order must match api/contracts/zdr-refusals.json, which already lists it last. Without this edit, Refusal::feature_not_supported("run_inspection") hits its debug_assert.

## EP-14-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/state.rs (CRLF): (1) import `run_ledger_service_client::RunLedgerServiceClient,` in the mp_contracts::model_plane::v1 use block. (2) Field after `pub event_log_client: EventLogClient<Channel>,`: `/// Session Core's RunLedgerService (EP-14 S5 run timing write and read) on the session_client endpoint.` then `pub run_ledger_client: RunLedgerServiceClient<Channel>,`. (3) In new() after `event_log_client: EventLogClient::new(session_channel.clone()),` add `run_ledger_client: RunLedgerServiceClient::new(session_channel.clone()),`. (4) In from_env, beside `let event_log_client = EventLogClient::new(Self::lazy_channel("SESSION_CORE_URL","SESSION_CORE_ADDR","http://localhost:9091")?);`, add the same for `let run_ledger_client = RunLedgerServiceClient::new(...)`. (5) After both `state.event_log_client = event_log_client;` lines (about 897 and 964), add `state.run_ledger_client = run_ledger_client;`. (6) Any with_nats constructor that lists every field also gets `run_ledger_client`.

## EP-14-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/park.rs (CRLF): in parked_response's RunAgentResponse literal, after `usage: None,` (line 534), add `        // The caller that holds the loop's round log sets it.` and `        round_timing: Vec::new(),`. In the test literal at line 1518, after `usage: None,`, add `                round_timing: Vec::new(),`. Without this, execution-core does not compile: RunAgentResponse gained field 12.

## EP-14-S5
shared-edit apps/Model Plane/go/pkg/zdrrefusal/zdr-refusals.json: copy apps/Model Plane/api/contracts/zdr-refusals.json byte-for-byte (adds the run_inspection feature). Re-pin go/pkg/zdrrefusal/zdr-refusals.json.sha256 and apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json.sha256 to `96cce805b28ece23aace053e6d2a7b726318effd4defe72113174e7128086515 *zdr-refusals.json` (LF ending). Then run scripts/tests/zdr-contract-sync.test.sh and compare against the D0-003 baseline by name.

## EP-14-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment block next to MODEL_GATEWAY_RUN_CONTEXT add `MODEL_GATEWAY_RUN_TIMING_PERSIST: ${MODEL_GATEWAY_RUN_TIMING_PERSIST:-false}`. In the session-core block next to SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED (line 1139) add `SESSION_CORE_RUN_LEDGER_READ: ${SESSION_CORE_RUN_LEDGER_READ:-false}`. In the execution-core block next to EXECUTION_EFFECT_LEDGER_ENABLED (line 1509) add `EXECUTION_ROUND_TIMING: ${EXECUTION_ROUND_TIMING:-false}`. Add compose-contract assertions if the gate requires them. The owner adds .env.example entries, including VITE_CHAT_RUN_WATERFALL for the SPA.

## EP-6-S8
shared-edit apps/Application Plane/convex-core/convex/agents.ts (CRLF, keep CRLF): append after getVersionForGateway (end of the EP-6 S7 block) this query:
/**
 * What the agent builder (EP-6 S8) reads before the gateway authorises an edit: the draft, the
 * pointer, the legacy fields a first draft is seeded from, and the two facts its gate needs (the
 * creator's Control subject, and the Space a room-born agent was created in). An agent of another
 * organisation reads as null, the same answer as an unknown id.
 */
export const builderDefinitionForGateway = query({
  args: {
    serviceKey: v.string(),
    externalAuthId: v.string(),
    externalOrgId: v.string(),
    agentId: v.id("agents"),
  },
  handler: async (ctx, args) => {
    assertServiceKey(args.serviceKey);
    const organization = await requireGatewayMember(ctx, args.externalAuthId, args.externalOrgId);
    const agent = await ctx.db.get(args.agentId);
    if (!agent || agent.orgId !== organization._id) return null;
    const creator = agent.createdBy ? await ctx.db.get(agent.createdBy) : null;
    // createSpaceAgentForGateway writes the agent and its first binding in one transaction with
    // the same timestamp and no createdBy; that binding's Space is where the agent was born.
    const origin = agent.createdBy
      ? null
      : (
          await ctx.db
            .query("spaceAgentBindings")
            .withIndex("by_external_org", (q: any) => q.eq("externalOrgId", args.externalOrgId))
            .collect()
        ).find((binding: any) => binding.agentId === args.agentId && binding.createdAt === agent.createdAt);
    return {
      agentRef: agent._id,
      name: agent.name,
      systemPrompt: agent.systemPrompt ?? null,
      model: agent.model ?? null,
      publishedVersion: agent.publishedVersion ?? null,
      draft: agent.draft ?? null,
      creatorExternalAuthId: creator?.externalAuthId ?? null,
      originSpaceRef: origin?.spaceRef ?? null,
    };
  },
});
Plus one case in test/agent-definition-versions.test.cjs: another org's agent reads as null; a room-born agent reports its origin Space and no creator.

## EP-6-S8
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts (LF): immediately after the line `  'agents.installations.list': exempt('no_owner_executor', 'read'),` add three lines: `  'agents.draft.save': exempt('agent_cannot_self_govern', 'external_write'),` / `  'agents.publish': exempt('agent_cannot_self_govern', 'external_write'),` / `  'agents.versions.list': exempt('no_owner_executor', 'read'),` (MODEL_ACCESS is Record<ActionId,...>, typecheck fails without them).

## EP-6-S8
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/streams.rs (stream_chat): immediately before `    let wake_intent = match crate::domains::spaces::inject_mentioned_space_agent(` insert:
    // EP-6 S8: a builder preview becomes a temporary read-only turn with the draft persona. Must run
    // before the mention and thread injectors; a no-op unless AGENT_DEFINITION_VERSIONS is `true`.
    if let Err((status, body)) = crate::domains::agent_actions::inject_agent_draft_preview(
        &state,
        &user,
        &org_id,
        &mut outbound_body,
    )
    .await
    {
        return (status, body).into_response();
    }
Then drop the `#[allow(dead_code, ...)]` line above inject_agent_draft_preview in agent_actions.rs.

## EP-12-S6
shared-edit apps/Model Plane/rust/services/inference-core/src/grpc.rs: in `impl InferenceService`, change `async fn authorize<T: TenantRequest>(` to `async fn authorize<T: TenantRequest + 'static>(`, and directly after the line `crate::metered::note_caller(request.extensions(), &principal);` insert:
        // EP-12 S6: an API-key principal reaches only the chat turn (Infer, InferStream, both
        // `pb::InferRequest`); every other RPC, including request types added later, is refused.
        principal.authorize_api_key_rpc(
            std::any::TypeId::of::<T>() == std::any::TypeId::of::<pb::InferRequest>(),
        )?;

## EP-12-S6
shared-edit apps/Model Plane/rust/services/inference-core/src/entitlement.rs: in `pub async fn resolve`, replace `self.resolve_for(&principal.org_id, principal.user_id.as_deref())` with `self.resolve_for(&principal.org_id, principal.entitlement_user_id())`. Without this, an api_key principal (user_id None) resolves as service_exempt.

## EP-12-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the inference-core service `environment:` block, next to `INFERENCE_CORE_AUTH_AUDIENCE`, add the line `      INFERENCE_API_KEY_PRINCIPAL: ${INFERENCE_API_KEY_PRINCIPAL:-false}`

## EP-12-S8
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml: in the gateway service `environment`, right after the line `MAIL_TASK_GATEWAY_BRIDGE_SERVICE_ID: ${MAIL_TASK_GATEWAY_BRIDGE_SERVICE_ID:-verevon-gateway}`, add these three lines (same indentation): `# EP-12 S8: public Agent API ingress (/api/agent/v1/*); exchanges x-api-key at auth-core with the verevon-gateway principal above.` then `GATEWAY_AGENT_API_ENABLED: ${GATEWAY_AGENT_API_ENABLED:-false}` then `GATEWAY_AGENT_API_RPM_PER_KEY: ${GATEWAY_AGENT_API_RPM_PER_KEY:-60}`

