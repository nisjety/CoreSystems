# CoreSystem development authority handoff matrix

Date: 2026-08-16
Status: non-secret deployment handoff; no credential values belong here

This matrix is the operator handoff for the four current release-critical
lanes. It records names, ownership, scopes, and presence checks only. Populate
the existing dev deployment from its approved secret store; do not generate or
rotate credentials as part of this document. A row is **ready** only when the
named inputs are present in the running container and the corresponding live
proof has produced a receipt.

| Lane | Service / owner | Non-secret references that must be present | Auth Core scope / principal contract | Current state |
|---|---|---|---|---|
| R-2 health | Capability Core / Model | `AUTH_CORE_ISSUER`, `AUTH_CORE_JWKS_URL`, `CAPABILITY_CORE_AUTH_AUDIENCE`, `CONTROL_USER_CORE_URL`, `CONTROL_SPACE_DECISION_KEY_ID`, `CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64` | Generic global health is only `service:execution-core`; explicit allowlist is `cap.command.sandbox` and `cap.command.shell`. Tenant health is never a global-row authority. | Source and disposable-Postgres proof passed; dev reporter observed; stale/outage candidate proof open. |
| R-2 owner-action health | Conversation Core / Application + Capability Core | `CAPABILITY_CORE_HTTP_URL`, `CONVERSATION_CAPABILITY_HEALTH_AUTH_CORE_URL`, `CONVERSATION_CAPABILITY_HEALTH_SERVICE_ID`, `CONVERSATION_CAPABILITY_HEALTH_SERVICE_API_KEY`, `CONTROL_RUN_ACTION_DECISION_KEY_ID`, `CONTROL_RUN_ACTION_DECISION_PUBLIC_KEY_BASE64`, `CONTROL_RUN_ACTION_AUTHORITY_URL`, `CONVERSATION_CONTROL_RUN_ACTION_AUTHORITY_TOKEN`, `CONTROL_OWNER_EFFECT_RESERVATION_URL`, `CONVERSATION_CONTROL_OWNER_EFFECT_RESERVATION_TOKEN` | Auth Core mints only `capability:read` + `capability:owner-action:health:write` for `aud=capability-core`; identity is exactly `conversation-core`; payload is content-free and restricted to `cap.tool.ticket.create`; startup is fail-closed until the Control verifier/current-authority/reservation/Execution delegation lane is complete. | Source-wired reporter and full Conversation Core tests passed; deployed observation, authenticated transport, stale/outage proof, provider receipt, and candidate evidence remain open. |
| Scheduled preparation | Capability Core / Control + Session | `CONTROL_USER_CORE_URL`, `CAPABILITY_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN`, `CONTROL_SPACE_DECISION_KEY_ID`, `CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64` | `session:schedule-prepare`; service identity is Capability Core; decision action is `model.schedule.run`. | Source proof passed; running dev references absent. |
| Scheduled run/step | Orchestrator + Execution Core / Control + Session | `ORCHESTRATOR_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN`, `ORCHESTRATOR_CORE_CONTROL_SCHEDULE_STEP_SERVICE_TOKEN`, `EXECUTION_CORE_SCHEDULED_STEP_DECISION_KEY_ID`, `EXECUTION_CORE_SCHEDULED_STEP_DECISION_PUBLIC_KEY_BASE64`, `CONTROL_USER_CORE_URL` | `session:runs:system-owner`, `session:scheduled-step`, `session:scheduled-step-authority`, `model:schedule:step`; Control uses the separate authority scope to resolve the exact prepared run/fire/template before signing; step caller is exactly `service:orchestrator-core`; no human bearer. | Source/disposable receipt proof passed; live Temporal/provider proof open. |
| Governed `tickets.create` | Execution Core + Conversation Core / Control + Application | `CONTROL_PLANE_USER_CORE_URL`, `EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN`, `EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN`, `CONVERSATION_CORE_AGENT_ACTION_URL`, `CONVERSATION_EXECUTION_CORE_SERVICE_TOKEN`, `EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK` (default `false`), `CONTROL_RUN_ACTION_DECISION_KEY_ID`, `CONTROL_RUN_ACTION_DECISION_PUBLIC_KEY_BASE64`, `CONTROL_RUN_ACTION_AUTHORITY_URL`, `CONVERSATION_CONTROL_RUN_ACTION_AUTHORITY_TOKEN`, `CONTROL_OWNER_EFFECT_RESERVATION_URL`, `CONVERSATION_CONTROL_OWNER_EFFECT_RESERVATION_TOKEN` | Separate run-action decision, model-view, owner-reservation, and Conversation service identities. Exact schema/payload/idempotency/owner binding; HTTPS is required for service/private hosts; the loopback HTTP exception is explicit dev-only and never a production substitute. No browser or generic health authority. | Source/disposable owner proof passed; deployed signer, transport, provider receipt, and candidate proof open. |
| Application delivery | Notification Core / Application | `NOTIFICATION_DELIVERY_MODE`, provider endpoint/key references, callback verification reference, HA replay-store reference | Application owns delivery receipts and projections; at-least-once processing with idempotent projection and explicit `unknown`. | Topology healthy; outbox/provider/callback/HA evidence open. |
| Candidate / rollback | Infra + release owner | signed candidate manifest, distinct rollback manifest, image/config/migration digests, key IDs, runtime evidence references | Promotion authority and rollback reviewer are operator-owned; ledger transitions remain `source_only` until evidence is signed and reviewed. | Candidate and rollback artifacts absent. |

## Presence-only verification

Run these commands without printing environment values:

```bash
bash apps/Model\ Plane/scripts/tests/dev-runtime-preflight.sh
bash scripts/coresystem-cross-plane-preflight.sh
bash apps/Model\ Plane/scripts/coresystem-qm-proof.sh
bash scripts/coresystem-conformance.sh --json
```

The first two commands inspect container health and key presence. The proof
runner uses ephemeral test dependencies and source contracts. The conformance
report is the release decision surface; its expected current result is
`status=blocked`, with `candidate=absent`, `rollback=absent`, and every Model
capability still `source_only` until an authorized operator supplies the
references and records live evidence.

## Handoff rules

- Never put bearer values, private keys, API keys, or raw runtime `.env` files in
  this matrix, a Temporal input, a checkpoint, a browser payload, or a log.
- Existing dev references may be reused for this internal pass, but production
  rotation and key-overlap/rollback require a separate approved handoff.
- A healthy Docker topology is not authority readiness. Every row needs a
  matching signed decision, owner receipt, and failure/reconciliation result.
- Missing or malformed inputs keep the affected lane unavailable; they do not
  fall back to a user credential or generic capability health.
