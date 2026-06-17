# Velion v3 Action System

## Current State

The action system is now both a UI product contract and a model-visible tool contract. It is not a
full distributed action runtime yet.

`src/shared/actions/action-registry.ts` defines Zod-validated action descriptors for:

- `knowledge.recrawl_source`
- `inbox.draft_reply`
- `agents.deploy_channel`
- `workflows.toggle_policy`

Each descriptor includes owner plane, risk, approval requirement, reversibility, input schema, and output schema.

`src/shared/actions/agent-tools.ts` converts the same descriptors into:

- AG-UI/TanStack tool specs with JSON-schema parameters for the Velion Gateway request body.
- TanStack `toolDefinition()` entries carrying approval and owner-plane metadata.
- A deduped selected-tool list that merges built-in web search, selected composer actions, and explicit advanced tools.

This is the Velion v3 AI-first invariant for the chat surface: actions that can be presented in the UI
are also advertised to the model through the same descriptor contract.

## Execution Path

`ActionCommand` calls:

- `previewAction()` to validate inputs and show approval/cost metadata.
- `executeAction()` to validate again and return execution metadata.

`executeAction()` currently:

- generates a local `run_*` ID with `crypto.randomUUID()`
- generates a local `audit_*` ID
- returns `waiting_approval` when the descriptor requires approval
- returns `queued` when it does not
- sets `eventStream` to `/api/v1/runs/{runId}/events`

It does not call Model Plane, Application Plane, Ingestion Plane, or Control Plane.

## Relationships

- Action descriptors name owner planes, but there is no transport to those planes.
- The context-pack builder exposes available action IDs to the model context rail.
- The AG-UI chat adapter sends descriptor-derived tools to `/api/v1/ag-ui/stream` as TanStack-compatible `RunAgentInput.tools`.
- The Rust gateway accepts that `RunAgentInput`, sanitizes tool descriptors, normalizes options from `forwardedProps`, and forwards a Model Plane invoke body.
- Cost policy exists locally in `src/shared/cost/cost-policy.ts` but is not connected to a model gateway.

## Stub, Mock, Placeholder, and Partial Audit

- Action execution is synthetic.
- Approval status is computed locally from descriptor metadata.
- Audit IDs are reserved locally but not persisted.
- Event stream URLs are returned but no event-stream route exists in this app.
- Tool advertisement is live for chat, but actual tool execution still depends on Model Plane/gateway action execution landing behind the descriptor IDs.

## Notes

This is a good contract-first direction. The next real implementation step would be a gateway-backed action executor that maps descriptor owner planes to actual Application/Model/Ingestion APIs.
