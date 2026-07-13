# Frontend v3 ↔ Model Plane capability handoff — 2026-07-13

Status: source credential and browser-ownership patches verified; capability
authority, retention policy, durable browser ownership, UX, and live deployment
remain release-blocking. This is not a claim that chat or shipping works live.

## Ownership and product policy

- Model Plane owns executable capability identity, health, policy, approval
  requirement, and machine-readable unavailability reason.
- Frontend v3 owns intentional user selection and presentation. Plain chat may
  remain tool-free; do not silently turn every message into a costly agent run.
- Browse, an explicit action selection, Plan mode, and Agent Run Console are
  distinct choices. Approval-required actions must use the governed agentic
  path and durable HITL; the inline direct loop is read-only containment.
- Shipping is not currently a selectable chat action, and no Velion Visma MCP
  runtime exists. Present Visma as `not_configured`, never connected/working.

## Source patch delivered

The Rust same-origin gateway now treats the Model Plane as a closed audience
set and requests distinct credentials for:

| Target | Audience | Forwarding contract |
|---|---|---|
| Model Gateway | `model-gateway` | legacy-compatible Model endpoint; public request bearer |
| Inference | `inference-core` | `x-inference-authorization` |
| Execution | `execution-core` | `x-execution-authorization` |
| Session | `session-core` | `x-session-authorization` |
| Capability | `capability-core` | target bearer/header on capability proxies |
| Cost | `cost-core` | target bearer/header on cost proxies |
| Data Plane | `data-plane` | target bearer/header on retrieval/graph calls |

Auth Core recognizes only this bounded Model-service set, issues audience-
specific scopes, forces the interactive issuer's ZDR claim, and returns an
explicit error for unsupported audiences. Required downstream credential
issuance now returns 503 instead of silently omitting a bearer. Tokens are not
interchangeable between targets.

Browser sessions and AI run IDs are now recorded against the server-validated
user and authoritative organization. Every action, tab, control, artifact,
frame, DevTools, close, AI-start/control, and WebSocket-upgrade path checks that
owner before contacting an upstream. Run-ID collision and poisoned/missing
ownership state fail closed. The store is still process-local: restart loses
ownership metadata, and multi-instance/HA requires a durable shared store.

There is also a source-level policy contradiction: Auth Core currently marks
every delegated token `zdr=true`, while Session Core and Letta correctly reject
durable ZDR work and no provider deployment is confirmed eligible. Do not
remove the downstream gates to make chat work; the issuer/product retention
policy must be resolved explicitly.

Verification on the dirty 2026-07-13 source tree:

- Auth Core: three focused token/role suites, **40** tests passed; production
  build passed.
- Frontend Rust gateway: browser **53/53**, full gateway **235/235**, check and
  format pass; new ownership helpers measured **105/105 lines (100%)**.
- Model Gateway: full package suite **407** passed, including **21/21**
  authenticated HTTP/SSE invoke-chain, **3/3** gRPC compatibility, and **5/5**
  signed-token orchestration HTTP tests. A granted retry with unknown execution
  delivery returns explicit gRPC `Unavailable` / HTTP 503 and does not request
  another resume.
- Session Core: **122** tests passed with zero failures and five database-gated
  cases ignored; only the durable approval CAS winner emits decision/resume
  events. Crash-safe delivery still requires an outbox/reconciler.
- Execution Core: **201** tests passed with one live Quarry E2E ignored; approval
  replay can resume only `AwaitingApproval`/`Paused`, never terminal/running/
  unknown runs.

## Capability contract delivered in source, authority still required

Model Plane now has an additive response with server-time health freshness,
version-bound attestation CAS, safe reason codes, approval/execution/cost state,
and audit. The same response must still be made authoritative for composer,
Browse, Plan, Agent Run Console, MCP settings, model offers, policy, and
execution. Minimum per-capability fields:

```json
{
  "id": "shipping.get_quotes",
  "state": "available",
  "reason_code": null,
  "reason": null,
  "requires_approval": false,
  "execution_mode": "direct_read",
  "cost_class": "bounded",
  "health_checked_at": "2026-07-13T00:00:00Z"
}
```

Allowed states are `available`, `disabled`, `unhealthy`,
`approval_required`, `unavailable`, and `not_configured`. The frontend must not
derive availability from a static action registry alone. A disabled or broken
capability remains explainable in the UI but cannot be submitted. At present,
Execution Core still has a hardcoded catalog, capability policy does not fully
govern dispatch, global health-reporter provisioning is absent, and durable
List/Get/model behavior needs compatibility tests. Capability policy currently
accepts only `global` and verified-tenant `org`; caller-supplied agent/run/
thread/workspace/user scopes are intentionally rejected until the request has
trusted server-derived subject identity. Migration `0007` has regression and
integration-gated fixtures but no release-Postgres execution proof.

## Remaining Frontend patch

1. Add an explicit Actions affordance only after policy/model offers/execution
   consume the authoritative contract;
   keep ordinary no-tool chat unchanged.
2. Add shipping read actions only after Model Plane exposes the stable tool
   identity and a real gateway-to-shipping executor. Booking/writes must be
   approval-required and agentic.
3. Select agentic mode automatically when the chosen capability contract says
   `requires_approval`; never infer risk from display text or client-only data.
4. Render health/unavailability reason and cost/approval implication before
   submission.
5. Remove duplicate static truth or make it a tested projection of the
   authoritative response.

## Release tests

The cross-plane gate must cover plain no-tool chat, Browse, explicit read tool,
Plan, Agent Run Console, shipping quote, shipping write pause/grant/deny,
unhealthy MCP, Visma `not_configured`, wrong audience, missing/failed token
issuance, wrong tenant, and an approval-bypass attempt. No item is complete
until the running stack—not only mocks or source tests—produces the expected
positive and negative result.

Current blockers are tracked in the Model Plane
[status](../../../Model%20Plane/MODEL_PLANE_STATUS.md),
[audit](../../../Model%20Plane/docs/core-research/plane-audit-2026-07-13.md), and
[safe rebuild decision](../../../Model%20Plane/docs/core-research/grpc-safe-rebuild-decision-2026-07-13.md).
