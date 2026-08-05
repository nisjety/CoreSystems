# Wave 7 — Agent fine-tuning prompt

Self-contained brief for a future Claude session (or human engineer) to land per-agent model fine-tuning end-to-end.

---

## Context (read before starting)

CoreSystem already has a fully wired Agent platform combining Chatbase-style UX with Intercom-style operations. The agent workspace at `/agents/{id}/{viewId}` is 5 tabs deep with everything **except fine-tuning** working against real backends. The gap doc tracks closure progress at `apps/Frontend Plane/verevon/docs/ui-ux-verevon-gap.md`.

**The relevant section to read first**: §16 ("Agent ⇆ Data Plane wiring") at the bottom of that doc. It contains the architecture sketch for fine-tuning + the explicit statement that this is "Wave 7" work.

### What is already in place (lean on these, do not rebuild)

| Capability | Where it lives | Why it matters for fine-tuning |
|---|---|---|
| Agent record with `tools`, `knowledgeSources`, model, systemPrompt | `apps/Application Plane/convex-core/convex/agents.ts` + `convex/schema.ts` | The fine-tuned model id needs to land in the agent's `model` field. The schema needs one new optional field — `finetuneJobId` — to track in-flight jobs. |
| Verevon → Model Plane JWT auth | `apps/Frontend Plane/verevon/src/lib/model-plane/auth-token.ts` | Reuse `getModelPlaneTokenFromSession(request)` so the new `/api/agents/{id}/finetune` proxy authenticates the same way the existing agent proxies do. |
| Gateway JWT validation | `apps/Model Plane/rust/services/model-gateway/src/auth.rs` | Already validates incoming bearers via JWKS from auth-core. New `/v1/finetune/*` routes go behind the existing `require_auth` middleware. |
| Azure OpenAI integration | `apps/Model Plane/rust/services/inference-core/src/provider/azure.rs` | The fine-tuning calls use the *same* Azure key + endpoint as inference. No new credentials needed. |
| Model registry surfaces | `apps/Model Plane/go/services/capability-core/internal/api/capabilities.go` + Convex `agents` table | When a fine-tune job completes and produces a deployment, register it in capability-core's `models` registry so the existing `useModels` hook (`verevon/src/components/chat/hooks/useModels.ts`) surfaces it in the agent's model picker. |
| Cron / scheduled-job pattern | `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` cron proxies + capability-core cron table | Fine-tuning jobs are long-running (minutes to hours). The job polling can either piggyback on the cron table (a "system" cron entry that polls Azure on a 60s interval) or live in a new `finetune_jobs` table. Either works; pick the lighter one. |
| Agent workspace UI shell | `apps/Frontend Plane/verevon/src/components/agents/AgentWorkspaceView.tsx` | The 5 tabs (playground / sources / tools / analytics / schedules) all live in this file. Add a 6th tab. |
| Per-tab hook pattern | `apps/Frontend Plane/verevon/src/components/agents/hooks/{useAgentStats,useAgentPlayground,useAgentTools,useAgentKnowledge,useAgentCron}.ts` | New `useAgentFinetune` follows the same shape. |

### What is NOT in place (the work)

- No upload endpoint for training datasets (the existing `/api/chat/upload` indexes docs for RAG, which is different from fine-tuning data).
- No gateway routes for `/v1/finetune/*`.
- No Azure OpenAI Files API integration on the gateway side.
- No job-state tracking table or polling worker.
- No UI tab for kicking off / monitoring jobs.
- No admin-role gate or per-org budget cap (fine-tuning is **expensive** — guard rails are not optional).

---

## What to build

### 1. UI — new "Fine-tune" tab in `AgentWorkspaceView`

Add a 6th tab beside the existing 5 (`playground`, `knowledge`, `actions`, `analytics`, `schedules`). The component should let an operator:

