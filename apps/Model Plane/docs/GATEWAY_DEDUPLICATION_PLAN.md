# Gateway de-duplication cleanup plan

Goal (user's words): make the gateway act as a real gateway/BFF, not a second backend for things
the planes already own.

Scope: 56 findings from the 9-domain audit of model-gateway + the verevonv3 BFF against
apps/master-ownership-matrix.md and apps/Model Plane/docs/capability-ownership-matrix.md.
5 ZDR/residency findings (SRS-1, SRS-3, BI-1, BI-2, INF-2) were split into their own workstream.

CAVEAT ON PROVENANCE: these are audit-agent findings. 61 of 64 observations were flagged, and that
ratio suggests some over-flagging. Three were independently verified by hand (PA-1, SRS-4, CR-01).
Verify each against current source before acting on it; expect some to not reproduce.

## Outcome so far (2026-08-11)

~18 of 56 findings examined. **12 did not survive verification** — they were
overstated, already fixed, or their prescribed fix was wrong. Two of those
prescribed fixes would have introduced NEW defects (CR-02 cross-tenant, RAG-2
demotion-becomes-blocking). Read the DEMOTED section before acting on anything
here.

What actually got fixed, and what it was worth:

| Finding | Was | Now |
|---|---|---|
| CBU-1 | Fine-tune monthly cap never bound — every write of `actual_cost_usd` is a hardcoded 0.0, so a job counted $0 the instant it succeeded and an org could run at-the-cap jobs back to back forever | Real cost when recorded, submit-time estimate until then; `cancelled` alone counts 0 |
| INF-1 | gRPC `invoke`/`invoke_stream` consulted a gateway-local response cache keyed on the raw user message — a structured-output call could be served an earlier plain-text answer, and a hit skipped inference-core's ZDR handling and routing | Gateway cache deleted from both paths; inference-core's `PromptCache` (a strict key superset) does the job |
| §23.6 | Result handles were committed and INERT — nothing created a handle, and `result_query` was in no tool list | Both halves wired; oversized JSON read results are parked and queryable |
| CR-02 | MCP catalog hydration wrote whatever capability-core returned under the org it *asked* for, without checking the rows' tenant | Rows from a foreign tenant are refused (`global` still allowed) |
| CR-06 | Org spend caps were configurable and enforced nothing | Caps read from org-core and applied via cost-core |
| SRS-2 | BFF thread delete was durable, but its helpers were named `archive_*` while issuing irreversible DELETEs | Renamed `delete_durable_thread[s]` and documented |
| SRS-4 | Cross-tenant stream-resume leak | Buffer keyed by verified identity |
| PA-4 / CR-05, CR-07, CR-04, CR-08, CR-11 | Gateway kept parallel stores for permissions, plugins, hooks, commands, tasks | All proxy to their owner (execution-core / capability-core) |

Two things worth carrying forward:

1. **The governing rule** (founder's words): *if the system exists in the
   backend, the gateway's job is to PROXY the request.* Deleting a duplicate by
   making the RPC return `unimplemented` is just as broken as the duplicate.
2. **Check for callers, not docs.** §23.6 shipped complete, tested, committed
   and unreachable. A `cargo check` dead-code warning found it, not the audit.
   See TEST-1: the crate's test suite does not terminate, which is how both
   that and CBU-1 stayed invisible.

STATUS — progress so far (all verified against source before acting):

FIXED
- INF-1  GATEWAY RESPONSE CACHE DELETED FROM BOTH gRPC PATHS (proxy, don't
         reimplement). Reproduced and worse than written up: in `invoke`, the
         two lines immediately BEFORE the lookup build the memory-loaded
         `messages` and an `infer_req` carrying `structured_output_schema`,
         `temperature` and `max_tokens` — then all of it is discarded and the
         cache is consulted with the raw last user message under scope
         `{org_id, user_id, model}`. `TurnCacheability` appears 0 times in
         grpc.rs, so there was no eligibility gate either.
         Consequence: a structured-output call whose last user message matched
         an earlier plain-text call from the same (org, user, model) was served
         that plain-text answer with `stop_reason: "end_turn"` and zero tokens
         — the caller parses it as JSON and fails, with nothing to explain why.
         Same for differing temperature/max_tokens, and for memory drift (the
         context is in the prompt, not the key). A hit also short-circuited
         inference-core's ZDR handling, routing, and its own cache.
         Removed all four sites: unary lookup + store, streaming lookup +
         store, and the now-orphaned `try_serve_from_cache` (60 lines).
         Verified the replacement is a strict superset before deleting:
         `PromptCache::cache_key` (inference-core/src/cache.rs:67-91) hashes
         org, user, provider_hint, model, EVERY message role+content,
         temperature, max_tokens, tools, tool_choice and the structured-output
         schema, and `get` refuses ZDR outright; live on the path at
         provider/fallback.rs:701 (get) and :741 (put).
         The SSE path's cache is deliberately KEPT — it keys on the rendered
         prompt and gates on `TurnCacheability`, which is the shape the fix
         direction asks for.
- CBU-1  (partial) THE FINE-TUNE MONTHLY CAP NOW ACTUALLY BINDS. Verified the
         whole chain: session-core's `get_org_monthly_spend` summed
         `CASE WHEN status IN ('queued','running') THEN estimated_cost_usd
         ELSE actual_cost_usd END` (finetune_grpc.rs:404) on the stated
         grounds that "the polling worker has written the real number" — it
         never does. EVERY write of `actual_cost_usd` in the tree is a
         hardcoded 0.0 (model-gateway finetune_poller.rs:255, :333, :614;
         finetune_routes.rs:764, :850, :1034, :1737), and
         finetune_routes.rs:347 even documents it as "authoritative … on
         completion". So a job contributed $0 the instant it succeeded and an
         org's month-to-date spend fell back to zero as its jobs finished:
         `FINETUNE_ORG_BUDGET_USD` admitted an unbounded number of
         at-the-cap jobs, one after another.
         Now: real cost when one exists, submit-time estimate until then, and
         `cancelled` alone contributes 0 — charging an org its full estimate
         for training it deliberately stopped would burn the cap on work
         nobody bought, while `failed` still counts (compute was generally
         consumed, and that is the safe direction for a guard).
         `cargo check -p session-core` clean. NOTE: runtime SQL string, so
         compile-verified only — Docker was unresponsive, so it has not been
         run against a live Postgres. Structurally identical to the previous
         query (same SUM/::float8, both columns NUMERIC(10,4) NOT NULL).
         STILL OPEN: the ownership half — fine-tune spend never reaches
         cost-core's ledger, so `GET /api/v1/cost/aggregate` under-reports the
         org's real spend and the inference-side guard in budget.rs keeps
         approving for an org that already spent its money on training.
         That needs Azure's billed cost, so it was not guessed at.
- SRS-2  VERIFIED RESOLVED end to end, plus a naming hazard fixed. The BFF's
         `delete_thread`/`clear_threads` no longer touch a local index — both
         proxy `Method::DELETE` to Model Gateway `/v1/threads[/{id}]`, which is
         owner-bound Session Core `DeleteThread`. But the helpers doing it were
         named `archive_durable_thread`/`_threads` while model-gateway ALSO has
         genuinely non-destructive `POST /v1/threads/archive` and
         `POST /v1/threads/{id}/archive` routes — so the call site read as
         "archive this" while permanently erasing the thread, its messages, run
         descendants, events/audit evidence, plans, tasks, approvals and
         continuation records. Renamed to `delete_durable_thread[s]` and
         documented at both definitions. `cargo check` clean.
- CR-02  (partial) HYDRATION IS NOW TENANT-CONFINED. `CatalogMcpServer` now
         carries `org_id` and `catalog_mcp_parts` refuses a row whose stated
         tenant is neither the tenant being hydrated nor `global`
         (runtime_registries.rs). 3 tests: foreign tenant refused, own +
         `global` accepted, absent field tolerated. PREREQUISITE for the rest
         of CR-02, not cosmetic — see the remainder section at the end.
- SRS-4  cross-tenant stream-resume leak. Buffer keyed by verified identity; a foreign
         caller derives a different key and takes the same not-found path.
- PA-4 /
  CR-05  tool permissions. The gateway's in-memory (org,tool)->verdict map is deleted;
         CheckPermission now proxies capability-core EvaluatePolicy (mirroring
         execution-core's own call so the two cannot disagree) and SetPermission writes
         through to capability-core scopes/grant|revoke.
- CR-07  plugins. Register/list/set-enabled proxy to capability-core /api/v1/plugins,
         its declared system-of-record. capability-core assigns the id and creates
         plugins DISABLED (safe rollout), so responses report what it stored rather
         than echoing the request.
- CR-04  hooks. Hooks are POLICY RULES (product decision), now forwarded as
         ExecuteStepRequest.hook_context so execution-core's existing hook engine
         evaluates them. Previously the gateway sent "" on every dispatch, so a
         registered deny enforced nothing. Hook gained `decision`/`reason`;
         `callback_url` deprecated — nothing ever POSTed to it.
- CR-08  slash commands. ListCommands/ExecuteCommand proxy to capability-core
         /api/v1/commands and /commands/exec. Disabled commands are filtered out
         (the wire `Command` has no enabled flag, so this is the only way not to
         advertise something switched off). Exec args convert to capability-core's
         flat string map; a nested/array argument is REFUSED rather than
         stringified, since a coerced argument would execute a command the caller
         never described.
- CR-11  tasks. CreateTask/ListTasks proxy to capability-core /api/v1/tasks — the
         same place this gateway's OWN HTTP routes already proxied to
         (`create_task_proxy`). Only the gRPC surface kept a separate in-memory
         store, so a gRPC-created task was invisible to the task UI, to
         capability-core and to Temporal, and gone on restart. An absent
         `created_at` maps to 0, never "now", so an undated task cannot read as
         just-created.
- CR-01  MCP catalog hydration. Authenticated MCP listing and chat tool
         advertisement now refresh the tenant projection from capability-core,
         restore durable owner/share metadata, and replace the local cache
         fail-closed when the catalog is unavailable. Credential material is
         never imported; OAuth is still resolved through the encrypted token
         endpoint. Staging must still prove restart and multi-replica behavior.
- PA-2 /
  INF-3  PII redaction. Both unary and SSE invoke paths now read the
         authenticated org's capability-core `/api/v1/safety` projection before
         content reaches a provider. An enabled input `pii_filter` is enforced
         regardless of request features; a client may request additional
         redaction only. Missing delegated authority, invalid policy data, and
         capability-core failure redact fail-closed. This remains source/test
         evidence until deployed credentials and a live policy toggle are
         observed.

DEMOTED — verified NOT a real finding, do not "fix"
- RAG-2  the relevance scorer is an ENSEMBLE on top of Quarry, not a second
         implementation of it, and the stated fix would change product
         behaviour. Both halves of the finding fail on inspection:
           * "overrides the plane's semantic judgement" — the module knows
             about the reranker and gives it the DOMINANT vote:
             `DEFAULT_PROVIDER_WEIGHT = 0.6`, documented as "the reranker read
             the query and the result together and is strictly better evidence
             than bag-of-words overlap, so it dominates … it does not get the
             whole vote because it is one LLM call that can be wrong, and
             because it only ever scores the head of the list" (relevance.rs
             :74-79). Every constant is calibrated against observed noise
             cases, not picked. This is deliberate ensembling.
           * "move DEFAULT_SOFT_HOSTS / IMPLIED_DOMAINS to Quarry's
             include_domains/exclude_domains" — those are HARD filters:
             `site:` restricts results TO the list, `-site:` removes them
             outright (quarry-runtime/src/serp.rs:61-66). The gateway's host
             policy is a SOFT preference that explicitly refuses to block —
             `MAX_SOFT_HOST_PENALTY = 0.9` with "a penalty of 1.0 would be a
             hard block, which this module refuses to have", and a demoted
             host with full term coverage still scores 0.65 and is kept. The
             two mechanisms are not interchangeable, so the "de-duplication"
             would silently convert demotion into blocking.
         Residual (true but minor, and inherent): soft-demoted hits are paid
         for in provider quota before being demoted. You cannot soft-demote
         what you never fetched, so this is the cost of the chosen semantics,
         not a defect. `WebSearchRequest` (gateway.proto:494-505) indeed has no
         domain fields — correctly so, given the above.
- CR-09  operator-authored skills DO reach the model. The finding assumed
         `/v1/skills` wrote capability-core's `skill_packages` table while the
         model read session-core's `agent_skills`. It does not. Traced end to
         end:
           * capability-core `/api/v1/skills` INSERTs into `agent_skills`
             (registry_apis.go:145), NOT `skill_packages`.
           * capability-core and session-core share one database — both are
             `postgres:5432/session_core` in deploy/docker-compose.yml
             (:464, :923). Same table, not two stores.
           * session-core `list_agent_skills` selects
             `WHERE org_id = $1 AND (NOT $2 OR enabled)` (grpc.rs:2547) — no
             origin filter, so an operator row is returned like any other.
           * the omitted `origin` column defaults to 'user'
             (0006_agent_skills_origin.sql:12), satisfying the CHECK, so the
             write succeeds and `replace_learned` picks it up.
         `skill_packages` is real but is a separate versioned capability-bundle
         registry keyed by `capability_id` — not a chat-context source. Nothing
         to fix here.
- SRS-5  cross-tenant run cancel is ALREADY FIXED. `invoke_cancel`
         (http_routes.rs) now takes `Extension<Claims>` and calls
         `CancelRegistry::cancel_for(request_id, org_id, user_id)`, which
         compares the owner recorded at registration and returns false on a
         mismatch — so a foreign caller gets the same 404 as an unknown stream
         (no existence oracle). The old unscoped `cancel` is `#[cfg(test)]` only.
- PA-3   HARNESS_POSTURE_FLOOR is read (sse.rs posture_floor) and consumed
         (sse.rs:409 resolve_posture). The floor works.
- CR-10  skills cache staleness is ALREADY FIXED. `SkillStore::is_org_loaded`
         (skills.rs) expires on a 60s TTL (`SKILL_CACHE_TTL_SECONDS`), and its own
         comment names the exact bug the audit reported: "Was permanent ... a skill
         edited or deleted through capability-core stayed invisible to a running
         gateway until someone restarted it". The registry already treats
         session-core as owner and re-pulls via `list_agent_skills`.

- CR-06  OrgPolicy DECOMPOSED PER OWNER (decision: keep rate limit local).
           max_cost_per_run_usd / max_tokens_per_run -> Control Plane. org-core
             grew GET /organizations/:id/quotas and PUT .../quotas/:key over the
             existing `org_quotas` table (its `Quota` type was previously dead
             code). SetPolicy writes through; GetPolicy reads back from the
             owner so what an operator sees is what is enforced.
           ENFORCEMENT: model-gateway reads the ceilings on the invoke path and
             hands them to cost-core's /api/v1/budget/check, which already took
             the cap as a request parameter by design (it accounts spend; the
             caller supplies policy). A request may TIGHTEN its own run, but a
             dimension it leaves unset falls back to the org ceiling, so
             omission cannot be used to escape an operator's limit.
           denied_tools -> capability-core scopes/revoke.
           rate_limit_rpm -> stays local; RateLimiter gained per-org overrides
             (it had one global RPM, so a per-org ceiling was unsettable).
             Setting one drops the stale bucket so a lowered limit bites now.
           allowed_models -> STILL homeless; refused by name.
         USD is stored in micro-dollars because quota_limit is BIGINT, under a
         key that names the unit. Quota reads FAIL OPEN on an org-core outage
         (logged): rejecting every chat turn during an unrelated Control Plane
         blip is worse, and cost-core still accounts actual spend.


GUIDING RULE (from the user): if the backend owns it, the gateway PROXIES to it.
Not refuse, not reimplement, not keep a parallel store. An earlier pass made these
RPCs return `unimplemented` and that was wrong — it left the caller with a broken
feature instead of a working one.


## Group B — Accepted-then-ignored (3)

_The gateway accepts a write (a policy, a hook, a skill) and it reaches nothing. This is worse than the feature being absent — the operator is told it worked. Fix: persist to the owning plane, or fail honestly._

### PA-3 [high] — permission-approval

**Status (2026-08-11): resolved in source; agentic staging observation remains open.**

The resolved posture is now forwarded as RunAgentRequest.mode instead of
hardcoding ask, so STREAM_OPENED and execution-core share one enforced value.

The detailed gateway/consequence/fix text immediately below is the historical
pre-fix finding; use the status and resolution above for the current behavior.

**Gateway:** model-gateway contains a complete local approval-posture policy engine whose output reaches no enforcement point. `apps/Model Plane/rust/services/model-gateway/src/profile.rs` (208 lines, 8 tests) maps profile→posture (`:56-61`), derives a server floor from JWT scopes (`:95-108` `floor_from_scopes`) and clamps the client request stricter (`:119-121` `resolve_posture`). `sse.rs:4362-4382` `posture_floor` reads two env knobs, `HARNESS_POSTURE_FLOOR` and `HARNESS_AUTONOMOUS_SCOPES`. `sse.rs:407-409` computes the posture — and `sse.rs:416-419` inserts it into the STREAM_OPENED envelope payload and nothing else (grep for `posture` in `sse.rs` yields only these lines plus the helper). The actual dispatch hardcodes the mode: `sse.rs:4450-4470` builds the single `RunAgentRequest` in the whole crate with `mode: "ask".to_owned()` (`:4461`), and the two direct `ExecuteStep` call sites hardcode `permission_mode: "auto"` (`tools.rs:155`, `browser_run.rs:419`). `RunAgentRequest` has no `permission_mode` field at all (`apps/Model Plane/proto/model_plane/v1/execution.proto:38-80`).

**Already exists:** execution-core already derives the posture from the request and already fails safe: `apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:995-1001` `resolve_mode` — only explicit `"auto"`/`"deny"` are honoured, everything else (including `""`) becomes `PermissionMode::Ask`. Per-capability risk lives in capability-core's engine (`go/services/capability-core/internal/policy/engine.go:150-160`, RiskHigh → `DecisionAsk`).

**Consequence:** Two concrete failures. (1) `HARNESS_POSTURE_FLOOR=ask`, documented at `sse.rs:4360-4361` as the way to "fail safe fleet-wide", is inert — no code path consumes the value, so an operator who sets it believes they have tightened the fleet and has changed nothing (the inline chat loop stays ungated per PA-1, the agentic loop was already `"ask"`). (2) The STREAM_OPENED event publishes `permission_mode` (`sse.rs:418`) that the run never ran under, so any operator dashboard, NATS consumer, or audit query reading that field records a posture that was not enforced — a false compliance record.

**Fix direction:** Either delete `profile.rs`/`posture_floor` and let execution-core's `resolve_mode` be the single implementation, or — better — add `permission_mode` to `RunAgentRequest` and send the resolved posture, so the value the gateway publishes is the value that is enforced. Do not leave a configurable security knob that no code reads.

### PA-4 [high] — permission-approval

**Gateway:** model-gateway ships a second tool-ACL and org-policy system. `apps/Model Plane/rust/services/model-gateway/src/runtime_registries.rs:1648-1714` — `PermissionRegistry`, an in-memory `DashMap<(org, tool_name), PermEntry{verdict, reason}>` whose miss behaviour is **default-open** (`:1656` "Missing = allow (default-open)"; `:1683-1687` returns `allowed: true, reason: "default allow"`). `:1716-1748` — `PolicyStore`, an in-memory `DashMap<org, OrgPolicy>` returning `OrgPolicy::default()` on miss. Both are held in `state.rs:252-253` / `:342-343` and exposed as gRPC (`grpc.rs:1678-1693` `CheckPermission`/`SetPermission`, `grpc.rs:1638` "Wave 10i — commands / hooks / permissions / policy"). Neither is hydrated from capability-core: the only reconcile consumer, `capability_consumer.rs:37,53-58`, actions `capability.mcp_server.removed` and nothing else. Neither is consulted by the gateway's own dispatch — `handle_check_permission` has no caller outside its own tests (`runtime_registries.rs:2132`, `:2158`).

**Already exists:** capability-core is the policy system of record: `apps/Model Plane/go/services/capability-core/internal/policy/engine.go:117-192` `Evaluate`/`EvaluateCapability` → allow/ask/deny driven by `RiskLevel`, plus a **fail-closed** scope check (`:196-215` `denyOnScope`) and a durable tenant-bound grant check against the `capability_scopes` table (`:237-265` `denyOnGrant`, "An explicit tenant-bound grant is mandatory"). RBAC enforcement is `engine.go:79-97`. It is reached over gRPC by execution-core at `rust/services/execution-core/src/capability_policy.rs:97-128`.

**Consequence:** A `SetPermission(org, tool, "deny")` recorded through the gateway looks accepted, survives no restart, is invisible to the next gateway replica, and is never seen by execution-core — the only place enforcement actually runs. In the other direction, any caller of the gateway's `CheckPermission` (an admin UI, an operator script) is told `allowed: true / "default allow"` for a capability that capability-core would deny for want of a scope grant. Whichever surface an operator trusts, it is the wrong one.

**Fix direction:** Make `handle_check_permission` a read-through to `CapabilityCore.EvaluatePolicy` (the client already exists in execution-core and can be mirrored, or proxy via the existing `capability_core_base_url` HTTP path used for MCP), and make `SetPermission` a write-through to capability-core's durable policy surface. If neither is wanted, remove both RPCs rather than ship a default-open shadow verdict.

### CR-09 [high] — capability-registry

**Gateway:** The registry users write skills into is not the registry the model reads from. Write path: the gateway's `/v1/skills` CRUD proxies to capability-core (http_routes.rs:4831-4873), landing in `skill_packages`. Read path: `SkillStore` (skills.rs:50-59) is populated exclusively from session-core `agent_skills` via `ListAgentSkills` (grpc.rs:1492-1514; sse.rs:2394-2425) — there is no disk preload wired in `state.rs` and no `RegisterSkill` RPC — and `handle_match_skills` scores only `store.list(&req.org_id)` (skills.rs:250-253). capability-core's `skill_packages` is never read into the match cache. Additionally, the mapping `agent_skill_to_skill` (skills.rs:185-194) copies only id/name/content/trigger_keywords and drops `tool_restrictions` and `description`.

**Already exists:** Two near-identical durable schemas: capability-core `skill_packages` (migrations/0003_capabilities_registry.up.sql:137 — name, description, content, trigger_keywords, trigger_file_patterns, tool_restrictions, enabled, eval_score, pinned_version) and session-core `agent_skills` (migrations/0004…:191 — name, description, content, trigger_keywords, trigger_file_patterns, tool_restrictions, enabled). `tool_restrictions` is a first-class field of the durable contract (`proto/model_plane/v1/sessions.proto:526`) but has no counterpart in the gateway's `Skill` message (gateway.proto:979-991). capability-ownership-matrix.md:144 already flags this as "a deliberate 3-way … needs its own consolidation analysis"; :59 names capability-core the skills registry owner.

**Consequence:** (a) A skill an operator authors through the product (`POST /v1/skills` → capability-core `skill_packages`) is stored, listable and editable — and never injected into a single chat, because `MatchSkills` only ever sees session-core `agent_skills`. The only skills that actually steer the model are the LLM-generated `background_review` ones the G7 loop writes. Authoring a skill appears to work and has no effect. (b) `tool_restrictions` — the skill author's own guardrail, e.g. "use read-only tools only" — is silently discarded at injection time, so the skill body is prepended to the prompt with its restriction stripped. Nothing downstream can re-derive it, because the gateway `Skill` type has no field to carry it.

**Fix direction:** Decide one owner for skill bodies (the matrix leans capability-core) and make the gateway's match cache load from it, so the write and read paths meet; until then, at minimum stop the silent data loss — carry `tool_restrictions` on the gateway `Skill` and either enforce it when the skill is injected or refuse to inject a skill whose restrictions cannot be honored.

## Group C — Ephemeral local copy of durable plane state (10)

_The gateway holds in memory what a plane is system-of-record for, and loses or diverges from it. Fix: rehydrate from the plane; treat local as a cache with a real miss path._

### AZI-1 [high] — authz-identity

**Status (2026-08-11): resolved in source for the capability-core catalog path.**
Authenticated hydration restores durable ownership/share projections, and the
share mutation now patches capability-core with rollback on rejection. The
Control-Plane `resource_grants` consolidation remains a separate open decision.

**Gateway:** model-gateway keeps a complete per-resource authorization/sharing ACL in process memory: `apps/Model Plane/rust/services/model-gateway/src/ownership.rs:29-125` defines Scope(Org|User), owner_user_id, shared_with, and the decision functions `usable_by`/`visible_to`/`can_modify`; `ownership.rs:134-240` backs them with a plain `DashMap`. The share mutation `mcp_share` at `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:2563-2586` calls only `state.ownership.set_shares(...)` and returns — it takes no `VerifiedCapabilityBearer`, makes no HTTP call, and publishes no event. There is no read path back: the only capability-core calls in the whole crate are `POST /api/v1/mcp` (http_routes.rs:2424, 2926) and `DELETE /api/v1/mcp/{id}` (http_routes.rs:2528) — there is no GET, and `capability_consumer.rs` is removal-coherence only (its own doc: `registered`/`updated` are intentionally ignored).

**Already exists:** capability-core is the declared system of record for exactly this data, and the gateway's own code says so: `apps/Model Plane/rust/services/model-gateway/src/runtime_registries.rs:839-846` — "Ownership (scope/owner/shares) rides in config_json so the durable catalog record stays the system-of-record for who-can-see-what, not just the ephemeral gateway sidecar" — and it serialises `owner_user_id` + `shared_with` + `scope` into the capability-core payload. The durable table is `apps/Model Plane/go/services/capability-core/migrations/0003_capabilities_registry.up.sql:188-206` (`mcp_servers`, with `scope`, `created_by`, `config_json JSONB`). The ownership ruling is `apps/Model Plane/docs/capability-ownership-matrix.md:59` and the cache rule is `:42`.

**Consequence:** capability-core's `config_json.shared_with` is permanently `[]` no matter how many shares are granted, so the record the gateway itself designates as "system-of-record for who-can-see-what" is factually wrong for every shared server. Concretely: Alice registers a private MCP server and shares it with Bob; the gateway allows Bob's agent to call its tools (`runtime_registries.rs:1334-1338` gates tool exposure on `ownership.usable`), but capability-core — the plane any auditor, admin UI, or second consumer would query — says the server has no grantees. On any gateway restart or redeploy the grant is gone and Bob is silently denied, while capability-core's row (which the registry cannot rehydrate at all) still exists. If model-gateway is ever run with more than one replica, replica B denies exactly what replica A allows for the same user and token — nondeterministic authorization with no audit trail of the grant anywhere durable.

**Fix direction:** Make `mcp_share` a write-through like `register_mcp_server`: take `VerifiedCapabilityBearer`, re-`POST /api/v1/mcp` (or add a dedicated shares endpoint) with the updated `config_json.owner_user_id`/`shared_with`, and roll the in-memory `set_shares` back if the catalog write fails — the exact pattern at http_routes.rs:2432-2448. Then add the missing read half so `OwnershipStore` hydrates from capability-core on miss (mirroring `approvals.rs::read_through_approval_for_owner_authenticated`, http_routes.rs / approvals.rs:884-929), which also fixes the restart hole for the registry itself.

### CR-01 [high] — capability-registry

**Status (2026-08-11): resolved in source; staging restart/revocation proof remains open.**
Authenticated catalog hydration now replaces the org cache and ownership
projection before MCP tools are advertised. The historical finding below
describes the pre-fix state.

**Gateway:** The MCP registry is documented as a cache (runtime_registries.rs:4-5 "gateway-scoped and ephemeral"; :822-823 "the gateway's in-memory store is a cache") but has no hydration path in either direction of the read. `state.mcp` is constructed empty (state.rs:337, `McpRegistry::new()`), and the only writers are the two registration handlers (http_routes.rs:2375 create, http_routes.rs:2901 OAuth callback). Every read goes straight to the local DashMap and nowhere else: `full_mcp_tool_defs` iterates `reg.inner` (runtime_registries.rs:553-562), `handle_list_mcp_servers` iterates `reg.inner` (:1303-1308), `handle_proxy_mcp_tool` looks up `reg.inner` (:1346-1350). main.rs:44 spawns only the reconcile consumer, which is removal-only by design (capability_consumer.rs:11-20, 60-72). No code path anywhere issues `GET /api/v1/mcp` to repopulate.

**Already exists:** capability-core owns the durable `mcp_servers` table — apps/Model Plane/go/services/capability-core/migrations/0003_capabilities_registry.up.sql:188 (with `enabled`, `rollout_state`, `risk_level`, unique on (org_id,name)) — served by `GET/POST /api/v1/mcp` at apps/Model Plane/go/services/capability-core/internal/api/registry_apis.go:812-813. The gateway already writes to it (runtime_registries.rs:833-876, http_routes.rs:2424-2453).

**Consequence:** Every gateway restart, redeploy, or crash silently empties every org's MCP server set. capability-core still holds `enabled=true` rows and a valid AES-GCM-encrypted OAuth token (`mcp_oauth_tokens`, migration 0003:220), so the admin/catalog view says the server is connected while chat and the agent loop see zero MCP tools — `full_mcp_tool_defs` returns an empty Vec and the model is simply never told the tool exists, with no error surfaced. Recovery requires the user to re-run the whole OAuth consent flow. In a multi-replica deployment the failure is worse than intermittent: only the replica that handled the registration has the server, so identical requests get different tool sets depending on which pod they land on.

**Fix direction:** Give `McpRegistry` a real read-through: on a miss for (org_id), fetch `GET /api/v1/mcp?org_id=…` with the existing `capability_bearer`/`capability_core_base_url` (both already in `AppState`) and populate `inner` + `ownership` from the row's `config_json` (which registration already writes at runtime_registries.rs:842-846), behind a short TTL like the existing `MCP_CATALOG_TTL`. The token is not needed to rebuild the entry — `resolve_stored_oauth_token` (mcp_oauth.rs:449) already fetches it per call from capability-core.

### CR-02 [high] — capability-registry

**Status (2026-08-11): partially resolved in source.** Per-request catalog
hydration closes the stale-cache window for authenticated chat/list requests;
the core-NATS consumer remains at-most-once for service-only paths, so a live
multi-replica revocation observation is still required.

**Gateway:** The reconcile consumer's own doc-comment justifies at-most-once core-NATS delivery on the grounds that "a missed removal is corrected by the next event or a cache TTL" (capability_consumer.rs:93-95). No such TTL exists. `McpRegistry` (runtime_registries.rs:138-147) holds `inner: Arc<DashMap<(String,String), McpServer>>` with no expiry and no sweeper; the only TTL in the struct is `MCP_CATALOG_TTL` (:57), which governs the discovered `tools/list` catalog, not the server entry. `McpRegistry::remove` (:179-185) is the sole eviction and is called only by the consumer and the explicit delete/rollback paths.

**Already exists:** capability-core is the revocation authority: `DELETE /api/v1/mcp/{id}` (internal/api/registry_apis.go:813) soft-deletes the `mcp_servers` row and publishes `mp.v1.capability.mcp_server.removed` via `internal/reconcile.Emit`.

**Consequence:** If the gateway is disconnected from NATS when a server is revoked (NATS_URL unset, bus restart, subscription gap — all silent, since `run` self-guards and returns), the gateway keeps advertising that server's tools to the model and keeps proxying `tools/call` to it, resolving the stored OAuth credential from capability-core on every call (runtime_registries.rs:1385-1391). Revocation of a compromised or off-boarded MCP integration does not take effect on a long-running gateway. There is no error, no drift alarm, and no expiry — only a restart clears it, which happens to work purely because of the CR-01 bug.

**Fix direction:** Either give `inner` the TTL the comment already assumes (an entry re-validated against `GET /api/v1/mcp/{id}` when stale — which is the same read-through CR-01 needs), or move the reconcile subscription to JetStream with a durable consumer so removals cannot be silently dropped.

### CR-03 [high] — capability-registry

**Status (2026-08-11): resolved in source for capability-core persistence;
cross-plane grant unification remains open.** Gateway sharing now uses the
durable MCP patch contract and rehydrates the owner/share projection before
making a decision. The historical finding below describes the pre-fix state.

**Gateway:** `OwnershipStore` (ownership.rs:133-136) is an in-memory `Arc<DashMap<(org,kind,resource_id), Ownership>>` implementing a private-until-shared grant model (scope org/user, `owner_user_id`, `shared_with`) for MCP servers, plugins, commands and hooks (ownership.rs:23-26). The module declares "The gateway is the authoritative gate" (ownership.rs:15). It is the gate for real tool exposure: `full_mcp_tool_defs` filters on `ownership.usable(...)` (runtime_registries.rs:559) and `handle_proxy_mcp_tool` re-checks it (:1333-1345). Registration writes `owner_user_id`/`shared_with` into capability-core's `config_json` exactly once (runtime_registries.rs:842-846), with the stated intent that "the durable catalog record stays the system-of-record for who-can-see-what" (:839-841). But the share mutation writes only to memory: `mcp_share` (http_routes.rs:2563) calls `state.ownership.set_shares(...)` (http_routes.rs:2573) and performs no catalog write-through — despite its own comment that "Sharing changes durable tool exposure policy and is never an ephemeral operation".

**Already exists:** Two durable owners already exist. (a) capability-core `mcp_servers.config_json` (migrations/0003_capabilities_registry.up.sql:195), which the gateway itself designates the system of record for visibility. (b) Control Plane user-core `resource_grants` — apps/Control Plane/user-core/migrations/012_resource_grants.up.sql:17-30 — generic `(org_id, resource_type, resource_id, subject_type, subject_id, role)`, described at :4-8 as "the ONE authority for explicit per-subject grants across every ownable resource type … subject_type + role are present from day one", with `internal/users/acl_repository.go` and the `authz_facade` already serving it.

**Consequence:** After the first share or unshare, capability-core's `config_json.shared_with` is permanently wrong — the record the gateway itself calls the system-of-record for who-can-see-what now disagrees with the gate that is actually enforced. Because `usable`/`visible`/`can_modify` all fail closed on a missing entry (ownership.rs:167-172, :175-187, :191-200), a second gateway replica — or the same one after restart — treats every MCP server as unusable and unmanageable: the owner cannot use their own server's tools, and `can_modify` fails closed "even for administrators", so nobody can delete or re-share it. Grants also cannot be revoked centrally: a Control-Plane off-boarding that clears a user's `resource_grants` has no effect on the gateway's copy.

**Fix direction:** Persist the grant where a grant already lives. Minimum: make `set_shares` write through to `PATCH /api/v1/mcp/{id}` updating `config_json.shared_with`, and hydrate ownership from `config_json` alongside the CR-01 read-through. Correct: express MCP/plugin sharing as `resource_grants` rows (`resource_type = 'mcp_server'`) through user-core's authz facade, and reduce `OwnershipStore` to a TTL'd read cache of that decision.

### CR-05 [high] — capability-registry

**Gateway:** `PermissionRegistry` (runtime_registries.rs:1654-1664) is a per-(org, tool_name) allow/deny map. `handle_check_permission` (:1671-1688) returns `allowed: true, reason: "default allow"` for any tool with no entry (:1683-1687) — default-OPEN. `handle_set_permission` (:1695-1714) stores a verdict. The only callers of either are the gateway's own gRPC handlers (grpc.rs:1683, :1692); a repo-wide search for `CheckPermission`/`SetPermission` outside generated proto code returns nothing in Rust or Go, so no dispatch path in any service consults this ACL.

**Already exists:** capability-core `internal/policy/engine.go` is the canonical policy engine — `DecisionAllow/DecisionDeny/DecisionAsk` (:22-25), risk-tiered and default-DENY for high-risk capabilities (package doc, :1-5), backed by the durable `capability_scopes` grant table via `ScopeResolver` (:33-41). execution-core actually calls it: `rust/services/execution-core/src/capability_policy.rs:11-34` holds a `CapabilityCoreClient` and issues `EvaluatePolicy` before dispatch, with "Unknown tools fail closed before dispatch" (:5). The gateway holds the same client (`state.capability_client`, state.rs:124) and uses it only for `ListCapabilities`/`GetCapability` read proxies (http_routes.rs:4558, :4586) — it never calls `EvaluatePolicy`.

**Consequence:** An operator calls `SetPermission(org, "book_shipment", "deny")` to kill a dangerous tool, gets a success response, and can read the deny back via `CheckPermission` — while the chat loop and execution-core both dispatch the tool anyway, because neither reads this map. The verdict is also lost on restart. Two operators auditing the same org get opposite answers depending on which surface they query: capability-core says deny-by-risk, the gateway says "default allow". A permission UI built on this API would display a guarantee the system does not provide.

**Fix direction:** Delete the local ACL and make `CheckPermission` a thin relay to `EvaluatePolicy` on the `capability_client` the gateway already holds (the same call execution-core makes), caching the decision briefly. `SetPermission` should relay to capability-core's scope grant/revoke endpoints (`/api/v1/capabilities/scopes/grant|revoke`, internal/api/capabilities.go:45-46) rather than writing a private map.

### AZI-2 [medium] — authz-identity

**Gateway:** The plain MCP registration path fires the capability-core write-through and inspects only the HTTP status: `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:2421-2454` — `let catalog_ok = catalog_result.as_ref().is_ok_and(|response| response.status().is_success());` — then returns, never reading the response body. The gateway's ownership record was already keyed under its own locally-minted id at `http_routes.rs:2387-2394` (`state.ownership.set(&org_id, KIND_MCP, &server.server_id, ...)`).

**Already exists:** capability-core upserts by `(org_id, name)`, not by id — the unique index is `apps/Model Plane/go/services/capability-core/migrations/0003_capabilities_registry.up.sql:208-210` (`mcp_servers_org_name_uq ON mcp_servers (org_id, name) WHERE deleted_at IS NULL`) — so it is the authority on the durable id. The OAuth registration path in the very same file already handles this correctly: `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:2934-2991` parses `id` out of the catalog response, re-keys the local cache when it differs, and only then writes ownership (`state.ownership.set(..., &server_id, ownership)` at 2987).

**Consequence:** After any gateway restart (or any re-registration of a same-named server), the plain path proposes a fresh id, capability-core keeps the original, and the gateway's `OwnershipStore` and `McpRegistry` are keyed to an id that does not exist in the catalog. Every subsequent authorization-relevant call then targets the wrong durable row: `DELETE /api/v1/mcp/{gateway_id}` returns 404, which `http_routes.rs:2542-2544` explicitly tolerates, so the gateway reports the server removed and drops its ownership record while capability-core's row — including its `owner_user_id` — survives indefinitely. The org believes it revoked a tool server; the durable catalog still lists it with an owner.

**Fix direction:** Lift the id-reconciliation block from the OAuth path (http_routes.rs:2934-2991) into the plain path: parse `id` from the capability-core response, re-key the `McpRegistry` entry when it differs, and set `OwnershipStore` under the persisted id only. Better still, extract it into one shared helper so a third registration path cannot regress again.

### CR-06 [medium] — capability-registry

**Gateway:** `PolicyStore` (runtime_registries.rs:1716-1725) is an in-memory `org -> OrgPolicy` map. `OrgPolicy` (proto/model_plane/v1/gateway.proto:1283-1295) carries `max_cost_per_run_usd`, `max_tokens_per_run`, `allowed_models`, `denied_tools`, `rate_limit_rpm`. `handle_set_policy` (:1755-1770) stores it; `handle_get_policy` (:1732-1748) returns a zero-valued default on a miss. Only callers are grpc.rs:1701 and :1710. The gateway's real budget path (`budget.rs:1-4`, called at sse.rs:374 and http_routes.rs:6360) correctly asks cost-core — but it reads `max_cost_usd`/`max_tokens` from the *request* (`NormalizedRequest`), never from `PolicyStore`.

**Already exists:** Three different owners, one per field group. Cost budgets: cost-core (master-ownership-matrix.md:74, "Cost ledger / budgets … **cost-core-go**"), which `budget.rs` already dials correctly. Model allowlist/routing: capability-core `routing_policies` (migrations/0003_capabilities_registry.up.sql:265) served at `/api/v1/routing` (internal/api/registry_apis.go:1288). Tool deny + safety: capability-core `policy/engine.go` and `safety_policies` (migrations/0003:293, `/api/v1/safety` at registry_apis.go:1462).

**Consequence:** An operator sets an org-wide `max_cost_per_run_usd = 5` and `denied_tools = "book_shipment"`, gets the stored policy echoed back, and nothing enforces either: the budget check only fires when the *caller* supplies `max_cost_usd` on the request, so any client that omits the field bypasses the org cap entirely; the tool denylist is read by no dispatch path; `allowed_models` never reaches inference-core's router. The org cap is not merely unenforced, it is trivially bypassable by omission — and it is lost on restart.

**Fix direction:** Retire `OrgPolicy`/`PolicyStore`. Route each field to its owner: org cost caps into the cost-core budget check (`budget::check_budget` should fold in the org policy fetched from cost-core, not just the request), model allowlist into capability-core `routing_policies` / inference-core's router-policy endpoint (the pattern the verevonv3 BFF already uses in `domains/router_policy.rs`), tool deny into capability-core's policy engine.

### CR-07 [medium] — capability-registry

**Gateway:** Two plugin catalogs in the same process, on two protocols. gRPC: `handle_register_plugin`/`handle_list_plugins`/`handle_set_plugin_enabled` (runtime_registries.rs:1442-1509) read and write `state.plugins`, an in-memory DashMap (:1425-1428), with no write-through and no hydration — registration defaults `status = "active"` (:1458-1460) and honors whatever `enabled` the caller supplies, with no risk, pinning or rollout gate. HTTP: the same gateway's `/v1/plugins` routes proxy straight to capability-core (http_routes.rs:4881-4923 via `proxy_to_capability_core`).

**Already exists:** capability-core `plugin_packages` (migrations/0003_capabilities_registry.up.sql:240) served at `/api/v1/plugins` (internal/api/plugins.go:41-42). The table encodes the safety posture the gateway path lacks: `risk_level TEXT NOT NULL DEFAULT 'high'`, `enabled BOOLEAN NOT NULL DEFAULT FALSE -- disabled until pinned+tested`, `pinned BOOLEAN DEFAULT FALSE`, `rollout_state DEFAULT 'canary'`. master-ownership-matrix.md:72 assigns the plugin registry to capability-core-go.

**Consequence:** A plugin registered and enabled over the gRPC surface never appears in the durable catalog an operator audits, and the pin/rollout/risk review capability-core enforces is simply absent on that path. Conversely a plugin disabled through the HTTP/admin path stays `enabled = true` in the gRPC registry. Any consumer that grows to read `ListPlugins` would be reading a catalog that no operator control panel can see or revoke, and that vanishes on restart.

**Fix direction:** Delete `PluginRegistry` and back the three gRPC handlers with the same `proxy_to_capability_core` calls the HTTP routes already use (`/api/v1/plugins`), so one durable catalog answers both protocols and the disabled-until-pinned default is preserved.

### CBU-3 [medium] — cost-billing-usage

**Gateway:** `apps/Model Plane/rust/services/model-gateway/src/runtime_registries.rs:1848-1919` implements `AnalyticsStore` — a per-org in-memory rollup of `total_cost_usd`, `total_input_tokens`, `total_output_tokens`, `total_invocations` and per-tool call counts (`OrgCounters`, `:1853-1861`) — and exposes it as a public gRPC contract via `handle_get_analytics` (`:1895-1919`), wired at `model-gateway/src/grpc.rs:1736-1743` and held in `state.rs:256`.

**Already exists:** cost-core does exactly this aggregation, durably and org/user-scoped: `GET /api/v1/cost/aggregate` (`go/services/cost-core/internal/server/server.go:113`, handler `:318-355`) returns `total_input_tokens` / `total_output_tokens` / `total_cost_usd` / `entry_count` over the Postgres `cost_entries` ledger, with a cross-user guard (`server.go:340-348`). `GET /api/v1/usage` (`:111`) does the per-user rollup. The BFF's real cost dashboard already reads the cost-core version (`verevonv3/apps/gateway/src/domains/cost.rs:123-132`).

**Consequence:** `model_plane.v1.ModelGateway/GetAnalytics` is a published, auth-gated gRPC method that answers every caller with `total_cost_usd: 0.0, total_input_tokens: 0, total_output_tokens: 0` for every org, forever. It is the only method on the gateway named "analytics", so it is precisely what a future usage/analytics dashboard would wire to — and that dashboard would ship reporting $0 spend while the cost panel next to it, reading cost-core, shows the real bill. Even if someone fixed the missing `record()` call, the counters are per-process and non-durable: they would reset to zero on every gateway restart or deploy and differ across replicas, so two browser tabs hitting different gateway pods would show different lifetime totals.

**Fix direction:** Delete `AnalyticsStore` and make `handle_get_analytics` a thin relay to cost-core `GET /api/v1/cost/aggregate` (plus `GET /api/v1/cost/entries` if the per-tool breakdown is wanted — that breakdown has no cost-core equivalent today and should be added there, not kept here). At minimum, fix the false doc comment so the next reader is not told this is populated.

### CR-11 [low] — capability-registry

**Gateway:** The same HTTP/gRPC split as CR-07, for tasks. `TaskStore` (runtime_registries.rs:1929-1938) plus `handle_create_task`/`handle_list_tasks` (:1945-1990) keep task records in an in-memory DashMap reachable over gRPC (grpc.rs:1815, :1824), while the same gateway's `/v1/tasks` and `/v1/cron` HTTP routes proxy to capability-core (http_routes.rs:4687-4771). The module header already concedes the problem: "durable state belongs in session-core `tasks` tables, cron in Temporal — the orphaned task-core service was retired, matrix §4.2" (runtime_registries.rs:17-19).

**Already exists:** capability-core `/api/v1/tasks` and `/api/v1/cron` (internal/api/workplane_apis.go:490-491, :704-705); durable task/cron state in session-core migration 0004 (`tasks`, `cron_schedules`, `cron_fires`); Temporal (orchestrator-core) as the durable scheduler. master-ownership-matrix.md:73 assigns tasks/cron to the orchestrator; capability-ownership-matrix.md:58 requires "one durable task store".

**Consequence:** A task created over gRPC is invisible to the HTTP/task UI, to capability-core, and to Temporal — it will never be scheduled, retried, or recovered, and disappears on restart, while `CreateTask` returns a task id that looks durable.

**Fix direction:** Back the gRPC task verbs with the same `proxy_to_capability_core("tasks", …)` calls the HTTP routes already use, or drop the gRPC verbs entirely; do not keep a fourth task store.

## Group A — Bypasses the owning plane (12)

_The gateway reaches a boundary directly that a plane owns. Fix: call the plane._

### PA-1 [critical] — permission-approval

**Gateway:** model-gateway's inline chat loop applies exactly one gate before dispatching any tool: `inline_tool_allowed` — a two-name denylist (`apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs:432-440`, `!matches!(name, "save_memory" | "browser_agent")`), checked at `tool_loop.rs:1490-1495`. Its own doc comment states MCP tools are deliberately exempt: "MCP tools are deliberately NOT withheld" (tool_loop.rs:434-436). `mcp__*` calls fall through at `tool_loop.rs:2025-2040` into `runtime_registries.rs::handle_proxy_mcp_tool`, whose only checks are ownership visibility (runtime_registries.rs:1333-1345), `server.enabled` (1350-1356), exact `tool_allowlist` membership (1357-1363), HTTPS-only transport (1367-1375) and JSON validity (1377-1382). No permission mode, no risk classification, no approval, no capability consult.

**Already exists:** execution-core owns a three-layer gate that runs before ANY dispatch, in one place: `apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs:373-385` (capability-core `EvaluatePolicy` → Allow/Ask/Deny, error = fail closed), `:390-396` (PreToolUse hook Deny/Ask), `:398-407` (`permission::evaluate_call` → Deny/AwaitApproval/Allow). The risk taxonomy is `apps/Model Plane/rust/services/execution-core/src/permission/mod.rs:106-157`, which gates every `mcp__*` name unconditionally (`:152-154`, "the server is third-party and the remote tool's side effects are unverifiable"). The capability binding is `apps/Model Plane/rust/services/execution-core/src/capability_policy.rs:326-354`, and MCP is explicitly excluded (`:349-351`, "Dynamic MCP tools require a durable registry-owned binding. Do not synthesize one"), so `capability_policy.rs:91-92` returns `permission_denied("tool has no governed capability binding")`. Production wires the real client (`execution-core/src/grpc.rs:696` `GrpcCapabilityPolicy::from_env`).

**Consequence:** MCP writes are reachable ONLY via the ungoverned path. A connected server's write tool (e.g. an ERP order-status mutation) fires from plain chat with no approval and no durable approval row, while an operator who sets that capability to deny/ask in capability-core sees zero effect on chat. There is also no `awaiting_approval` event, so nothing in the run-event feed or the approvals list records that a side-effecting external call happened. Secondary divergence in the same mechanism: `create_artifact`/`update_artifact` (tool_loop.rs:2523,2528) match the `create`/`update` keywords in `permission/mod.rs:107-120`, so the same tool pauses on the agentic path and runs silently in chat.

**Fix direction:** Route inline MCP dispatch through the owning plane instead of `handle_proxy_mcp_tool` directly: call `ExecutionCore.ExecuteStep` with the run's real `permission_mode` so `runtime_loop::execute_step` applies capability policy + hooks + `evaluate_call`, and give MCP tools a durable capability binding in capability-core (the gap `capability_policy.rs:349-351` names) so the plane can answer instead of failing closed. If inline MCP must stay latency-sensitive, the gateway should at minimum consult `CapabilityCore.EvaluatePolicy` and honour Ask by minting a real session-core approval — never decide locally.

### PA-2 [high] — permission-approval

**Status (2026-08-11): resolved in source and focused tests; staging policy-toggle
observation remains open.**

`pii_redaction_required` reads capability-core with the independently verified
capability bearer, accepts only an enabled input `pii_filter` as an authority to
redact, and falls back to redaction when that authority cannot be established.
Client `features` remain additive. The detailed text below is the historical
finding.

**Gateway:** model-gateway decides whether PII redaction runs from a CLIENT-supplied field. `apps/Model Plane/rust/services/model-gateway/src/moderation.rs:42-46` — `wants_moderation(features)` returns true iff the request body's `features` array contains `"moderation"` or `"pii"`. Call sites: `sse.rs:315-319` and `sse.rs:709-713` (features from `sse.rs:277` `let features = req.features.clone()`), `http_routes.rs:6250-6252` and `http_routes.rs:6470-6472`. The module's own header (`moderation.rs:3-8`) states "capability-core owns the *policy* (`safety_policies`: `pii_filter` / `content_safety` / `injection_defense`); the gateway is … the natural *enforcement* point" — but nothing in the gateway ever reads that policy. Grepping the gateway for a safety-policy fetch returns only this comment; the only capability-core HTTP calls are the MCP catalog (`http_routes.rs:2406-2430`, `2916-2999`) and tasks/cron proxying (`http_routes.rs:4608-4770`).

**Already exists:** capability-core owns a durable, per-org, enable-able, prioritised safety policy: table `safety_policies` at `apps/Model Plane/go/services/capability-core/migrations/0003_capabilities_registry.up.sql:290-314` (org_id, name, kind, config_json, applies_to, enabled, priority), full CRUD at `apps/Model Plane/go/services/capability-core/internal/api/registry_apis.go:1443-1606`, and reconcile emission on write at `registry_apis.go:1543-1545` (`reconcile.KindSafetyPolicy`, defined `internal/reconcile/reconcile.go:34`). A seeded capability `cap.safety.pii-filter` exists at `internal/registry/registry.go:49`. Master matrix line 114 assigns "safety" to capability-core.

**Consequence:** An organisation that enables `pii_filter` in capability-core gets no redaction whatsoever: any client that omits `features:["pii"]` (including every default SPA request) sends raw user content — emails, long number sequences — straight to the external model provider. The control appears configured and audits as configured, but has no effect. Conversely a client can silently turn the org's guardrail on or off per request, which is exactly the client-trust boundary the same codebase closes carefully for approval posture (`sse.rs:402-406`).

**Fix direction:** Resolve the effective safety posture server-side per org from capability-core (`GET /api/v1/safety`, cached with the reconcile consumer that already exists for MCP in `capability_consumer.rs`), then OR it with the client's `features` request the same way `profile::resolve_posture` lets a client only ratchet stricter — client input may add redaction, never remove it.

### SRS-4 [high] — sessions-runs-state

**Gateway:** model-gateway's `GET /v1/invoke/resume/:request_id` replays buffered assistant content with the caller's identity explicitly discarded. `sse.rs:3562-3576`: the handler binds `Extension(_claims): Extension<Claims>` (underscore — never read) and then calls `state.stream_buffers.replay_after(&request_id, after_seq)`, keyed by `request_id` alone. The buffer behind it is not process-local: `stream_buffer.rs:9-13` documents a Redis backend chosen so "a reconnect that lands on a different gateway replica still resumes", `:31-32` sets a 600s TTL, and `:34,158` show the key is `mp:gw:stream:{request_id}` — no org, no user.

**Already exists:** session-core is the system of record for message content (`capability-ownership-matrix.md:53`), and every other read path for that content in this same gateway is tenant-bound: `list_thread_messages` (`http_routes.rs:6046-6074`) passes `claims.org_id` into `ListConversationRequest` and session-core scopes the read; `session_flow.rs:421-459` `require_durable_run_owner_with_token` exists specifically to resolve a caller-named run against session-core's ownership authority and fails closed on `permission_denied`/`unavailable`.

**Consequence:** Any authenticated caller — from any org — who learns a `request_id` can read another tenant's assistant output for ten minutes. The id is not a secret the system protects: it is handed to the client in the `connected` SSE event (`sse.rs:1053`), carried in every `chunk`/`done` payload, and used as a URL path segment on the cancel route, so it lands in browser history, proxy access logs, screenshots and bug reports. `list_thread_messages` would refuse the same read; the resume path does not.

**Fix direction:** Bind the buffer to the tenant: include `(org_id, user_id)` in the Redis key and in `replay_after`, and reject a resume whose claims do not match — the same discipline `tool_result_handles.rs:583` already applies to tool payloads. Cheapest correct version: key `mp:gw:stream:{org}:{user}:{request_id}` and derive the prefix from the verified claims the handler already receives.

### INF-3 [high] — inference-providers

**Status (2026-08-11): same source fix as PA-2; live provider-bound verification
is still required.**

**Gateway:** PII redaction before content leaves for an external provider is gated on a flag the CLIENT sends, not on the org's policy. `moderation.rs:45-47` — `wants_moderation(features)` is true only when the request's `features` array literally contains `"moderation"` or `"pii"`. Every call site is the same shape: `sse.rs:315-316`, `sse.rs:709-710`, `http_routes.rs:6250-6251`, `http_routes.rs:6470-6471` — `if wants_moderation(&features) { redact_pii(&content).0 } else { content }`. The gateway never reads capability-core's safety surface: its only `capability_core_base_url` uses are MCP (`http_routes.rs:2424, 2528, 2926, 4621`; `mcp_oauth.rs:462, 638`).

**Already exists:** capability-core owns `safety_policies` as durable, org-scoped state with full CRUD: `apps/Model Plane/go/services/capability-core/internal/api/registry_apis.go:1440-1610` (`SafetyHandler` at `/api/v1/safety`, `applies_to`, `config_json`, `enabled`, `org_id`-scoped SELECT at `:1499`), table created in `migrations/0003_capabilities_registry.up.sql`. A concrete policy is seeded: `internal/registry/registry.go:49` — `{ID: "cap.safety.pii-filter", Name: "PII Safety Filter", Kind: models.KindSafetyPolicy, … Enabled: true, OrgID: "triodelab"}`, described as "Redact personally identifiable information from model I/O." The matrix assigns safety to capability-core (`master-ownership-matrix.md:114`; `capability-ownership-matrix.md:59-60`), and `moderation.rs:3-5` itself concedes it: "capability-core owns the *policy* (`safety_policies`: `pii_filter` / `content_safety` / `injection_defense`)".

**Consequence:** An org admin toggling `cap.safety.pii-filter` in capability-core changes nothing at all — the gateway never asks. In the other direction, any caller that omits `"pii"` from `features` (every non-SPA consumer of the public invoke surface, every integration, every retry that drops the field) ships raw email addresses and 13-19-digit card/IBAN-length sequences (`moderation.rs:57-74`) straight to the external model provider, while the org's own policy row says redact. For an EU tenant this is a compliance control that reports as enabled and is inert; and because the flag is client-supplied, it is self-disabling by omission rather than fail-closed.

**Fix direction:** Resolve the effective `pii_filter` policy for the request's org from capability-core (`GET /api/v1/safety`, org-scoped) and cache it the way `pricing.rs` caches cost-core's catalogue — TTL'd, fail-closed to the policy default rather than to "off". Keep `redact_pii` where it is; it is the right enforcement point. Treat the client `features` flag as at most an additional opt-IN, never as the thing that can turn an org policy off.

### BFF-2 [high] — bff-gateway

**Gateway:** Runs its own fetch-escalation waterfall and dials external scraping providers directly. apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/enhanced_fetch.rs:67 posts to `https://api.scrapfly.io/scrape` and :138 to `https://api.brightdata.com/request` using `state.enhanced_scrape_api_key`. It is invoked precisely when Quarry has already refused: knowledge/quarry.rs:153-158 (on a failed `/v1/scrape`) and knowledge/products.rs:216. Before that, knowledge/quarry.rs:122-142 makes its own driver-escalation decision — `retry_worthwhile` re-issues the scrape without browser rendering when the status is not 403/429 and elapsed < 12s. The result is cached (`state.cache.store(&key, &preview)`, knowledge/quarry.rs:157) and fed into product extraction.

**Already exists:** Quarry v2 is the sole owner of fetching and of the escalation policy: master-ownership-matrix.md:40 "Static fetch / TLS impersonation | Owner = Quarry v2"; :48 Firecrawl-like scrape formats Owner; §2 target table :84 `quarry-runtime` = "PageRunner, DriverPlan waterfall, security policy, deterministic execution" and :87 `quarry-security` = "DNS guard, URL signatures, blocklist hints, SSRF enforcement". Quarry's real waterfall is apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/driver_plan.rs:57-90 (stealth_level 0/1/2) plus apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/stealth.rs. Quarry has no scrapfly/brightdata adapter — grep across crates/ returns none.

**Consequence:** Content enters the product on a path no quarry-security guard, blocklist, or URL signature ever sees, and that produces no quarry-control job/run record — so the fetch has no audit trail, no per-org cost accounting, and no provider rate limit, while the resulting page is cached and fed into extraction with no source-trace evidence for Data Plane provenance. Because Quarry cannot see this tier, tuning Quarry's waterfall (a new stealth level, a newly blocked domain, a changed backoff) has no effect here: the two escalation policies drift apart permanently, and the gateway can successfully retrieve a target Quarry has decided must not be retrieved.

**Fix direction:** Move the Scrapfly/Bright Data adapters into `quarry-runtime`'s DriverPlan as a final waterfall tier behind quarry-security, and delete knowledge/enhanced_fetch.rs plus the `retry_worthwhile` block in knowledge/quarry.rs:122-142. The gateway should call `/v1/scrape` once and render whatever Quarry decides.

### BFF-5 [high] — bff-gateway

**Gateway:** Makes an organization-level ZDR / AI-mode policy decision that the owning service never makes. `require_support_ai_review` (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/inbox.rs:360-447) fetches org-core `/api/v1/organizations/{id}`, reads `metadata.interactiveRetention.zdr` and `metadata.supportAi.mode`, and returns 412 `zdr_ai_proposal_forbidden` (inbox.rs:421-430) or 412 `ai_review_mode_required` (inbox.rs:436-446). It gates the SPA's AI-proposal paths (inbox.rs:663 and the ticket dispatchers via the import at actions/dispatchers.rs:9).

**Already exists:** conversation-core owns the durable HITL AI-action queue: apps/Application Plane/conversation-core/conversation-core-go/internal/http/server.go:122 (`agents.POST /ai-actions`) into `Service.CreateAIAction` (internal/conversation/service.go:785-818). That function validates kind, org_id, conversation_id and payload shape — and contains no ZDR check and no `supportAi.mode` check. conversation-core is aware of the gateway's version: internal/clients/orgcore_client.go:61 names "the gateway's require_support_ai_review" in a comment, and already has `ZDREnabledOrgIDs` available for the lookup.

**Consequence:** apps/Application Plane/conversation-core/conversation-core-go/internal/consumers/model_action_proposed_consumer.go:101 calls `CreateAIAction` directly off NATS, entirely bypassing the gateway. A Model-Plane-proposed AI action is therefore persisted into a ZDR-enabled org's HITL queue with retained content, while the identical action from the SPA is refused — the ZDR guarantee holds for the human path and fails open for the automated one, which is the wrong way round. The same inversion applies to `supportAi.mode`: an org that has switched off AI review still accumulates model-proposed actions.

**Fix direction:** Move both checks into `Service.CreateAIAction` in conversation-core so the HTTP route and the NATS consumer share one gate. Keep the gateway check only as a fast-fail UX hint that quotes the owner's error code.

### RAG-6 [medium] — retrieval-rag

**Status (2026-09-15): resolved in source; the dead `searxng_url` field is pending deletion.**
quarry-edge gained `POST /v1/search/videos` next to `images` (`quarry-edge/src/routes.rs`,
`search_routes.rs` — same Bearer scope, same 501-with-hint when SearXNG is unconfigured, plus a
`SEARCH_QUERY` meter the images route had been missing), and `search_videos` is now an ordinary
`post_quarry` call taking the `AuthenticatedUser` extension and minting a quarry token exactly as
`search_images` does. `sanitize_videos` stays BFF-side — trimming results to embeddable fields is this
layer's job — but it now reads the edge's `thumbnail_src`, not SearXNG's raw `thumbnail`. The BFF's
`web_search_*` failure codes are relabelled `video_search_*` on this route so the SPA's existing
translation still applies. `state.searxng_url` is unread as of this change (the compiler reports
`field 'searxng_url' is never read`); its deletion is blocked only on five test-fixture `AppState`
literals outside the search domain, listed in `docs/gateway-integration-plan.md` §A.9.

**Gateway:** The verevonv3 BFF calls a search backend directly. `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/search.rs:451-512` (`search_videos`) builds `GET {state.searxng_url}/search?categories=videos&format=json&safesearch=1` with its own `reqwest` client, 15s timeout, and local result sanitizer (`sanitize_videos`, `search.rs:791+`). The module header at `search.rs:9` documents the exception: "`videos` → SearXNG `/search?categories=videos`" while every sibling route (`web`, `images`, `similar`, `suggest`, `answer/stream`) goes to `quarry_edge_url`. No `org_id` is sent.

**Already exists:** quarry-edge owns the SearXNG verticals and already implements this exact pattern for images: `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/search_routes.rs:596-680` (`images`) — "focused SearXNG image search", "Reuses the configured `searxng_url`", returns 501-with-hint when unconfigured — routed at `quarry-edge/src/routes.rs:158`, behind the `search:read` scope check at `quarry-edge/src/auth.rs:464`, with quarry-edge's response cache/TTL policy (`quarry-edge/src/cache.rs:101`) and rate-limit mapping. Master matrix §2 line 83 makes quarry-edge the "Public REST/SSE, request normalization, ZDR guards, cache admission" boundary.

**Consequence:** Video search gets none of quarry-edge's boundary work: no `search:read` scope check, no cache admission or TTL policy, no provider circuit-breaking, no ZDR guard, and — because `search_videos` never sends `org_id` — no per-tenant scoping or attribution at all, so the query text of every tenant's video search reaches SearXNG unlabelled and unaudited. It also forces the frontend gateway to hold network reachability and config for a backend the BFF's own comments (`config.rs:180`) say should stay private to the data/ingestion networks, and it means "how do we talk to SearXNG" now has two implementations that will drift (quarry-edge's returns 501-with-hint when unconfigured; the BFF's returns a 502 with a SearXNG status code in the user-visible message, `search.rs:493-502`).

**Fix direction:** Add `POST /v1/search/videos` to quarry-edge next to `images` (same `SearXNGImages`-style provider, same auth scope, same cache), then make `search_videos` a `post_quarry` call like every other route in the file and drop `state.searxng_url` from the BFF config.

### SRS-5 [medium] — sessions-runs-state

**Status (2026-08-11): resolved in source; multi-replica cancel delivery remains open.**

The source fix is now applied: the verified Claims extension is required by
the cancel handler, and CancelRegistry binds each request id to the originating
tenant and user. A mismatched owner receives the same inactive-stream 404, so
the old cross-tenant cancellation/existence-oracle concern is closed. The
remaining work is moving this owner-bound signal to a durable/NATS subject for
multi-replica deployments.

**Gateway:** `POST /v1/invoke/:request_id/cancel` makes a run-lifecycle decision with no ownership check. `http_routes.rs:5803-5806`: the handler signature is `(State(state), Path(request_id))` — no `Extension<Claims>`, no run-owner lookup — and it calls `state.cancels.cancel(&request_id)`. The registry it flips is `request_id`-keyed with no tenant component (`cancel_registry.rs:16-17`). The flag is not cosmetic: the stream loop reads it at `sse.rs:1552`, sets `cancelled`, and at `sse.rs:1886-1893` calls `cancel_direct_inference_run_authenticated`, which issues session-core `CancelRun` (`session_flow.rs:785-803`).

**Already exists:** session-core owns run metadata and lifecycle (`capability-ownership-matrix.md:54`). The gateway already has the correct gate and uses it on its own gRPC surface: `grpc.rs:163-179` `require_durable_run_owner` -> `session_flow.rs:421-459` `require_durable_run_owner_with_token` -> session-core `ResolveRunOwner`, which returns `permission_denied` for a run the caller does not own and `unavailable` (fail-closed) when the authority is unreachable. `grpc.rs:1435` applies exactly this before `record_trajectory` touches run-scoped state.

**Consequence:** An authenticated caller in any org who holds a `request_id` terminates another tenant's in-flight chat turn. Worse, the durable record looks legitimate: session-core accepts the `CancelRun` because it is submitted with the *victim's own* verified bearer from inside their stream task, so the run is durably marked cancelled with no trace that an outsider triggered it. The audit trail attributes the cancellation to the wrong actor.

**Fix direction:** Give `invoke_cancel` the `Extension<Claims>` it is missing and either store `(org_id, user_id)` alongside the flag in `CancelRegistry::register` and require a match, or resolve the request's run through `require_durable_run_owner_with_token` before flipping anything — the helper already exists two modules away.

### SRS-6 [medium] — sessions-runs-state

**Gateway:** Chat-artifact version history is per-process state. `artifacts.rs:98-127` `ArtifactVersionStore` is a plain `DashMap<(thread_id, artifact_id), u32>`; the module doc concedes the loss at `artifacts.rs:20-24` ("A deploy resets counters... if artifact history ever needs to survive a restart, this is the seam to swap for session-core persistence"). That counter is load-bearing, not advisory: `tool_loop.rs:1573-1584`, on `update_artifact`, hard-errors with "no artifact '{id}' exists in this conversation — use create_artifact for a new one" whenever `current_version` returns `None`.

**Already exists:** session-core is the system of record for thread-scoped state — `threads`, `messages`, `checkpoints`, and an append-only `events` log carrying `correlation_id`/`resource_ref` (`migrations/0001_init.sql:4,14,46,68`, `migrations/0002_events_and_ordinals.sql:26-33`). It also already models durable artifacts, though task-scoped: `task_artifacts` (`migrations/0004_tasks_cron_hooks_skills_memory.sql:100-116`).

**Consequence:** This is not hypothetical multi-replica hand-wringing: the gateway's own resume buffer is deliberately Redis-backed *because* multiple replicas are a real deployment posture (`stream_buffer.rs:9-13`), while the artifact map is not. So on a two-replica deployment, or after any deploy, "make the intro shorter" on an artifact the user is looking at right now returns an error telling them the artifact does not exist in this conversation. The failure is user-visible, confusing, and unrecoverable except by re-creating the artifact under a new id — which orphans the version history the client already rendered.

**Fix direction:** Persist the version line where the thread lives: either an `artifacts` table in session-core keyed `(org, thread, artifact_id)` with the version as a monotonic column (mirroring the ordinal triggers in `migrations/0002`), or reuse the `events` log with `resource_ref = artifact/{id}` and derive the version from the ordinal. Until then, `update_artifact` should degrade to a create-with-new-version rather than a hard error, so a deploy cannot break a live canvas.

### BI-3 [medium] — browser-ingestion

**Status (2026-09-15): resolved in source — same fix as RAG-6, which describes this bypass from the
retrieval side; see there for detail.** Video search now enters quarry-edge at `POST /v1/search/videos`
and therefore inherits the boundary work this finding said it lacked: org scoping and attribution from
the forwarded user/token, the edge's cache and rate-limit mapping, and one metered `SEARCH_QUERY` unit
per query. The two-pointers-can-drift concern is gone with it — the gateway no longer reads a SearXNG
URL at all, so Quarry's instance is the only one the product can reach. Note the fallback-chain point
in the paragraph below is only partly addressed: `/v1/search/videos` is a focused SearXNG vertical like
`/v1/search/images`, not a `smart_router` fan-out, so a SearXNG outage still fails the VIDEOS tab while
web search survives. That is now a property of one provider path inside the edge rather than of a
second, un-attributed client in the BFF, and is tracked as such.

**Gateway (historical):** `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/search.rs:472` — `search_videos` built `format!("{}/search", state.searxng_url)` and queried SearXNG directly with `categories=videos`, `format=json`, `safesearch=1`, then shaped the raw provider JSON via `sanitize_videos`. The handler took no `AuthenticatedUser`, so no org reached the provider call.

**Already exists:** SearXNG is a Quarry-owned search provider: `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/serp.rs:422-476` is the `SearchProvider` impl (same `categories` query param at `serp.rs:448` and `:549` for images), sitting behind `smart_router.rs`'s Tantivy→SearXNG→Brave fan-out and `rerank.rs`'s `RerankingSearchProvider`. The very same BFF file routes every other search mode correctly to `quarry_edge_url` — `search.rs:235` `/v1/search`, `:327` `/v1/search/similar`, `:381` `/v1/search/images`, `:425` `/v1/search/suggest`, `:588` `/v1/answer/stream` — and its own module doc (`search.rs:3-5`) states "All web search infrastructure lives inside quarry-edge".

**Consequence:** Video search has no fallback chain: if SearXNG is down the handler hard-fails 502 (`search.rs`, the `Ok(resp)` non-success arm), where `smart_router` would have fallen through to another provider — so videos break while web search keeps working, from the same outage. It also gets no reranking, no `provider:` telemetry (`serp.rs:468`), no per-org usage attribution, and no ZDR consideration. And `SEARXNG_URL` in the gateway config is a second pointer that can drift to a different SearXNG instance than Quarry's, producing two different result sets for the same query in the same product.

**Fix direction:** Add `/v1/search/videos` to `quarry-edge/src/routes.rs` (it currently exposes only `/v1/search`, `/images`, `/similar`, `/suggest` — `routes.rs:157-160`) backed by the existing `serp.rs` SearXNG provider, and make `search_videos` a proxy like its five siblings. Delete `searxng_url` from the gateway config.

### BFF-7 [medium] — bff-gateway

**Gateway:** Acts as the system of record for durable user content held in process memory. `StudioStore` (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/studio.rs:51-59) is an `Arc<RwLock<BTreeMap<String, Vec<StudioProject>>>>` with no Redis and no upstream. Canvas projects and blocks are created, read, updated and listed straight from it (studio.rs:225, :250, :266, :283, :320, :337, :360), with `seed_scope_projects` (studio.rs:586) seeding defaults. The module's own doc comment states it: "This is a narrow gateway-local repository for canvas projects and blocks" (studio.rs:1-5).

**Already exists:** CLAUDE.md:29 — "Application Plane owns collaborative/realtime workspace projections and notifications"; master-ownership-matrix.md:30 puts human-facing canvas UX above the core planes but its durable state in a plane. The intended owner exists only as a placeholder: apps/Application Plane/studio-core/ contains a single file, studio.md — no cmd/, no internal/, no routes.

**Consequence:** A user's canvas work is destroyed by every gateway restart or redeploy, and is invisible to any other gateway replica — behind more than one pod, a user's project list depends on which instance answers, so saving on one and reloading on another silently shows an empty workspace. The content is also outside every GDPR path: the gateway has no NATS client, so an org-erasure fan-out cannot reach it, and the DSAR export in privacy.rs (which proxies user-core) will not disclose it, meaning the org holds user-authored content it cannot enumerate or delete.

**Fix direction:** Either build studio-core in the Application Plane (matching the studio.md contract) and make studio.rs a proxy, or persist projects behind an existing Application Plane owner. Until then the surface should be labelled non-durable in the UI rather than presented as saved work.

### BFF-9 [medium] — bff-gateway

**Gateway:** Calls the Zammad admin API directly with a shared token and no org scope. apps/Frontend Plane/verevonv3/apps/gateway/src/domains/agents.rs:76-101 GETs `{ZAMMAD_API_URL}/api/v1/users?role=Agent`, `/api/v1/groups` and `/api/v1/macros` with `format!("Token token={}", state.zammad_api_token)` (agents.rs:88). The handler `chatbot_runtime` (agents.rs:36-74) resolves no organization at all — it takes only `State(state)` and returns raw counts.

**Already exists:** Zammad is owned outside the Frontend Plane: apps/Ingestion Plane/services/support-worker/src/activities/patch-zammad.ts is the plane-owned mutation path (with triage/sla/csat workflows alongside it) and apps/Application Plane/zammad-foundation/bootstrap/src/bootstrap.ts owns provisioning. CLAUDE.md — "Provider actions go through integration-corev2's actions surface; the frozen operation/capability contract is docs/actions-surface-operations.md". The gateway's own ticket surfaces correctly go through conversation-core instead (domains/tickets.rs:1-4).

**Consequence:** Every authenticated user of every org receives the same instance-wide Zammad agent/group/macro counts from the shared support desk — a cross-tenant read of another tenant's support topology, since no org filter is applied anywhere in the handler. There is also no integration-corev2 audit event for the provider call, no per-org connection check (a tenant with no support integration still gets `connected: true`), and no provider-side rate limit; and because the credential lives in the gateway's env rather than the owning plane, a Zammad token rotation breaks this surface independently of the plane that manages the credential.

**Fix direction:** Route the readiness probe through integration-corev2's actions surface (or conversation-core, which the rest of tickets.rs already uses) so the org's own connection and capability determine the answer, and drop `ZAMMAD_API_URL`/`ZAMMAD_API_TOKEN` from the gateway.

## Group D — Divergent second implementation (31)

_Two implementations of one rule that can disagree. Fix: delete one, usually the gateway's._

### BFF-1 [critical] — bff-gateway

**Resolved 2026-08-11:** the BFF no longer owns or persists conversation content. `GET /api/v1/chat/threads/:id/transcript` reads through to Model Gateway `/v1/threads/:id/messages`, which is backed by Session Core's canonical conversation. `PUT` still accepts the legacy browser snapshot shape for wire compatibility but ignores turns/task steps; presentation mutations go only to Session Core. The removed `ChatHistoryStore`/Redis transcript path means BFF replicas, DSAR handling, and ZDR no longer have a second content store to reconcile. The richer task-step/run evidence remains a separate read-model gap, not a duplicate transcript owner.

**Already exists:** session-core owns it outright: Model Plane/docs/capability-ownership-matrix.md:53 "Thread / message timeline | session-core | Postgres | gateway ingress". Storage is apps/Model Plane/rust/services/session-core/migrations/0001_init.sql:4 (`threads`) plus a `messages` table read by `list_conversation` (apps/Model Plane/rust/services/session-core/src/grpc.rs:2387-2428). The gateway ALREADY proxies the durable surface at apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/json_handlers.rs:76-99 (`/api/v1/chat/threads/:id/messages` -> model-gateway `/v1/threads/{id}/messages`).

**Previous consequence (closed):** `/transcript` and `/messages` no longer diverge, and the BFF has no durable transcript to survive Session Core erasure or violate ZDR. DSAR/export and rich activity evidence still need their own owner-backed projections before they can be called complete.

**Remaining direction:** Add an owner-backed activity projection for orchestration todos and run/artifact evidence; do not reintroduce transcript storage into the BFF.

### RAG-1 [high] — retrieval-rag

**Status (2026-08-11): resolved in source; live Data Plane authorization still required.**

**Previous gateway behavior:** model-gateway built its RAG context block from candidates without requesting Data Plane's packer. It omitted `context_budget_tokens` and `context_format` on both the chat fallback and `knowledge_search` tool paths.

**Already exists:** `retrieval-engine-rs` owns context packaging (master matrix §2 line 99). `apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs:1100-1119` runs `pack_context_with_pins(&pins, &reranked, &sources, budget, &format)` — token-budgeted, and it packs **the org's pinned permanent-memory (CAG) facts FIRST, in priority order, before retrieval candidates** (`context_pack/mod.rs:15-40, 103-152`; pins from `context_pins::list_pins`). It runs only `if let Some(budget) = req.context_budget_tokens` (orchestrator.rs:1100). The proto exposes it: `apps/Data Plane v2/proto/retrieval_v2.proto:24-25`. session-core — the canonical context-assembly owner — does use it: `apps/Model Plane/rust/services/session-core/src/grpc.rs:2802-2822` sends `top_k: 10`, `context_budget_tokens: Some(max_tokens/4)`, `context_format: Some("toon")`. And the gateway's own pass-through relay forwards it correctly (`dataplane.rs:671-672`), proving the wiring exists.

**Resolved behavior:** both paths now request a bounded `context_budget_tokens` pack in `toon` format, and the gateway renders `context_pack.facts` first (preserving Data Plane's pinned-fact ordering) while retaining only local citation numbering and injection-warning framing. Local character limits remain presentation safety ceilings, not retrieval-selection policy. A live positive proof is still blocked until the verified user bearer is accepted by the running Control/Data Plane authorization path.

**Remaining direction:** derive the fixed budget from the negotiated model answer budget once that field is propagated through the chat request; keep the current 1,600-token bound as the safe fallback.

### RAG-2 [high] — retrieval-rag

**Gateway:** `apps/Model Plane/rust/services/model-gateway/src/relevance.rs` (~1100 lines) is a full relevance scorer + filter over web-search hits: a stemmer and Norwegian+English stopword lists (`relevance.rs:290-312, 458-481`), a term-overlap score (`lexical_overlap`, `relevance.rs:689-714`), a soft-demoted host class (`DEFAULT_SOFT_HOSTS`, `relevance.rs:171`, penalty `0.35` at line 91, plus a `0.15` profile-path penalty at line 102), a topic→authoritative-host bonus table (`IMPLIED_DOMAINS`, `relevance.rs:216`, `+0.25` at line 110), and a keep threshold of `0.30` (line 123). `assess` blends them: `base = 0.6*provider + 0.4*lexical` (`relevance.rs:738-744`), `score = clamp(base + bonus - demotion)` (line 781). `keep_mask` (line 822) drops everything below threshold. Applied to every chat `web_search` (`tool_loop.rs:3271-3277` → `gate_web_search_outcome`) and to deep research (`deep_research.rs:810-855`). Host policy is configured by a gateway env var, `VEREVON_RELEVANCE_SOFT_HOSTS` (`relevance.rs:421`).

**Already exists:** Quarry owns web-hit relevance. `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/rerank.rs:46-133` is `ModelPlaneSearchReranker` (asks the Model Plane to score how well each result answers the query), wrapped as a transparent `SearchProvider` at `rerank.rs:234-270`; `apply_rerank` (`rerank.rs:169-220`) is a pure reorder that attaches `score` in `[0,1]`. Quarry also owns source selection as a first-class request parameter: `quarry-runtime/src/serp.rs:61-66` `SearchOptions.include_domains` / `exclude_domains` — "Applied as `site:` / `-site:` operators on remote SERP providers".

**Consequence:** A hit Quarry's LLM reranker ranked #1 with score 0.40 is re-scored by `assess` as 0.6*0.40 + 0.4*0.0 = 0.24 when the page title/snippet shares no stemmed term with the question — below the 0.30 keep line — so it is dropped. `gate_web_search_outcome`'s doc comment (`tool_loop.rs:2994-2998`) states filtering happens *before* the citation events exist, so that hit "never becomes a source at all": the user's Kilder tab shows the plane's own top result labeled `filtered as irrelevant to the question` (`relevance::filtered_reason`, `relevance.rs:856`). Separately, the soft-host demotion runs *after* the SERP call, so the demoted results are still paid for in provider quota and latency — `exclude_domains` would have stopped them at the provider. And "which hosts to avoid" now has two homes (a gateway env var and Quarry's per-request options) that no operator can reconcile.

**Fix direction:** Extend `WebSearchRequest` with `include_domains`/`exclude_domains`/`time_range` and let Quarry apply the domain policy at the provider, so `DEFAULT_SOFT_HOSTS`/`IMPLIED_DOMAINS` move to the plane that owns source selection. Keep the *threshold gate itself* at the edge only if it is re-expressed as a function of Quarry's score alone (Quarry never thresholds — that filter is a genuine gap) and stop letting a locally-computed lexical score override the plane's semantic judgement.

### SRS-2 [high] — sessions-runs-state

**Status (2026-08-11): resolved for Session Core thread/run state; full DSAR
remains explicitly broader.** `DeleteThread`/`DeleteThreads` are now
owner-bound Session Core RPCs exposed by Model Gateway `DELETE /v1/threads/:id`
and `DELETE /v1/threads`. The transaction removes the owned thread, messages,
run descendants, events/audit outbox evidence, plans, tasks, approvals, and
encrypted continuation records; it never lets one user erase another user's
thread. `POST .../archive` remains non-destructive. The local proof is a
compiling Session Core test plus an ignored disposable-Postgres owner/scope
test. This is not a claim that thread-associated Letta semantic memory has
been erased: that backend needs its own enumerate/delete receipt and partial
failure contract before it can satisfy a DSAR claim.

**Gateway:** The BFF implements conversation deletion entirely against its own copy. `delete_thread` (history.rs:275-299) drops the thread from the BFF index and deletes `chat-history:transcript:*`; `clear_threads` (history.rs:301-320) does the same for every thread. Neither calls the Model Plane. Meanwhile `list_threads` (history.rs:152-165) fetches session-core's list through `read_durable_threads` (history.rs:349-386) and folds it back in via `merge_thread_indexes` (history.rs:436-475), which re-adds any thread present durably but absent from the BFF index (history.rs:468-470). The user-facing entry point is real: `CoreSidebar.tsx:353-361` `clearHistory()` calls `clearChatThreads()`.

**Already exists:** session-core owns thread lifecycle (`migrations/0001_init.sql:4`). There is no durable thread-delete anywhere to delegate to: no delete-thread route in model-gateway `http_routes.rs` or `grpc.rs`, and no `DeleteThread` RPC in `session-core/src/grpc.rs`. The only durable thread deletion in the whole plane is org-wide GDPR erasure (`session-core/src/gdpr.rs:265,326,343` — `DELETE FROM messages`, `DELETE FROM events`, `DELETE FROM threads WHERE org_id = $1`).

**Consequence:** A user clicks "clear history": the sidebar empties, and on the very next listing every conversation reappears — now titled with session-core's raw auto-title instead of the AI-generated one, because `merge_thread_indexes` only fills an empty/placeholder title (history.rs:455-457). Opening one still returns the full durable messages via `/messages`. The transcript half (task steps, turn metadata) is genuinely and irrecoverably gone, so from then on the two stores hold permanently different views of the same conversation. Functionally, "delete my chats" is a lie the product tells, and the residue is exactly the content the user asked to remove.

**Fix direction:** Add a durable thread delete to session-core (org-scoped, the same authority shape as `ListConversation`), expose it through model-gateway, and make the BFF's DELETE a proxy that deletes durably first and only then drops its presentation index. Until that RPC exists, the BFF DELETE should fail loudly rather than half-succeed.

### CR-04 [high] — capability-registry

**Gateway:** `HookRegistry` (runtime_registries.rs:1583-1592) stores lifecycle hooks in memory. `handle_register_hook` (:1600-1624) validates the event against its own vocabulary `pre_tool | post_tool | on_error | on_complete` (:1607-1615) and stores a `Hook { hook_id, event, tool_scope, callback_url, enabled }` (proto/model_plane/v1/gateway.proto:1219-1228). The store is read by nothing except its own two gRPC handlers (grpc.rs:1665, :1674) — no hydration, no write-through, no delivery to any enforcement point.

**Already exists:** execution-core owns hook firing: `rust/services/execution-core/src/hook/mod.rs` — an engine with wire tokens `pre_tool_use`/`post_tool_use` (:34-37) and decisions `Allow`/`Deny`/`Ask` (:41-46), whose rules "arrive on the `hook_context` JSON that flows into `ExecuteStep`" (:11-13). Durable hook config exists as session-core `hook_configs` (rust/services/session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql:165-178) with `hook_type` (pre_tool|post_tool) and `action` (approve|block|modify|log). capability-ownership-matrix.md:61 names capability-core the registry owner — but capability-core has no hooks table and no `/api/v1/hooks` route (verified: no hooks entry in migrations/0003 and no HandleFunc in internal/api/).

**Consequence:** An org registers a `pre_tool` hook intended to block a dangerous tool, receives HTTP 200 and a `hook_id`, and lists it back successfully — and no tool call is ever intercepted. It is a safety control whose entire observable behavior is a successful-looking acknowledgement. A future fix that naively forwards these rows to execution-core would still silently no-op, because `HookRule::event` would never match.

**Fix direction:** Pick one vocabulary and one owner. Register hooks against the durable store (session-core `hook_configs`, or move it to capability-core to match the matrix) using execution-core's `pre_tool_use`/`post_tool_use`/`allow|deny|ask` shape, have the gateway relay registration there, and have the run path load the org's rules into `hook_context` so `hook/mod.rs` actually evaluates them. Until that exists, `RegisterHook` should fail with `unimplemented` rather than returning success.

### CBU-1 [high] — cost-billing-usage

**Gateway:** model-gateway runs a second, self-contained spend-and-budget system for fine-tuning that never touches cost-core. `apps/Model Plane/rust/services/model-gateway/src/finetune_routes.rs:478-531` (`enforce_budget_estimate`) reads its caps from gateway env vars (`FINETUNE_PER_JOB_BUDGET_USD`, `FINETUNE_ORG_BUDGET_USD` — lines 487-488), reads the org's month-to-date spend from session-core's `finetune_jobs` table via `GetOrgMonthlySpend` (lines 489-497), and then makes the allow/deny decision locally in `budget_verdict` (`finetune_routes.rs:136-169`). No call to cost-core exists anywhere on this path — `grep -rn 'cost-core|COST_CORE|cost/record' finetune_routes.rs finetune_poller.rs` returns nothing.

**Already exists:** cost-core owns both the ledger and the budget decision. `apps/Model Plane/go/services/cost-core/internal/server/server.go:116` registers `POST /api/v1/budget/check`; the handler (`server.go:440-506`) aggregates the durable `cost_entries` ledger for the org and returns an allow/deny verdict. The SAME gateway already calls it correctly on the inference path — `apps/Model Plane/rust/services/model-gateway/src/budget.rs:44-140`. cost-core has zero awareness of fine-tuning (`grep -rln finetune go/services/cost-core/` returns nothing).

**Consequence:** The org monthly fine-tune cap is unenforceable for completed work. Concretely: an org with `FINETUNE_ORG_BUDGET_USD=50` submits a 10,000-example gpt-4 fine-tune (server floor = 10000 x 0.005 = $50.00). While `queued`/`running` it counts against the cap. The moment it reaches `succeeded`, its status leaves the `('queued','running')` set, the SQL falls through to `actual_cost_usd` = 0, and the org's month-to-date spend reads $0.00 again — so the next $50 job is admitted, and the next, indefinitely. Separately, in the other direction: because fine-tune cost never reaches cost-core's ledger, `GET /api/v1/cost/aggregate` (which is what the BFF's cost dashboard renders, `verevonv3/apps/gateway/src/domains/cost.rs:123-127`) under-reports the org's real spend by the entire fine-tuning bill, and the inference-side budget guard in `budget.rs` therefore keeps approving inference for an org that has already burned its money on training.

**Fix direction:** Make cost-core the single ledger and the single decision point for fine-tune spend too: have the finetune poller `POST /api/v1/cost/record` a real cost entry when a job reaches a terminal state (which requires actually reading Azure's billed cost instead of hardcoding 0.0), and replace the local `budget_verdict` + env caps with a `POST /api/v1/budget/check` call carrying the per-job estimate — the same call `budget.rs:86-101` already makes. session-core's `finetune_jobs` stays the job-lifecycle record; it stops being a spend ledger.

### INF-1 [high] — inference-providers

**Gateway:** model-gateway keeps its own LLM response cache and consults it BEFORE calling inference-core, and the gRPC path keys it on the raw user message only. `grpc.rs:527-541` fetches memory context and builds the real prompt (`build_messages(&memory_context, &content)`) plus an `InferRequest` carrying `structured_output_schema`, `temperature` and `max_tokens` (`grpc.rs:~1290 build_infer_request`); `grpc.rs:544` then calls `try_serve_from_cache(..., &content, req.zdr)`, which at `grpc.rs:370` does `cache.lookup(content, scope, zdr)` — scope is only `(org_id, user_id, model)` (`grpc.rs:365-369`). Writes use the same raw key (`grpc.rs:623-633`, `grpc.rs:1023-1035`), and the streaming twin repeats it at `grpc.rs:758-766`. There is NO eligibility gate on this path: `langcache::TurnCacheability` (tools/citations/structured-output exclusions) exists only in `sse.rs:1325-1348` — grep for `TurnCacheability` in `grpc.rs` returns nothing.

**Already exists:** inference-core's `PromptCache` — `apps/Model Plane/rust/services/inference-core/src/cache.rs:31` (struct) with `cache_key` at `cache.rs:67-95` hashing org_id, user_id, provider_hint, model, EVERY message role+content, temperature, max_tokens, and the response-shaping fields (tools / tool_choice / structured-output schema). It is wired into the live path at `inference-core/src/provider/fallback.rs:701` (`self.cache.get(req)`) and `:741` (`self.cache.put`), constructed at `fallback.rs:513`/`:536` with a 300s default TTL.

**Consequence:** A gRPC `Invoke` carrying `structured_output_schema` whose last user message matches an earlier plain-text `Invoke` from the same (org, user, model) is served the earlier PLAIN-TEXT answer, returned as a normal `InvokeResponse` with `stop_reason: "end_turn"` and zero tokens (`grpc.rs:396-406`) — the caller parses it as JSON and fails, with no error anywhere to explain it. Same class of failure for a differing `temperature`/`max_tokens`, and for memory drift: `fetch_memory_context` at `grpc.rs:530-538` is in the prompt but not in the key, so after a user's memory changes, the pre-change answer keeps being replayed for up to an hour. Because the hit short-circuits before `client.infer(...)` at `grpc.rs:560`, inference-core's ZDR handling, routing/intent layer and its own cache are all skipped for that turn.

**Fix direction:** Delete the gateway-side response cache on the gRPC paths and let inference-core's `PromptCache` do the job it already does correctly — it sits behind the same call and its key is strictly a superset. If a gateway-tier cache is wanted for latency, it must key on the rendered assembled prompt (the `render_cache_prompt` helper `sse.rs:1349` already uses) and reuse the `TurnCacheability` gate, so the two caches cannot disagree about eligibility.

### BFF-3 [high] — bff-gateway

**Gateway:** Computes the social publish verdict locally and substitutes it for the owning service's. `enqueue_core_publish` (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/social.rs:1595-1648) POSTs the real social-core `/api/v1/social/posts/{id}/publish-jobs`, then discards the returned job's status and replaces it with `build_publish_result(&post, &accounts)` (social.rs:1642), copying only `id` and `idempotency_key` from the real job. `build_publish_result` (social.rs:2668-2740) decides `blocked` / `queued` / `partial` per platform from the gateway's own hardcoded `platform_adapters()` capability table (social.rs:2171-2265) and a local `social.post.write` check.

**Already exists:** social-core owns the publish job and its eligibility: apps/Application Plane/social-core/internal/social/service.go:588-648 (`BuildPreviews`, `characterLimit`, `mediaRequired`, `previewWarnings`), and the `/publish-jobs` endpoint the gateway just called returns the job's own status. Provider capability and connection state are integration-corev2's, per CLAUDE.md — "Provider actions go through integration-corev2's actions surface; the frozen operation/capability contract is docs/actions-surface-operations.md".

**Consequence:** The job is already enqueued upstream by the time the verdict is computed, so the two disagree in the dangerous direction: the UI reports "blocked — publish is blocked until the account is connected" for a post social-core has accepted and will run, so a user believes they stopped a post that goes out anyway. In the other direction a stale entry in the gateway's `required_capabilities` list shows "blocked" for a platform that would publish fine, and the user works around a limit that does not exist. Publish blocking in the UI has no causal relationship to what publishes.

**Fix direction:** Render the status social-core returned on the publish job. If the SPA needs per-platform detail, add it to social-core's publish-job response (one owner) rather than recomputing it from a gateway-side capability table.

### BFF-4 [high] — bff-gateway

**Gateway:** Rewrites and truncates the user's post copy per platform, and keeps a second copy of the platform limit table. `adapted_copy` (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/social.rs:2631-2666) composes platform-specific text — prepending the title and appending gateway-authored sentences such as "Save this for the next planning review." for Instagram and a "What this means for teams…" clause for LinkedIn — then truncates to `max_characters`. `build_platform_previews` (social.rs:2590-2629) serves this whenever social-core returns no previews (social.rs:1947-1957). `platform_adapters()` (social.rs:2171-2265) hardcodes max_characters 3000/280/2200/63206/2200/250 and media_required per platform.

**Already exists:** social-core owns previews and the limit table: apps/Application Plane/social-core/internal/social/service.go:588-601 `BuildPreviews` returns `Content: body` unmodified, with `characterLimit` (service.go:615-630, the same 280/3000/2200/63206/2200/250 values), `mediaRequired` (service.go:633-635), and `previewWarnings` (service.go:637-649) which only warns on over-limit rather than truncating.

**Consequence:** The preview the user reviews and approves is gateway-authored text the platform will never receive — `validate_create_post` (social.rs:2277-2317) sends the RAW body upstream — so approval happens over content that differs from what publishes. Separately, the limit table now exists in Rust (social.rs:2171) and Go (service.go:615); the next time a platform changes a character limit only one will be updated, and the UI will start rejecting or truncating posts social-core accepts, or vice versa.

**Fix direction:** Delete `adapted_copy`, `platform_adapters` and `build_platform_previews`; render social-core's `BuildPreviews` output. If per-platform copy adaptation is a real product feature it belongs in the Model Plane as a generation step whose output the user edits and stores, not as silent BFF string formatting.

### AZI-3 [medium] — authz-identity

**Gateway:** The BFF fabricates a full entitlement and quota set locally when billing-core returns 5xx: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/billing.rs:286-310` (`default_billing_account`) hardcodes `plan: "free"`, `subscription_state: "active"`, `entitlements: {feature.chat: true, feature.api_keys: true, feature.audit_logs: true, feature.integrations: false, feature.sso: false}`, and `quota_limits: {api_calls: 1000, users: 5, storage_mb: 1000}`. It is returned as a **200 OK** at `domains/billing.rs:78-86` whenever `response.0.is_server_error() && billing_account_fallback_enabled(&state)`, where the gate is `cfg!(debug_assertions) || state.allow_dev_actor_headers || state.allow_dev_auth_bypass` (`domains/billing.rs:282-284`).

**Already exists:** billing-core owns entitlements and quotas, and the same gateway file asks it everywhere else: `domains/billing.rs:100-104` proxies `GET {billing_core}/api/v1/billing/orgs/{org}/entitlements/{feature}`, `:128-132` proxies `/quotas/{metric}`, and `domains/leads.rs:74-100` (`require_leads_entitlement`) gates a paid feature on billing-core's answer and 402s otherwise. `domains/orgs/info.rs:33-46` proxies org entitlements to org-core. The fail-closed rule is `apps/master-ownership-matrix.md:13` ("must fail closed when Control authority is unavailable") and `:140` ("an authority outage must never create or widen access").

**Consequence:** During a billing-core outage the SPA is told the org is on the free plan with SSO and integrations disabled and a 1000-call quota, regardless of what the org actually pays for — an enterprise tenant sees its SSO and integrations affordances disappear and its quota misreported, with `metadata.fallback` as the only signal. The status code is 200, so no caller can distinguish a real answer from a fabricated one. The blast radius is bounded today because the flag is dev-gated (`config.rs:368-380` refuses dev flags when `APP_ENV=production`), but `cfg!(debug_assertions)` is a build property, not an environment one — a debug-profile build deployed anywhere enables it with no env var set.

**Fix direction:** Delete the fabricated account and propagate billing-core's failure honestly — return 503 with an `billing_authority_unavailable` code, exactly as `middleware.rs:227-236` already does for the membership authority. If the SPA needs to stay usable during an outage, let it decide that from an explicit error code rather than being handed invented entitlements it cannot tell apart from real ones.

### PA-5 [medium] — permission-approval

**Gateway:** model-gateway holds a third hook store whose rules can never fire. `apps/Model Plane/rust/services/model-gateway/src/runtime_registries.rs:1584-1646` — `HookRegistry` with `handle_register_hook`/`handle_list_hooks`, exposed at `grpc.rs:1665` and `grpc.rs:1674`. Nothing in the gateway ever converts a registered hook into a `hook_context`: both ExecuteStep call sites send it empty (`tools.rs:156` `hook_context: String::new()`, `browser_run.rs:420` same), and `RunAgentRequest` carries no hook field at all (`proto/model_plane/v1/execution.proto:38-80`). There is no hydration from the durable owner — `capability_consumer.rs:37` filters `capability.mcp_server.` only.

**Already exists:** Firing is owned by execution-core `apps/Model Plane/rust/services/execution-core/src/hook/mod.rs` and is invoked at `runtime_loop/mod.rs:390-396`, honouring `Deny` and `Ask` (→ HITL). Its contract is explicit at `hook/mod.rs:16-18`: "No rule matching an event => Allow, so hooks are purely additive policy and a malformed or **empty context never locks tools out**." The durable rule store exists as `hook_configs` — matrix §3 line 61 attributes it to capability-core; on disk the table is `apps/Model Plane/rust/services/session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql:162-184` (org_id, tool_name_pattern, enabled, priority), a doc/reality drift worth reconciling separately.

**Consequence:** An operator who registers a `PreToolUse` rule such as `{tools:"shell", decision:"deny"}` through `RegisterHook` gets a success response and sees the rule in `ListHooks`, but execution-core evaluates an empty context and returns Allow, so `shell` runs. A control that reports itself as enabled and enforces nothing is worse than an absent one, because it stops anyone from looking further.

**Fix direction:** Either hydrate the gateway registry from the durable `hook_configs` owner and serialise the matching rules into `hook_context` on every ExecuteStep / add a hook field to `RunAgentRequest`, or delete the gateway's `HookRegistry` RPCs so the only place to configure a hook is the store execution-core actually reads.

### PA-6 [medium] — permission-approval

**Gateway:** The `/v1/browser/runs` entry point decides locally that HITL does not apply to browser-agent runs. `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs:66-76` — `reject_unsupported_browser_approval` returns an error whenever the caller sets `require_approval: true`, and `browser_run.rs:419` dispatches the step with `permission_mode: "auto".to_owned()`, documented at `:288-291` as "deliberately, so the run doesn't pause for HITL approval before the loop's own gates … ever run".

**Already exists:** execution-core classifies `browser_agent` as write-risky and gates it: `apps/Model Plane/rust/services/execution-core/src/permission/mod.rs:138-143` lists it explicitly ("drives a live agentic browser loop (navigate + click + type + form submit) with real side effects on external sites"), so under `ask` it yields `AwaitApproval` at `runtime_loop/mod.rs:403-406`. The same tool is also inline-denied by the gateway's own chat loop (`tool_loop.rs:439`) precisely because it "require[s] governed agentic execution and approval" (`tool_loop.rs:1493`).

**Consequence:** A browser-agent run started through the REST route performs real side effects on external sites (form submits, clicks) with no approval pause and no approval record, while the identical tool invoked from the agentic loop pauses. Which behaviour a user gets depends on which door they came through, not on policy. Partially mitigated: capability-core still evaluates `cap.browser.open` (`capability_policy.rs:338`) before the permission layer, so a high-risk classification there would still force Ask — but that mitigation is invisible from this route's code and is not what the route relies on.

**Fix direction:** Send the run's resolved posture rather than a literal `"auto"` (see PA-3), and build the approval continuation the module honestly says is missing (`browser_run.rs:66-69`) instead of refusing `require_approval` — or, if browser runs are genuinely meant to be un-gateable, encode that as a capability-core decision on `cap.browser.open` so one owner states it once.

### RAG-3 [medium] — retrieval-rag

**Gateway:** `tool_loop.rs:641-706` defines `TIME_SENSITIVE_TOKENS` — a hand-built freshness vocabulary in English *and* Norwegian (`i dag`, `nyeste`, `akkurat nå`, `innbyggertall`, `arbeidsledighet`, …) matched with word-boundary + inflection-suffix logic (`contains_word`, `tool_loop.rs:600-623`), plus a 4-digit-year detector whose floor is derived from the real clock (`tool_loop.rs:712-720`). Its output decides whether a chat turn is forced to search the web.

**Already exists:** Quarry owns query-intent classification behind a pluggable trait. `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/intent_classifier.rs:1-32` documents `RuleClassifier` / `MpIntentClassifier` (Model-Plane-backed) / `HybridClassifier` / `CachedClassifier`. The rule implementation is `smart_router.rs:860-903` `classify_intent` with `FRESH_KEYWORDS` at `smart_router.rs:907-920` and a year window at `smart_router.rs:895`. `QueryIntent::Fresh` is defined as "Contains time-sensitive language. Corpus likely stale." (`smart_router.rs:62-63`) and routes the query away from the possibly-stale local Tantivy corpus (`smart_router.rs:15, 444-470`).

**Consequence:** For the Norwegian query "innbyggertall i Oslo i dag" the gateway correctly forces a search — and then Quarry's `classify_intent` finds no English `FRESH_KEYWORD`, returns `Default`, and serves the stale local corpus first (`smart_router.rs:461` "Widen only if local corpus didn't meet the threshold"). The exact staleness the gateway's heuristic exists to prevent is re-introduced one hop downstream, for the product's primary language. Quarry's year window is also the hardcoded `(2024..=2030)` constant that the gateway explicitly documents as the bug it fixed (`tool_loop.rs:716-720`: "The floor used to be a literal 2024. That was already three years stale…") — the lesson was learned in one service and not the other.

**Fix direction:** Move the freshness vocabulary (or at minimum the Norwegian half and the clock-derived year floor) into Quarry's `RuleClassifier`, and have the gateway pass its verdict through `WebSearchRequest.intent` rather than keeping a private copy. One classifier, one vocabulary, consulted by both the force-a-search decision and the provider routing.

### RAG-4 [medium] — retrieval-rag

**Status (2026-08-11): resolved in source; live retrieval-confidence observation remains open.**

The gateway now preserves Data Plane's low-confidence verdict in the Usage
score: weak retrieval evidence is capped below the UI's confidence threshold
and receives a penalty rather than the normal citation bonus. The focused
confidence suite covers the regression.

**Gateway:** `confidence.rs` scores answer confidence for the `Usage` SSE event. `Evidence.kb_citations` (`confidence.rs:79`) is populated at `sse.rs:1751-1753` as `grounding.citations.len()`, and `score` (`confidence.rs:132-153`) adds `evidence_bonus` (`confidence.rs:111-120`) on top of `BASE = 0.72`. Nothing in `Evidence` (`confidence.rs:76-88`) or `score` reads `grounding.low_confidence`.

**Already exists:** Data Plane computes the authoritative retrieval-confidence verdict: `apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs:1060-1072` sets `low_confidence` from the top candidate's `rerank_score < config.confidence_threshold`, and deliberately declines to judge when no reranker scored (returning `false` rather than fabricating a verdict). It ships on the wire (`proto/retrieval_v2.proto:90`). The gateway already reads it — `retrieval.rs:533` copies it into `Grounding.low_confidence`, and `retrieval.rs:191-195` injects `LOW_CONFIDENCE_GROUNDING_NOTICE` because of it.

**Consequence:** A turn Data Plane flagged low-confidence with five weak citations scores `0.72 + 0.14 = 0.86` and renders as 86% confident, clearing the UI's 0.75 low-confidence threshold that `confidence.rs:127-129` explicitly targets — while the identical turn's prompt carries "The knowledge-base matches above are weak / low-confidence retrieval results, not a confirmed answer." The product tells the model to hedge and tells the user to trust it, on the same answer. This is the same class of dishonesty the surrounding modules were written to eliminate (`retrieval.rs:152-170` honesty-contract note).

**Fix direction:** Add the Data Plane verdict to `Evidence` (e.g. `kb_low_confidence: bool`) and make `evidence_bonus` refuse to credit citations the retrieval owner judged weak — or floor the score below the UI's 0.75 threshold when `grounding.low_confidence` is set. Do not recompute a retrieval-confidence signal locally; consume the plane's.

### RAG-5 [medium] — retrieval-rag

**Gateway:** `retrieval.rs:530` hardcodes `mode: "hybrid".to_owned()` in `build_grounding`, and `retrieval.rs:586` does the same in `graph_only_grounding` — the constructor used at `retrieval.rs:814` **after the retrieval gRPC call has failed** and only graph evidence exists. `Grounding.mode` is a serialized field (`retrieval.rs:120-124`) shipped to the SPA in the grounding SSE event (`sse_events.rs:54`).

**Already exists:** Data Plane genuinely routes per query and knows which mode it used: `apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/types.rs:189-191` `ModeMix::from_weights(weights, hybrid_enabled)` yields sparse-only / dense-only / hybrid (tests at `types.rs:371, 383, 395` pin `zero_dense_routes_sparse_only`, `zero_sparse_routes_dense_only`, `hybrid_disabled_forces_dense_only_even_with_bm25_weight`), and the mode mix is recorded in `retrieval_runs.mode_mix` (`types.rs:95`). Per-org/agent weights that drive it are looked up in `agent_config/mod.rs`.

**Consequence:** A query an org configured for dense-only (or one where BM25 weight is zero, which `ModeMix::from_weights` genuinely routes sparse-only) is reported to the user as hybrid RAG. Worse, when retrieval fails outright and only GraphRAG evidence survives, the grounding payload still claims `mode: "hybrid"` with `fact_count: 0` — the observability surface says a hybrid retrieval ran when no retrieval ran at all, which is precisely the failure mode the trace/`trace_id` plumbing exists to make visible.

**Fix direction:** Populate `RetrieveResponse.retrieval_metadata` in `retrieval-engine-rs` with the resolved `ModeMix` and read it in `build_grounding`; until that lands, emit `mode: None`/`"unknown"` rather than a constant, and give `graph_only_grounding` a distinct mode (`"graph"`) so a failed retrieval is not presented as a successful hybrid one.

### CR-08 [medium] — capability-registry

**Gateway:** `CommandRegistry` (runtime_registries.rs:1515-1530) is a per-(org, name) slash-command map. Its only mutator, `upsert` (:1524-1529), has zero production callers — a repo-wide search finds it referenced only by the struct's own definition and `state.rs:340`'s construction, so the store is permanently empty. `handle_list_commands` (:1537-1551) therefore always returns `[]`, and `handle_execute_command` (:1558-1581) always 404s. Where it does resolve, it explicitly does not execute: "The actual dispatch (tool_name → execution-core, remote_url → HTTP POST) is the bridge's job. Here we just acknowledge." (:1567-1568), returning an output envelope with `error_message: String::new()`.

**Already exists:** capability-core owns commands and actually dispatches them: `internal/commands/handler.go:60-62` registers `/api/v1/commands` and `/api/v1/commands/`, seeded from a built-in catalog (`internal/commands/data.go`), with real dispatch — `/models` to inference-core `ListModels`, `/compact` to session-core `CompactNow`, `/help` (handler.go:183-250) — wired in `cmd/main.go:274`. capability-ownership-matrix.md:59 lists command registries under capability-core.

**Consequence:** A client that discovers slash commands through the Model Plane gRPC surface sees none, while capability-core's HTTP surface serves a working catalog — two answers, one wrong. And the execute contract is unsafe by construction: `ExecuteCommandResponse` with an empty `error_message` is indistinguishable from a real execution, so if the registry were ever seeded, a caller would treat "nothing happened" as success.

**Fix direction:** Delete `CommandRegistry` and relay `ListCommands`/`ExecuteCommand` to capability-core's `/api/v1/commands` and `/api/v1/commands/exec`, which already perform the real dispatch. If the gRPC verbs must remain before that wiring lands, `ExecuteCommand` should return `unimplemented` rather than an empty-error acknowledgement.

### CR-10 [medium] — capability-registry

**Status (2026-08-11): resolved in source.** The gRPC MatchSkills lazy-load now
uses SkillStore::replace_learned, matching the SSE path and pruning deleted or
disabled session-core skills; the existing multi-tenant reconciliation tests
cover the cache semantics.

**Gateway:** The same learned-skill cache refresh is implemented twice, differently, against the same shared state. The SSE chat path uses `replace_learned` (sse.rs:2416-2423), which upserts then prunes learned entries no longer present — skills.rs:97-101 documents exactly why: "Pruning by id is also what makes DELETION take effect — re-upserting alone would leave a removed skill steering answers forever." The gRPC `match_skills` path (grpc.rs:1504-1509) does the naive thing that comment warns against: a bare `for a in … { upsert(...) }` loop followed by `mark_org_loaded`. Both write the same `SkillStore.inner` and both stamp the same `loaded` TTL map (skills.rs:58, TTL default 60s at :37).

**Already exists:** session-core `agent_skills` is the system of record and its `ListAgentSkills` handler already filters `enabled` server-side (`rust/services/session-core/src/grpc.rs:2347-2356`, `WHERE org_id = $1 AND (NOT $2 OR enabled)`); both gateway callers correctly pass `enabled_only: true`. The correct client-side reconciliation also already exists in the gateway — `SkillStore::replace_learned` (skills.rs:102-122).

**Consequence:** An operator deletes or disables a skill in session-core. If the gRPC `MatchSkills` path is the one that performs the refresh when the TTL expires, the deleted skill is not pruned and stays in the match cache — and is re-marked fresh, so on a gateway whose traffic is gRPC-dominant the removed skill keeps being injected into prompts indefinitely. The exact failure `replace_learned` was written to prevent still occurs, just via the other door.

**Fix direction:** Make grpc.rs:1504-1509 call `replace_learned` — the one-line change that gives both paths the same semantics — or better, extract the fetch+reconcile into a single `refresh_learned_skills(state, org, auth)` helper that both callers use, so a third caller cannot reintroduce the divergence.

### BI-4 [medium] — browser-ingestion

**Gateway:** `domains/knowledge/enhanced_fetch.rs:201-262` implements HTML→text extraction in the BFF: `html_to_text` (script/style stripping, tag scanning, block-tag line breaks), `is_block_tag:241` (a hardcoded 16-tag list), and `decode_and_collapse:249` (a 7-entity decoder: `&nbsp; &amp; &lt; &gt; &quot; &#39; &apos;`). The output is capped at `MARKDOWN_CAP = 80_000` (`:20`) and returned in the `markdown` field of the preview envelope (`:165`).

**Already exists:** `apps/master-ownership-matrix.md:86` — `quarry-transform` (Rust) owns "Markdown/html/links/images/attributes/chunks/diff/branding/static outputs". Quarry's markdown is what the healthy path returns and what `build_scrape_preview` reads on the success branch (`domains/knowledge/quarry.rs:169-177`).

**Consequence:** The same page yields two materially different `markdown` bodies depending on which path served it, and nothing downstream can tell which extractor ran. Concretely, the 7-entity decoder leaves the Norwegian entities `&oslash;`, `&aring;`, `&aelig;` undecoded, so a Bright Data-served page renders "H&oslash;yre" where Quarry would render "Høyre" — corrupted text that then gets cached for 24h by BI-2 and shown as source evidence. The 16-tag block list also drops `<td>`, so table cells run together into one line, silently mangling price/quantity tables — which is the product's primary use for `products.rs`.

**Fix direction:** Remove the local extractor entirely; it exists only to serve the BI-1 bypass. If a raw-HTML input ever needs conversion inside CoreSystem, call quarry-transform through a quarry-edge endpoint so one extractor definition governs all markdown.

### BI-5 [medium] — browser-ingestion

**Gateway:** `apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs:1744-1802` — the `browser_agent` chat tool POSTs `{BROWSER_AGENT_URL}/v1/agent/run` (singular) with a body of `{user_id: org_id, objective, target_url, max_steps: 8, enable_web_search: true}` (`:1757-1764`), authenticated by a static `X-API-Key` from `BROWSER_AGENT_API_KEY` (`:1770-1773`), and reads back `content`/`confidence`.

**Already exists:** quarry-edge owns the browser-agent lane: `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs:6-8` — `POST /v1/agent/runs` (plural) acquires a **leased** session, `/step` executes one typed `AgentAction`, `DELETE` releases. Its `StartRunBody` carries `zdr` (`agent_routes.rs:123`) and `AgentConstraints { max_steps, allowed_domains, max_runtime_s, max_cost_usd }` (`quarry-core/src/contracts.rs:139-146`), with a defense-in-depth ZDR/profile guard at `agent_routes.rs:151-157`. Browser grants are `browser-broker`'s (`master-ownership-matrix.md:116`). The correct pattern already exists in the same repo: `model-gateway/src/browser_run.rs:138-202` validates a broker grant and enforces its `allowed_domains` before dispatching.

**Consequence:** Whatever `BROWSER_AGENT_URL` points at drives a browser on the org's behalf with no browser-broker grant, no `allowed_domains` ceiling, and no ZDR bit — so a ZDR chat turn can drive a browser that persists cookies/profile state, and the model can name any `target_url` with nothing checking it against a grant. `org_id` is also sent in the `user_id` field, so any per-user policy on the far side is applied to the wrong subject. Honest scope note: `BROWSER_AGENT_URL` is set nowhere in the repo, so the arm currently returns "browser_agent is not configured" (`tool_loop.rs:1750-1755`, documented inert at `apps/Model Plane/docs/core-research/model-gateway.md:286`) — the exposure is one environment variable away, not live today.

**Fix direction:** Delete the arm, or re-point it at `quarry-edge /v1/agent/runs` using the `quarry_auth::TokenProvider` credential and a `browser-broker` grant, exactly as `browser_run.rs:138-202` already does — passing `zdr` and real `AgentConstraints` instead of a hardcoded step count.

### BI-6 [medium] — browser-ingestion

**Gateway:** Both gateways hand-mirror Quarry's driver-selection contract and hardcode its escalation thresholds. `model-gateway/src/quarry.rs:94-107` redefines `DriverSignals` ("Mirrors `quarry_runtime::driver_plan::DriverSignals`") and `:132-137` `DriverSignals::browser()` hardcodes `prior_block_signals: 2`; the BFF independently hardcodes `"signals": { "prior_block_signals": 1 }` at `domains/knowledge/products.rs:203-207`.

**Already exists:** `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/driver_plan.rs:35-49` `plan_from_signals` is the single planner: `prior_block_signals >= 2` → browser, `== 1` → TLS, else static. `master-ownership-matrix.md:84` assigns the "DriverPlan waterfall" to `quarry-runtime`.

**Consequence:** If Quarry raises the browser threshold to `>= 3`, a `prior_block_signals: 2` falls through the `== 1` arm into `static_fetch` — so model-gateway's one browser escalation silently becomes a plain static refetch. Every JavaScript-rendered page then returns empty text and `fetch_url` reports "got no readable text" (`tool_loop.rs:1712-1723`) with no error, no log, and no failing test anywhere: a capability regression that presents as the model politely telling users the page has no content. The missing `serde(default)` compounds it — adding one field to `DriverSignals` makes quarry-edge reject every signalled request from both gateways at once.

**Fix direction:** Publish `DriverSignals` and named escalation constants from a shared crate (or expose `escalate: true` / `driver: "browser"` as an intent field on `/v1/scrape` so Quarry maps intent→threshold itself), and delete both hardcoded numbers.

### CBU-2 [medium] — cost-billing-usage

**Gateway:** `apps/Model Plane/rust/services/model-gateway/src/finetune_routes.rs:192-202` (`server_estimate_cost_usd`) holds its own model price table inline: `let per_example = if base_model.contains("gpt-4") || base_model.contains("gpt4") { 0.005 } else { 0.0005 };` — a two-bucket, substring-matched USD rate card computed in the gateway process. `effective_budget_estimate` (`:208-219`) feeds it straight into the budget decision.

**Already exists:** cost-core owns the price catalogue as durable, operator-editable data: the `model_pricing` table (`apps/Model Plane/go/services/cost-core/migrations/0002_model_pricing.up.sql`), the resolver `apps/Model Plane/go/services/cost-core/internal/pricing/pricing.go:126-158` (`Cost` / `lookup`), and the public read surface `GET /api/v1/pricing` (`internal/server/server.go:115`). The same gateway already consumes that catalogue correctly elsewhere — `model-gateway/src/pricing.rs:100-133` fetches and caches it, and `pricing.rs:11-13` documents it as "cost-core's price catalogue as the single source of truth".

**Consequence:** Any base model whose name does not literally contain "gpt-4" drops into the 10x-cheaper bucket. A 10,000-example fine-tune of `o4-mini` or `o3` is floored at 10000 x 0.0005 = $5.00 instead of $50.00, so it clears the default $20 `FINETUNE_PER_JOB_BUDGET_USD` cap that a same-sized gpt-4 job would trip. The guard's own stated purpose (`finetune_routes.rs:180-190`: a conservative floor so "a malicious or buggy client cannot bypass the per-job and org caps by claiming a cost of zero") is defeated by simply choosing a non-GPT-4-named base model.

**Fix direction:** Add fine-tune training rates as rows in cost-core's `model_pricing` catalogue (a `training_per_example` / `training_per_million` column alongside input/output), expose them on `GET /api/v1/pricing`, and have the gateway resolve the floor through the existing `PricingCache` in `pricing.rs` instead of an inline `if base_model.contains(...)`. One catalogue, one matcher.

### CBU-4 [medium] — cost-billing-usage

**Gateway:** `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/billing.rs:286-311` (`default_billing_account`) constructs a complete billing account server-side — `plan: "free"`, `subscription_state: "active"`, and a full entitlement + quota matrix (`feature.chat: true`, `feature.api_keys: true`, `feature.audit_logs: true`, `feature.integrations: false`, `feature.sso: false`, `api_calls: 1000`, `users: 5`, `storage_mb: 1000`) — and `billing_account` returns it with `StatusCode::OK` whenever billing-core answers 5xx (`billing.rs:78-86`). The gate is `billing_account_fallback_enabled` (`:282-284`): `cfg!(debug_assertions) || state.allow_dev_actor_headers || state.allow_dev_auth_bypass`.

**Already exists:** billing-core (Control Plane) owns plan, subscription state, entitlements and quotas. The gateway's own happy path proxies them: `GET {billing_core_url}/api/v1/billing/orgs/{org}/account` (`billing.rs:63-66`), `/entitlements/{feature}` (`:101-106`), `/quotas/{metric}` (`:129-134`). `CLAUDE.md` states Control Plane owns "billing, sessions, audit, quotas, and entitlements"; `capability-ownership-matrix.md:52` marks the gateway's role on billing as read-only.

**Consequence:** During a billing-core outage on any debug build or non-prod-APP_ENV deployment, every org is silently told it is on an active `free` plan with chat, API keys, and audit logs granted and a 1000-call quota — a Control Plane decision that Control Plane never made and cannot revoke, delivered as a 200 so the SPA cannot distinguish it from a real answer (only the `metadata.fallback` marker hints at it). An org whose subscription was actually suspended, or whose plan actually lacks `feature.audit_logs`, gets access it is not entitled to for the duration of the outage. This also violates the Application-authority rule in `master-ownership-matrix.md:10-16` ("must fail closed when Control authority is unavailable").

**Fix direction:** Return the upstream 5xx (or a 503 `billing_unavailable`) instead of a synthesized account, so callers fail closed. If a local dev experience without billing-core is genuinely needed, make it an explicit single flag (e.g. `BILLING_CORE_STUB=1`) that is refused whenever `APP_ENV` is anything but `local`/`dev`, and have the stub return a distinguishable envelope rather than a 200 shaped like a real account.

### INF-4 [medium] — inference-providers

**Gateway:** The gateway pins concrete model ids for its own background inference, deliberately bypassing inference-core's routing layer. `sse.rs:3872` — `const TITLE_MODEL: &str = "gpt-4o-mini";` and `sse.rs:3900` — `const FOLLOW_UPS_MODEL: &str = "gpt-4o-mini";` (compile-time, no env override). `http_routes.rs:3444` defaults the dictation cleanup pass to `"gpt-4o-mini"` after `MODEL_GATEWAY_DICTATE_MODEL`. The intent is explicit at `sse.rs:3865-3866`: "A pinned id bypasses the intent layer entirely (`intent::parse_mode` treats only the `verevon-*` names as modes)", and `sse.rs:3869-3870` names the source it is copying: "inference-core's designated cheap fallback (`intent::CHEAP_FALLBACK`)".

**Already exists:** inference-core owns the same value as runtime-tunable policy, not a constant: `inference-core/src/provider/intent.rs:196` — `pub const CHEAP_FALLBACK: &str = "gpt-4o-mini";` feeding `provider/routing_policy.rs:132` — `cheap_fallback: CHEAP_FALLBACK.to_owned()` inside `RoutingPolicy`, which is persisted in session-core and read/written over gRPC by `provider/policy_client.rs:1-10` ("session-core owns the durable store for the Verevon intent layer's runtime policy"). The admin path is already shipped end-to-end: the Frontend Plane BFF proxies it at `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/router_policy.rs:47` (GET) and `:66` (PUT) to inference-core's `/internal/v1/router-policy`.

**Consequence:** An admin retargets `cheap_fallback` through the shipped router-policy admin (deployment retired, cheaper or EU-resident model preferred): every `verevon-*` turn follows, but thread titles, composer follow-up chips and dictation cleanup keep calling the old id and start failing at the provider. Their failure mode is silent by design — the title path degrades to "no title" (`sse.rs:3873-3878`) and the chips "silently never rendered" (`sse.rs:3896-3899`), which is exactly how the previous iteration of this bug survived unnoticed. Pinned ids are also outside the two guards the policy provides: an `Exhausted` org budget forces `cheap_fallback` for tiered calls but does not touch these, and the `tool_fallback_ladder` (`routing_policy.rs:36-49`) is "[o]nly consulted for a model the intent layer resolved from a `verevon-*` mode", so a 429 on that one deployment kills these calls with no sibling to fall to.

**Fix direction:** Ask inference-core for the value instead of copying it: expose the resolved `cheap_fallback` (the policy is already fetchable via `/internal/v1/router-policy`) and use it for title/follow-ups/dictation, or introduce a `verevon-cheap` tier so these calls name an intent and inference-core resolves it. The stated reason for pinning — that the tier could resolve to a reasoning model that burns the 24-token budget on hidden thinking (`sse.rs:3854-3868`) — is a routing-table concern that belongs in `RoutingPolicy`, not a reason for the edge to hold its own model id.

### BFF-6 [medium] — bff-gateway

**Gateway:** Fabricates audit and run identifiers for every executed action. apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs synthesizes `auditId` at 15 call sites — :66, :113, :174, :231, :311, :365, :414, :530, :594, :675, :782, :828, :874, :904, :1645 — e.g. `format!("audit_{}_{}", run_id, user.user_id)` (:66) and `format!("audit_brreg_{}_{}", …)` (:365). It likewise invents `runId` fallbacks: `format!("run_{}", user.user_id)` (dispatchers.rs:62) and `format!("shipping_quotes_{}", user.user_id)` (dispatchers.rs:410), then advertises `"eventStream": format!("/api/v1/knowledge/runs/{}/events", run_id)` (dispatchers.rs:67) against them.

**Already exists:** audit-core is the Control Plane audit system of record and mints its own identifiers — apps/Control Plane/audit-core/internal/store/store.go:283-296 (`ID int64`, `EventID string`), with ingest at internal/api/api.go:112 (`POST /audit`) and read at :111. The gateway already proxies the read side correctly in apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs, which the SPA Trust Center uses.

**Consequence:** The `auditId` the product hands a user for a completed action resolves to nothing: taking it to the Trust Center audit log (`GET /api/v1/audit`, backed by audit-core's own `id`/`event_id`) returns no record, so an action that claims to be audited has no retrievable trail — a compliance surface that reads as evidence but is a string template. The `runId` fallbacks are also not unique per invocation: `shipping_quotes_{user_id}` is byte-identical for every quote that user ever runs and `run_{user_id}` for every recrawl, so the advertised `eventStream` URL points at a run that does not exist and collides across invocations.

**Fix direction:** Return the identifiers the owning service actually minted, or omit the field. For actions that genuinely need an audit record, POST to audit-core `/audit` (already reachable via `state.audit_core_url`, config.rs:38) and surface the id it returns.

### BFF-8 [medium] — bff-gateway

**Gateway:** Computes its own run-quality metric from raw run rows. apps/Frontend Plane/verevonv3/apps/gateway/src/domains/eval.rs:92-97 defines `slice_accuracy` as completed / terminal, with `classify` (eval.rs:71-89) treating `status == "completed"` as success. It fans out over `MAX_THREADS = 25` threads × `MAX_RUNS_PER_THREAD = 50` runs (eval.rs:34-35) belonging to the calling user — `authorized_org_id` is fetched and then discarded at eval.rs:100 (`let _org_id = …`) — and presents the result as an Ops/Quality rollup with an accuracy figure and a recent-vs-prior drift trend.

**Already exists:** session-core computes the canonical run-quality metrics: `build_verification_metrics` at apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs:939-1030 produces `false_success_count`, defined by its own test as counting only completed rows refuted by postcondition (orchestration_grpc.rs:3373-3394), served via `get_verification_metrics` (orchestration_grpc.rs:1641-1676). The gateway already proxies exactly this at apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orchestration.rs:402-425.

**Consequence:** The product can display 100% accuracy on the Ops/Quality tile while the verification-metrics surface reports a nonzero false-success rate over the same runs, because the gateway counts `status == "completed"` as success — precisely the case session-core's postcondition verifier exists to refute. An operator using the tile to decide whether agent runs are trustworthy gets the number already known to be wrong. The figure is additionally computed over one user's most recent 25 threads while being labelled an org rollup, so it changes depending on who is looking at it.

**Fix direction:** Serve session-core's `GetVerificationMetrics` (already proxied in orchestration.rs) as the quality surface, or have session-core add an org-scoped accuracy field to it. Delete the local `classify`/`slice_accuracy` computation rather than maintaining a second definition of a successful run.

### AZI-4 [low] — authz-identity

**Gateway:** The BFF mints its own permission vocabulary from role strings: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/auth/protected.rs:360-369` (`permission_hints`) returns `profile:read`, `profile:update`, `org:read`, `org:members:read`, `org:members:update`, `admin:users:read`, `admin:users:update`. It is served to the SPA as the `permissions` field of `GET /api/v1/me/session-context` (`protected.rs:180` and `:208`). Its role input is `ctx.get("role")` from `resolve_session_context` (`protected.rs:179`), which is the **60-second-cached** user-core context (`apps/Frontend Plane/verevonv3/apps/gateway/src/upstream.rs:37` `SESSION_CONTEXT_TTL_SECS: u64 = 60`, cache read at `upstream.rs:62-81`) — not the live `user.authorized_membership.role` the same request already resolved.

**Already exists:** auth-core owns the canonical scope vocabulary and derives it from the org role: `apps/Control Plane/auth-core/src/auth/plane-token-scopes.ts:1-74` (`MEMBER_SCOPES`/`ADMIN_SCOPES`, `planeScopesForRole`, `modelGatewayScopesForRole`). capability-core owns RBAC evaluation: `apps/Model Plane/go/services/capability-core/internal/policy/engine.go:58-135` (`Engine` with `subjectRoles`/`roleCaps`, `Evaluate`). Neither vocabulary contains any of the seven strings the BFF invents. The matrix assignment is `apps/Model Plane/docs/capability-ownership-matrix.md:60` ("Policy / permissions / RBAC | **capability-core**").

**Consequence:** Today: none observable — nothing consumes it. Latent: the moment any UI gates on `permissions`, it gates on strings no plane can enforce (so a client-side allow has no server-side counterpart), computed from a role that can be up to 60s stale, while the authoritative live role for the identical request is sitting in `user.authorized_membership.role` two lines away. A demoted admin would keep admin affordances for a cache window, and adding an eighth permission would require editing a Rust file instead of auth-core's or capability-core's role map — guaranteeing the three vocabularies drift.

**Fix direction:** Either drop the field (it has no consumer), or, if the SPA needs render hints, derive them from `user.authorized_membership.role` (live, already resolved by `middleware.rs:201-210`) and name the strings from auth-core's `plane-token-scopes.ts` set so the client hint and the server-enforced scope are the same vocabulary.

### RAG-7 [low] — retrieval-rag

**Gateway:** `retrieval.rs:533` computes `low_confidence: resp.low_confidence || facts.is_empty()` — the gateway ORs its own rule onto Data Plane's verdict, where `facts` is the gateway's locally-derived list requiring a non-empty `knowledge_id` AND `document_id` AND text, capped at `FACT_LIMIT = 5` (`retrieval.rs:369-423`).

**Already exists:** `apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs:1060-1072` is the single owner of the low-confidence verdict, and its comment is explicit that a wrong rule here "turned that into a blanket low-confidence verdict on every answer" and that RRF `final_score` "is NOT a substitute" — i.e. the plane has already reasoned carefully about exactly which signal is admissible.

**Consequence:** A response whose candidates carry a `knowledge_id` but an empty `document_id` (or vice-versa) produces zero gateway facts and is declared low-confidence — injecting `LOW_CONFIDENCE_GROUNDING_NOTICE` and telling the model to hedge — even when Data Plane's reranker scored the top candidate above `confidence_threshold`. The user gets an unnecessarily hedged answer for what is really a metadata-shape problem, and the cause is invisible because the notice says "weak / low-confidence retrieval results".

**Fix direction:** Pass `resp.low_confidence` through unmodified. If "the response contained candidates the gateway could not render" is worth surfacing, make it a distinct signal (e.g. `unrenderable_candidates`) with its own honest wording, not a second definition of the plane's confidence verdict.

### BI-7 [low] — browser-ingestion

**Gateway:** `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:3400-3458` `sanitize_action` re-declares the browser action vocabulary as a local `match`: it accepts `navigate, click, press, scroll, select, wait, wait_for, screenshot, back, forward, get_content, click_point, mouse_wheel, type`, explicitly denies `evaluate` (`:3449-3453`), and has no arm for `pdf` — which falls into `_ => "Unsupported browser action."` (`:3454-3457`).

**Already exists:** `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs:88-136` defines `AgentAction` (serde `tag="type"`, snake_case) with 16 variants — the 14 above plus `Evaluate` and **`Pdf`**. It is the wire type quarry-edge deserializes directly: `agent_routes.rs:167-171` `StepBody { action: AgentAction }`, and `quarry-runtime/src/action_runtime.rs:4` states the driver invokes `Navigate`, `Screenshot`, and `Pdf`.

**Consequence:** `pdf` capture is unreachable from the Verevon browser UI even though Quarry implements it and quarry-edge would accept it, and the user sees the misleading "Unsupported browser action" rather than "not exposed here". Every action Quarry adds in future is dead on arrival at the SPA until someone remembers to edit this list, and the failure mode gives no hint that the gap is in the facade rather than the engine.

**Fix direction:** Derive the accepted set from `AgentAction` (shared contract crate or generated types) and express only the *deliberate* subtractions — the `evaluate` denial is a legitimate, documented facade narrowing and should stay, as an explicit deny-list on top of the shared enum rather than a re-typed allow-list.

### CBU-5 [low] — cost-billing-usage

**Gateway:** `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/eval.rs:184-213` sums `input_tokens` / `output_tokens` off session-core run records fanned out across up to `MAX_THREADS = 25` threads x `MAX_RUNS_PER_THREAD = 50` runs (`eval.rs:33-34`), then emits `avgInputTokens` / `avgOutputTokens` in the `/api/v1/eval/quality` response (`:242-243`).

**Already exists:** cost-core is the token accounting system of record and already aggregates the same quantity durably and org-wide: `GET /api/v1/cost/aggregate` returns `total_input_tokens` / `total_output_tokens` / `entry_count` (`go/services/cost-core/internal/server/server.go:113`, `:318-355`), fed by the `mp.v1.usage.*` envelopes the gateway itself publishes (`cmd/main.go:38-40`, `:266-276`). The BFF already reads it in `domains/cost.rs:57-65`.

**Consequence:** The Ops/Quality panel and the Cost panel report token numbers that cannot be reconciled against each other for the same org — different scope (25-thread sample vs org lifetime), different source, and no shared definition of what counts as a token. If a usage envelope is ever dropped (the publish is best-effort — `sse.rs:3352` discards the result with `let _ =`) or a run row is written without token counts, the two diverge further with no signal to the reader about which is authoritative.

**Fix direction:** Drop `avgInputTokens`/`avgOutputTokens` from `/api/v1/eval/quality` and let the SPA read token figures from the existing `/api/v1/cost/summary` route (which already proxies cost-core), or — if a per-run average is genuinely wanted — derive it from cost-core's `total_*_tokens / entry_count` so both panels trace to one ledger. Keep the accuracy/drift computation, which is real edge fan-in.

### BFF-10 [low] — bff-gateway

**Gateway:** Keeps a hand-maintained copy of Quarry's browser action vocabulary. `sanitize_action` (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:3400-3458) allowlists exactly navigate, click, press, scroll, select, wait, wait_for, screenshot, back, forward, get_content, click_point, mouse_wheel and type, denies `evaluate` explicitly (browser.rs:3450-3454), and rejects everything else with `invalid_browser_action: "Unsupported browser action."` (browser.rs:3455-3458). It also re-implements bounds checks — `validate_viewport_coordinate` 0..=10000 (browser.rs:3461-3479) and `validate_wheel_delta` ±5000 (browser.rs:3481-3496).

**Already exists:** The vocabulary is a plane-owned contract: `AgentAction` at apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/contracts.rs:90-137 defines Navigate, Click, ClickPoint, Type, Press, Scroll, MouseWheel, Select, Wait, WaitFor, Screenshot, Pdf, Evaluate, Back, Forward, GetContent — routed by quarry-edge at crates/quarry-edge/src/agent_routes.rs:1350 (`/v1/agent/runs/:run_id/step`). master-ownership-matrix.md:41 assigns browser actions and the ActionRuntime to Quarry.

**Consequence:** `pdf` — a real, implemented Quarry action — is unreachable from the UI: it returns `invalid_browser_action: "Unsupported browser action."`, a fabricated client error for a capability that exists, so PDF capture appears broken rather than merely unexposed. Every future action Quarry adds is silently dead until someone remembers to edit this match arm, and the coordinate/wheel bounds are a second validation that can reject inputs Quarry accepts. The explicit `evaluate` denial is a legitimate narrowing at the edge and is not part of this finding.

**Fix direction:** Forward the action to quarry-edge and let it validate, keeping only the deliberate `evaluate` denial as an edge narrowing. If a typed shape is wanted, generate it from `quarry-core::contracts::AgentAction` rather than restating it.

### BFF-11 [low] — bff-gateway

**Gateway:** Hardcodes a billing entitlement and quota table as a fallback. `default_billing_account` (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/billing.rs:285-311) returns `plan: "free"`, a full `entitlements` map (feature.chat, feature.api_keys, feature.audit_logs true; feature.integrations, feature.sso false) and `quota_limits` (api_calls 1000, users 5, storage_mb 1000) when billing-core cannot be reached. It is gated by `billing_account_fallback_enabled` (billing.rs:281): `cfg!(debug_assertions) || state.allow_dev_actor_headers || state.allow_dev_auth_bypass`.

**Already exists:** billing-core owns entitlements and quotas, and this same file proxies them correctly on the live paths: `billing_entitlement` -> `{billing_core}/api/v1/billing/orgs/{org}/entitlements/{feature}` (billing.rs:102) and `billing_quota` -> `.../quotas/{metric}` (billing.rs:130). CLAUDE.md:24 — "Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, and entitlements."

**Consequence:** In any environment where a dev flag is left enabled, a billing-core outage stops failing closed and instead grants a gateway-authored entitlement set, so feature gating during an incident reflects a table in the BFF rather than what the org actually pays for. The table also drifts silently: when billing-core adds a feature flag or changes a free-tier quota nothing signals that this copy is stale, and the gateway keeps reporting the old numbers.

**Fix direction:** Fail closed on a billing-core outage (return the upstream error) instead of substituting a local plan. If a dev fixture is genuinely needed, load it from a test config rather than embedding billing-core's plan vocabulary in production code.

## Verified real, NOT yet fixed — next up

### SRS-6 [medium] artifact version counters are per-process
`ArtifactVersionStore` is a plain DashMap; `update_artifact` hard-errors
"no artifact '{id}' exists in this conversation" when the counter is missing.
The module doc concedes it ("A deploy resets counters... this is the seam to
swap for session-core persistence"). Not hypothetical: the resume buffer is
Redis-backed precisely because multi-replica is a real posture, so on two
replicas or after any deploy, editing a visible artifact fails. Fix = persist
the counter in session-core, which owns durable thread state.

### SKILL-1 [medium] `tool_restrictions` is a decorative safety control
Surfaced while disproving CR-09, and it is NOT specific to authored skills —
it applies to every skill, learned or operator-written.

A skill row carries `tool_restrictions` ("this skill may only use these
tools"). It is accepted by the API, persisted, updated on upsert
(session-core grpc.rs:2422), and returned on the wire (:2570). No consumer
ever reads it to constrain anything: the only other occurrences in the tree
are empty-vec constructions in fixtures and defaults (model-gateway
skills.rs:413, execution-core runtime_loop/agent.rs:2781). The gateway's
`agent_skill_to_skill` has no field to map it into, so it is dropped at the
boundary.

So an operator can restrict a skill to a safe tool set, see it saved, see it
echoed back — and the model will still call anything. Same failure shape as
the decorative-HITL finding: a control that reports success without acting.

Either enforce it (carry it onto the gateway `Skill`, intersect it with the
tool set when that skill is injected) or stop accepting it — a silently
ignored restriction is worse than an absent one, because it is trusted.

### CR-02 remainder — the stale-cache half, and why the obvious fix was a trap

What is actually still open: `mcp_tool_defs` hydrates from capability-core only
when `capability_bearer` is `Some` (runtime_registries.rs). The `ListMcpTools`
gRPC — execution-core's exposure bridge for the governed agent loop — passes
`None` (grpc.rs:1809-1817). On that path nothing re-reads the catalog, the
`McpRegistry` DashMap still has no TTL, and eviction depends entirely on the
at-most-once core-NATS reconcile consumer. Miss that event and a revoked MCP
server keeps being advertised and proxied to the governed loop indefinitely.

The trap. The obvious fix — hand `ListMcpTools` a service credential so it can
hydrate too — is WRONG, and would have converted a staleness bug into a
cross-tenant one. capability-core scopes `GET /api/v1/mcp` by the verified
principal on the bearer (`mcpVerifiedOrganization`, registry_apis.go:364) and
ignores the `org_id` query parameter hydration sends; the gateway then writes
whatever comes back under the `org_id` it was asked for. With a service
credential those two tenants differ by construction, so tenant A's registry
would be filled with the service principal's tenant — every one of those tools
callable. Nothing detected this, because the gateway did not even deserialize
the rows' `org_id`. It does now and refuses the mismatch, so whichever fix is
chosen next fails loudly instead of silently.

Remaining options, in preference order:
 1. Forward the caller's capability bearer through `ListMcpTools`. Correct
    tenancy by construction, and the only option needing no new credential.
    Costs a proto field (or request metadata) and an execution-core change.
 2. Accept the org-scoped `X-Mcp-Service-Token` on `GET /api/v1/mcp`. The
    mechanism already exists — `HMAC-SHA256(key=MCP_OAUTH_SERVICE_TOKEN,
    message=org_id)`, registry_apis.go:385-398, built precisely so a captured
    header is replayable against one tenant only — but is wired today only to
    the oauth-token sub-routes.
 3. Give `McpRegistry.inner` the TTL its own consumer doc-comment already
    assumes (capability_consumer.rs:93-95). Bounds staleness everywhere but
    does not make revocation prompt, and still needs a credential to
    re-validate. Weakest of the three.

Options 1 and 2 both need a cross-service change, so this is left for a
decision rather than guessed at.

### §23.6 WAS COMMITTED AND INERT — now WIRED (2026-08-11)

Not an audit finding. `cargo check -p model-gateway` warns that
`handle_or_inline_output` and `parse_mcp_tool_name` are never used, and the
warning is telling the truth about a whole shipped feature.

tool_loop.rs has exactly ONE `#[cfg(test)]`, at line 3210, in a 5144-line file.
Every call site of `handle_or_inline_output` is at 3254+ — all inside the test
module. Outside tests the only occurrences are the definition (:2138) and a
doc-comment mention (:2048). `grep -rn "result_handles\." src/` outside
tool_result_handles.rs itself returns NOTHING, so `ResultHandleStore::insert`
has no production caller either.

Consequently no tool result is ever parked under a handle. And `result_query`
— the tool that reads handles — appears in NO ToolDefinition list, so the model
is never told it exists. Both halves are dead, in opposite directions:

  * the write half is implemented but never invoked; large MCP results are
    still plain-truncated at `MAX_TOOL_OUTPUT_CHARS` (8_000) exactly as before
    the module existed
  * the read half is fully wired for dispatch (tool_loop.rs:2043) and for
    artifact events (:255) — but unreachable, because the model cannot call a
    tool it was never offered

This is not documented staging. The module's "Scope, honestly" section
(tool_result_handles.rs:18-27) carefully lists which of §23.6's six
capabilities are implemented and says "what *is* wired is handle-to-artifact
materialization" — which reads as though the four implemented capabilities are
live. Nothing says no handle is ever created.

Wiring is two changes, and both are model-visible, so they are a decision
rather than a cleanup:
 1. call `handle_or_inline_output` where an MCP tool result becomes
    `ToolOutcome.output`, instead of `truncate_chars(..)`
 2. advertise `result_query` in the tool definitions

Same shape as the older "7 Temporal workflows have zero prod callers" finding:
the gap is WIRING, not building. Check for callers before believing a feature
ships.

#### §23.6 wiring — what landed

Both halves are now reachable:
 1. **Handles are created.** `verevon_read_outcome` — the shared outcome path
    for `insights_overview`, `social_list_accounts`, `social_list_posts`,
    `social_list_campaigns` and `knowledge_list_documents`, all of which return
    JSON row sets — now calls `handle_or_inline_output` before the ceiling.
    An oversized queryable result is parked and the model gets a description
    plus a `handle_id`.
 2. **`result_query` is advertised** in `builtin_tool_defs()`. Unconditional is
    safe: the model can only use it with a `handle_id` it was given, and a turn
    that parked nothing (every ZDR turn, every small result) hands out no id.

The advertised schema was machine-checked against the parser: all seven
arguments `parse_handle_query`/the dispatch arm read are documented, and both
operator enums match `FilterOp::parse` and `AggregateOp::parse` exactly — an
advertised operator the parser rejects would be a tool call the model is
invited to make and always loses a turn to.

⚠ A REGRESSION THE EXISTING TEST CAUGHT. `handle_or_inline_output` returns the
payload UNCHANGED — not truncated — whenever it cannot park it (ZDR, already
small, or not queryable JSON). Wiring it in as a straight replacement for
`truncate_chars` therefore removed the `MAX_TOOL_OUTPUT_CHARS` ceiling for
every non-JSON result, and that ceiling is load-bearing: it is the one bound
that holds whatever an upstream returns, on a path where every round re-sends
the whole accumulated history. The fix is to keep the ceiling as an OUTER
bound. It never touches a handle note, which is a small fixed envelope with
`projection_hints` capped at `MAX_PROJECTION_HINTS`.

Note the latent inconsistency this exposed in the committed §23.6 code: the ZDR
test asserts byte-identical passthrough of an oversized payload while its own
comment says ZDR "keeps the pre-existing truncation behavior". The function
does not truncate. Left as-is — the caller now supplies the bound — but the
comment overstates what the function does.

3 tests added (advertised-contract-matches-parser, oversized-result-parks-and-
is-queryable, small-result-stays-inline); the pre-existing truncation test kept
its assertions and only gained the new arguments.

### TEST-1 [RESOLVED 2026-08-11] hanging tests blocked the entire model-gateway suite

`tool_loop::tests::every_advertised_builtin_tool_has_a_dispatch_arm`
(tool_loop.rs:3891) loops `builtin_tool_defs()` and calls `dispatch_tool` for
each against a real `AppState::new()`. The advertised set includes `web_search`,
`fetch_url`, `get_weather`, `knowledge_search` and `code_interpreter`, so it
issues real HTTP and gRPC calls to endpoints that are not configured in a test
process. Those do not fail fast — tonic channels to an unroutable host block —
so the test runs indefinitely.

Impact is the whole crate, not one test: `cargo test -p model-gateway` never
returns. Three separate runs were left hung during this session (one killed at
the harness timeout with exit 144, zero output), and a concurrent session hit
it too. The practical consequence is that nobody has been running the
model-gateway suite, which is how §23.6 shipped inert.

Workaround that works today and gave the first clean signal of the session:

    cargo test -p model-gateway --lib -- \
      --skip every_advertised_builtin_tool_has_a_dispatch_arm

Real fix: the test's INTENT is purely static — "every advertised name has a
dispatch arm and is not refused by the inline gate". It does not need to
execute the tools. Either assert against the arm list without dispatching, or
point the tool endpoints at a stub/loopback address with a short connect
timeout. As written it is an integration test wearing a unit test's clothes.

The specific property it was the only cover for — a newly advertised tool
reaching its arm — is now also asserted directly and in microseconds by
`advertised_result_query_reaches_its_arm_and_is_not_refused_inline`, which is
the pattern the whole test should follow.

#### TEST-1 addendum — it is not one test

After skipping the dispatch-arm test, the suite ran another ~800 tests clean and
then hung again on
`grpc::tests::invoke_uses_an_opaque_scoped_idempotency_key_for_its_managed_start`,
which hangs when run entirely alone.

That one is NOT a network-timeout case — it uses `MockInferenceOk`, so
inference is mocked. Cause not yet identified; it is somewhere in `invoke`'s
session-flow/lifecycle path.

Explicitly ruled out as the cause: this session's removal of the gateway-side
response cache from `invoke` (INF-1). `langcache::GLOBAL` is a `OnceLock` that
NO test ever initialises, so `langcache::global()` returns `None` throughout the
test binary — both the lookup and the store that were removed were already
no-ops under test. The diff is provably neutral on that path, and the test
hangs identically with it applied.

Current usable command, which does give a clean signal:

    cargo test -p model-gateway --lib -- \
      --skip every_advertised_builtin_tool_has_a_dispatch_arm \
      --skip invoke_uses_an_opaque_scoped_idempotency_key_for_its_managed_start

Two independent hangs, one of them not network-related, is the strongest
available evidence that this suite has not been run in a long time — which is
consistent with §23.6 shipping inert and with CBU-1's fine-tune cap never
binding. Worth a dedicated pass to find the second cause and to bound every
test's I/O.

#### TEST-1 result — 804 pass, 0 fail, and FIVE tests that never finish

CORRECTION to an earlier version of this section, which claimed "every other
test in the crate passes". It does not. With the two named tests skipped the
run reports **804 ok, 0 FAILED, 4 ignored** — but the crate has ~809 runnable
tests, and five never print a result line at all:

    grpc::tests::invoke_returns_inference_response_and_emits_events
    grpc::tests::invoke_returns_internal_when_inference_unavailable
    grpc::tests::invoke_stream_forwards_chunks_and_done
    grpc::tests::invoke_stream_returns_internal_when_inference_unavailable
    tools::tests::sleep_caps_at_max

A hanging test prints nothing, so counting `... ok` lines silently undercounts
and reads as success. The four grpc ones hang when run alone as well (>600s).

NOT caused by this session's INF-1 cache removal, and this is proven rather
than argued: `langcache::global()` is `GLOBAL.get_or_init(SemanticCache::from_env)`,
`from_env` returns `None` when no backend is configured (langcache.rs:179), and
none of the backend env vars are set in the test environment. Both the lookup
(which early-returns on `None`) and the two stores (`if let Some(cache) = ...`)
were therefore UNREACHABLE under test. Removing unreachable code cannot change
which tests hang.

Suggestive but unconfirmed: the hanging invoke set is exactly the NON-ZDR
tests, while `zdr_invoke_suppresses_gateway_events_and_durable_session_side_effects`
passes — so the block is somewhere in the durable side-effect path that ZDR
skips. Two hypotheses were checked and neither holds up: managed terminalization
auth is a wiremock stub (`terminal_auth_core_url`, grpc.rs:3663), and session /
inference / managed-run clients are all mocked in
`test_service_with_lifecycle`. Root cause still unidentified — do not assume it
is understood.

So the crate has at least three distinct problems:
 1. `every_advertised_builtin_tool_has_a_dispatch_arm` — real network/gRPC to
    unconfigured endpoints
 2. the five tests above — hang with no result line; cause unknown
 3. the binary never terminates even once every reporting test has reported

#### TEST-1 result — 804 pass, 0 fail, and a third hang in teardown

With both named tests skipped: **804 ok, 0 FAILED, 4 ignored** — every other
test in the crate passes with this session's changes applied. That is the first
clean signal on model-gateway in this session.

The run still does not TERMINATE. All ~809 expected tests print their result
line, then the binary stays alive with no `test result:` summary — so the third
hang is in teardown, not in a test body (a hanging test prints no line at all;
these all printed). Most likely a spawned task or a Drop keeping the tokio
runtime alive after the last assertion.

So the crate has three distinct problems, in increasing subtlety:
 1. `every_advertised_builtin_tool_has_a_dispatch_arm` — real network/gRPC to
    unconfigured endpoints
 2. `invoke_uses_an_opaque_scoped_idempotency_key_for_its_managed_start` —
    hangs alone, inference mocked, cause unidentified
 3. teardown never completes even when every test has reported

None are caused by this session's changes: for (2) and (3) the removed cache
lookup/store were already no-ops under test, since `langcache::GLOBAL` is a
`OnceLock` no test ever initialises.

---

## TEST-1 RESOLVED — root cause found, all three hangs fixed

`cargo test -p model-gateway --lib` now runs to completion: **810 passed,
0 failed, 1 ignored, ~9s, exit 0.** It previously never terminated at all, so
every finding above that says "nobody runs this suite" now has a suite to run.

### Root cause: a HALF-OPEN dependency, not a slow one

Docker Desktop was left mid-shutdown (the machine is being migrated). Its
proxy still held `127.0.0.1:9091` in LISTEN with no container behind it. So:

  * TCP connect SUCCEEDED — no fast `ECONNREFUSED` to fail on
  * nothing ever completed the HTTP/2 handshake
  * `AppState` built every downstream channel with `connect_lazy()` and **no
    `connect_timeout`**, and tonic imposes no default

An unreachable dependency is harmless; a half-open one is not. This is also
why the hangs looked environment-dependent and un-diagnosable from the code:
with services UP the calls answered, and with Docker fully DOWN they would
have been refused instantly. Only the in-between state wedges.

Diagnosis that worked, after reading the code got nowhere: `sample <pid>` on
the hung process showed the runtime parked in `kevent` with no I/O pending,
then a scratch test with per-stage `tokio::time::timeout` isolated it to
`service.invoke` rather than the harness.

### The three fixes

**1. Production — bound connection establishment** (`state.rs`).
`CONNECT_TIMEOUT` (5s) now applies to `lazy_channel` and all 13 static
endpoints. This is a real production defect, not test-only: `fetch_memory_context`
is written to degrade gracefully — it catches an error and continues without
context — but it can never catch a call that DOES NOT RETURN. Unbounded, one
unresponsive dependency blocks every non-ZDR `invoke` forever instead of
costing it some context. Deliberately a CONNECT bound only; a per-request
`timeout` would truncate legitimate streaming RPCs.

**2. Tests — stop reaching out of the process** (`grpc.rs`).
`test_service_with_lifecycle` mocked inference, session and managed-run but
left `memory_client` pointed at the real `localhost:9091` default, so every
non-ZDR invoke test made a live call. Added `MockMemoryService`. This is why
only the non-ZDR tests hung — ZDR skips the memory fetch, which is exactly the
signal that located the bug.

**3. Two tests that were simply wrong about time.**
  * `sleep_caps_at_max` really slept the full 60s cap. Now
    `#[tokio::test(start_paused = true)]` — same assertion, virtual clock.
  * `every_advertised_builtin_tool_has_a_dispatch_arm` dispatched ~33 tools
    sequentially, several of which call downstreams, so it cost one connect
    timeout each. Now bounded per tool and run concurrently: 2s, was minutes.
    Its question is STATIC — does the name reach an arm, is it refused inline —
    and both failure modes return immediately, so a tool still running when
    the bound expires has already answered it.

### Carry forward

`connect_lazy()` with no `connect_timeout` is the reusable lesson, and it is
unlikely to be unique to this service. Any plane whose tests touch a
`localhost` default has the same exposure the next time Docker is left
half-running — which, on a machine being migrated, is the normal state.

### Whole-workspace verification

`cargo test --workspace --lib` — **13 crates, 1484 passed, 0 failed, exit 0.**

Running the workspace rather than the single crate turned up one more break
that `-p model-gateway` could never have shown: `execution-core`'s
`MockSession` implements `SessionCore`, and the durable thread-delete RPCs
added to the session-core proto left it two methods short, so the workspace
did not COMPILE. A proto addition builds fine for its own service and breaks
whoever else implements the trait — worth running the workspace after any
proto change, not just the owning crate's tests.

No other crate hangs. The connect-timeout exposure is real elsewhere in
principle, but nothing else in this workspace currently reaches a localhost
default from a test.
