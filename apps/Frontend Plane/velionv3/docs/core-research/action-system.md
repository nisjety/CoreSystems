# Velion v3 Action System

> Verified 2026-07-11 (source-only; SPA :5173 + gateway :3185 containers down this pass).
> The action system is now gateway-backed and executes against real plane backends. The earlier
> "synthetic execution / no transport" description has been superseded — see the corrected
> Execution Path, Relationships, and Audit sections below.

## Current State

The action system is both a UI product contract and a model-visible tool contract, and it now has a
live transport: descriptors dispatch through the Rust gateway to real plane backends.

`src/shared/actions/action-registry.ts` defines ~22 Zod-validated action descriptors (verified
2026-07-11) across several domains, including:

- `knowledge.*` — `recrawl_source`, `scrape_url`, `crawl_site`, `import_source`, `upload_files`, `connect_source`
- `operating_map.*` — `generate`, `refresh`, `review_proposal`, `create_agent_blueprint`
- `tickets.*` — `create`, `classify_conversation`, `update`, `assign`, `link_resource`, `resolve`
- `social.*` — `create_draft`, `schedule_post`, `publish_post`
- `brreg.lookup_organization`
- `workflows.toggle_policy`

The previously documented `inbox.draft_reply` and `agents.deploy_channel` descriptors no longer exist
in the registry. Each descriptor includes owner plane, risk, approval requirement, reversibility,
input schema, and output schema (`src/shared/actions/types.ts` `ActionDescriptor`).

`src/shared/actions/agent-tools.ts` converts the same descriptors into:

- AG-UI/TanStack tool specs with JSON-schema parameters for the Velion Gateway request body.
- TanStack `toolDefinition()` entries carrying approval and owner-plane metadata.
- A deduped selected-tool list that merges built-in web search, selected composer actions, and explicit advanced tools.

This is the Velion v3 AI-first invariant for the chat surface: actions that can be presented in the UI
are also advertised to the model through the same descriptor contract.

## Execution Path

`previewAction()` / `executeAction()` now live in `src/shared/actions/action-client.ts` (not fabricated
inline). `ActionCommand.tsx` calls:

- `previewAction()` to validate inputs against the descriptor schema and surface approval/cost metadata
  (approval requirement and estimated cost are read locally from descriptor metadata for the preview).
- `executeAction()` to validate again and dispatch to the gateway.

`executeAction()` (verified 2026-07-11):

- validates input with the descriptor's Zod `inputSchema`; on failure it throws a typed
  `Invalid action input for <id>` error.
- checks a `LIVE_ACTIONS` allowlist. For actions **not** in the allowlist it throws an honest
  `action_not_available: "<id>" has no gateway implementation` error — there is **no** client-side
  fabrication of `run_*` / `audit_*` IDs (the earlier synthetic behavior was removed and is explicitly
  guarded against in comments).
- for live actions it `POST`s to `/api/v1/actions/execute` with body `{ actionId, input }` and header
  `x-velion-org-id: <actor.orgId>`, returning the server-provided `ActionExecution`
  (`runId`, `status`, `auditId`, `eventStream`).

It therefore **does** reach the planes, via the gateway dispatcher below.

## Relationships

- Action descriptors name owner planes, and transport to those planes now exists: the gateway
  `domains/actions.rs` route `/api/v1/actions/execute` → `handlers::execute_action` matches on
  `action_id` and calls per-action dispatchers in `domains/actions/dispatchers.rs` (~36 KB). These map
  owner planes to real backends: `knowledge.*` → Quarry/Ingestion + imports-core, `tickets.*` →
  conversation-core-go, `social.*` → social-core (which enforces the real approval gate),
  `brreg.lookup_organization` → org-core, `operating_map.*` → Data/Model Plane, `workflows.toggle_policy`.
  Unmapped actions return `501 not_implemented`; `knowledge.upload_files` returns an honest
  `422 upload_requires_multipart` redirect to the multipart imports route rather than faking success.
- Cross-tenant scoping (IDOR re-check, verified 2026-07-11): the dispatchers derive org from
  `upstream::authorized_org_id(state, user)`, which reads the org from the **validated session**
  (active org / session-context `orgId`). The client-sent `x-velion-org-id` header is **not** trusted
  for scoping — the previously reported forged-header IDOR remains closed.
- The context-pack builder (`src/shared/context-packs/context-pack.ts`) exposes available action IDs
  (`actionRegistry.map(a => a.id)`) to the model context rail with an `ids-and-summaries-only` redaction policy.
- The AG-UI chat adapter sends descriptor-derived tools to `/api/v1/ag-ui/stream` as TanStack-compatible `RunAgentInput.tools`.
- The Rust gateway (`domains/ag_ui.rs`) accepts that `RunAgentInput`, sanitizes tool descriptors, normalizes options from `forwardedProps`, and forwards a Model Plane invoke body.
- Cost policy exists locally in `src/shared/cost/cost-policy.ts` (a pure tier-selection function) but is still not connected to a model gateway.

## Stub, Mock, Placeholder, and Partial Audit

Corrected 2026-07-11 — the earlier "synthetic" findings are now resolved:

- Action execution is **no longer synthetic**: `executeAction()` dispatches to the gateway
  `/api/v1/actions/execute` and the response (`runId`, `status`, `auditId`, `eventStream`) is
  server-provided by the dispatchers, not fabricated client-side.
- Approval is computed locally from descriptor metadata **only for the preview**; the authoritative
  approval gate is enforced server-side (e.g. social-core for `social.*` publish/schedule).
- Audit IDs come from the server, not reserved locally.
- Event-stream routes exist in the gateway (e.g. `chat/streams.rs` `GET /api/v1/runs/:run_id/events`,
  plus knowledge/import run-event streams), so returned `eventStream` URLs resolve.
- Tool advertisement is live for chat, and gateway-side execution has now landed for the descriptors
  in `LIVE_ACTIONS` (`action-client.ts`).

Remaining gaps:

- `cost-policy.ts` is still a standalone local function, not wired into the model gateway.
- Not every registry descriptor has a live dispatcher; unmapped ones fail honestly (`501` /
  `action_not_available`) rather than pretending to run.

## Notes

The contract-first direction has been realized: the gateway-backed action executor mapping descriptor
owner planes to real Application/Model/Ingestion/Data/Control APIs now exists in
`apps/gateway/src/domains/actions/`. Remaining follow-ups are wiring `cost-policy.ts` to the model
gateway and filling in dispatchers for any descriptors still returning `501`.