- See the agent's current model (`agent.model`) — read-only display.
- Upload a JSONL file containing training examples. Each line is either OpenAI chat-format `{"messages":[{"role":"user","content":"..."},{"role":"assistant","content":"..."}]}` or completion-format `{"prompt":"...","completion":"..."}`. Validate client-side that every line parses + carries the required keys. Show a clear "X examples ready, Y rejected" summary before the operator clicks submit.
- Pick a base model from a dropdown limited to fine-tunable Azure deployments (`gpt-4o-mini`, `gpt-4o` today — query capability-core via `/api/models?finetunable=true`).
- (Optional) Override hyperparameters: epochs (default 3), learning_rate_multiplier (default 1.0), batch_size (default auto).
- Click "Start fine-tune". The button is gated behind an admin role check — fail loud with "Admin role required" when the calling user doesn't have it.
- See a list of past jobs for this agent: `{id, baseModel, status, createdAt, completedAt, fineTunedModelId, error}`. Status states: `queued | running | succeeded | failed | cancelled`.
- For a `succeeded` job, a "Publish to agent" button that PATCHes the agent's `model` field to the new fine-tuned id (so subsequent invokes use it). Until the operator clicks publish, the agent keeps using its current base model — explicit promotion, no silent swap.
- For a `running` job, a cancel button that hits `DELETE /v1/finetune/jobs/{id}`.

Pattern to follow: `useAgentCron` + `SchedulesTab` in `AgentWorkspaceView.tsx`. Same shape — list + form + per-row actions.

### 2. Verevon API proxies

- `POST /api/agents/{id}/finetune` — multipart/form-data upload. Body: `file` (JSONL), `baseModel`, `hyperparameters?`. Forwards to gateway `POST /v1/finetune/jobs`. Auth via `getModelPlaneTokenFromSession`.
- `GET /api/agents/{id}/finetune` — lists past jobs for the agent. Forwards to gateway `GET /v1/finetune/jobs?agent_id={id}`.
- `DELETE /api/agents/{id}/finetune/{jobId}` — cancels. Forwards to gateway `DELETE /v1/finetune/jobs/{jobId}`.

Follow the pattern in `apps/Frontend Plane/verevon/src/app/api/cron/route.ts` + `cron/[id]/route.ts` — same auth flow, same passthrough shape.

### 3. Gateway routes — new file `model-gateway/src/finetune_routes.rs`

Routes (all behind `require_auth`):

- `POST /v1/finetune/jobs` — accepts multipart with the JSONL + JSON metadata. Logic:
  1. Validate the operator has org-admin scope (`claims.scopes.contains(&"admin".to_owned())` — auth-core's JWT issuer needs to include this for org owners; if not yet, add to the token mint side at `apps/Control Plane/auth-core/src/auth/model-plane-token.controller.ts`).
  2. Check per-org budget: read `finetune_org_budget_used` counter, compare against `FINETUNE_ORG_BUDGET_USD` env (default e.g. $50/month). Reject with 429 + clear message if exceeded.
  3. Upload the JSONL to Azure OpenAI Files API: `POST {AZURE_OPENAI_ENDPOINT}/openai/files?api-version={version}&purpose=fine-tune` with `multipart/form-data`. Capture the `file_id`.
  4. Kick off the job: `POST {AZURE_OPENAI_ENDPOINT}/openai/fine_tuning/jobs?api-version=2024-08-01-preview` with `{model, training_file, hyperparameters?, suffix}`. Capture `job_id` + initial status.
  5. Persist `{job_id, org_id, agent_id, base_model, azure_file_id, status: 'queued', created_at}` into a new `finetune_jobs` Postgres table (add migration: `apps/Model Plane/rust/services/session-core/migrations/000X_finetune_jobs.up.sql` OR `apps/Model Plane/go/services/capability-core/migrations/...` — capability-core is the better home since it already owns models + cron tables).
  6. Return `{job_id, status}` to verevon.

- `GET /v1/finetune/jobs?agent_id=...` — list jobs for the agent (Postgres SELECT scoped by `org_id` from claims).

- `GET /v1/finetune/jobs/{id}` — fetch single job (refresh from Azure if `status in ['queued','running']`).

- `DELETE /v1/finetune/jobs/{id}` — `POST {AZURE_OPENAI_ENDPOINT}/openai/fine_tuning/jobs/{job_id}/cancel?api-version=...` + Postgres UPDATE status = 'cancelled'.

Wire the routes in `model-gateway/src/http_routes.rs::build_router` next to the cron block. Register the new module in `lib.rs`.

### 4. Polling worker

Add a background tokio task that:
- Every 60s, scans `finetune_jobs WHERE status IN ('queued','running')`.
- For each, calls `GET {AZURE_OPENAI_ENDPOINT}/openai/fine_tuning/jobs/{job_id}?api-version=...`.
- Updates the row: `status`, `completed_at`, `fine_tuned_model` (Azure returns this on success — it's the **model name**, not the deployment).
- On `succeeded`: also creates the Azure **deployment** (a separate API call to `PUT {AZURE_OPENAI_ENDPOINT}/openai/deployments/{name}?api-version=...` with the fine-tuned model name + a deployment id like `{agent_id}-ft-{shortJobId}`). Then INSERTs into capability-core's `models` table so the model picker surfaces it.
- Emit a NATS event `mp.v1.finetune.{job_id}.event` so verevon's chat / agent UI can react (Convex subscriber already pulls from model-plane-nats per W4-2).

Pattern: there's an existing polling-loop precedent in `apps/Model Plane/rust/services/session-core/src/compaction.rs` (the 60s tick loop). Copy that structure.

### 5. Admin role plumbing

The gateway's `Claims` struct (`auth.rs`) already has `scopes: Vec<String>`. Auth-core's `issueModelPlaneToken` (in `apps/Control Plane/auth-core/src/auth/convex-token.service.ts`) currently doesn't populate scopes. Add: when the calling user's role on the active org is `owner` or `admin` (read from user-core's `session-context`), include `scopes: ['admin']` in the JWT payload. The gateway then enforces via `claims.has_scope("admin")`.

This unblocks any future admin-only surface, not just fine-tuning.

### 6. Cost guard

Add to `model-gateway/deploy/docker-compose.override.yml`:

```yaml
FINETUNE_ENABLED: "1"               # master kill switch
FINETUNE_ORG_BUDGET_USD: "50"       # per-org monthly cap
FINETUNE_PER_JOB_BUDGET_USD: "20"   # single-job cap
```

When `FINETUNE_ENABLED` is unset / "0", the gateway routes return 503 "fine-tuning disabled in this environment". Dev compose can keep it off until staging-side verification.

### 7. Smoke test

Before declaring done:

1. Upload a 10-line JSONL (use a trivial Norwegian Q&A pair set) via the new UI.
2. Verify the job appears in the list with status `queued`.
3. Wait 5+ minutes — status should transition through `running` → `succeeded` (or `failed` with a clear error from Azure).
4. Verify a new Azure deployment exists by curling `GET {AZURE_OPENAI_ENDPOINT}/openai/deployments?api-version=...`.
5. Verify the new model surfaces in the agent's model picker.
6. Click "Publish to agent" — verify the agent's `model` field updates in Convex.
7. Send a chat to the agent — verify the response comes from the fine-tuned deployment (gateway log line `model_used=` should show the fine-tuned id).

### 8. Documentation

Close the open Wave 7 item in `apps/Frontend Plane/verevon/docs/ui-ux-verevon-gap.md`:

- Add a §17 section titled "Agent fine-tuning — closed".
- Update the tally at the bottom of §16 to remove "1 explicit open".
- Include the same shape as §15 + §16 closure entries: triage table, files modified, verified-live evidence, architecture diagram.

---

## Constraints / non-goals

- **Don't** fine-tune Anthropic, OpenAI direct, or Google — Azure is the default deployment and the Azure fine-tuning API is the only one we can authenticate against today. Mention this clearly in the UI ("Fine-tuning is currently available for Azure models only").
- **Don't** auto-promote fine-tuned models. Explicit "Publish to agent" click is required — running fine-tuning experiments without affecting production traffic is the whole point.
- **Don't** delete the base model from the agent's record on publish. Keep `agent.baseModel` (the original) + `agent.model` (currently active, may be fine-tuned) so a revert is one PATCH away.
- **Don't** ship without the budget gate. Fine-tuning a single gpt-4o run can cost ~$25 in tokens; an unguarded button is a real money risk.
- **Don't** skip the admin-role check. Per-agent fine-tuning is org-wide configuration, not a per-user knob.

## Estimated scope

- UI tab: 4-6 hours.
- Verevon proxies: 2 hours.
- Gateway routes + Files upload: 4-6 hours.
- Polling worker: 3-4 hours.
- Admin-scope plumbing through auth-core: 2 hours.
- Cost guards + tests + docs: 3-4 hours.
- **Total**: 2-3 focused days.

## When you're done

Update this prompt file: rename to `wave7-fine-tuning.closed.md`, add a "Result" section at the top citing the §17 doc entry, and commit. The next Wave will need its own prompt next to this one.
