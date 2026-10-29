# QM-Inspired Improvement Plan (2026-08-13)

Last reconciled: 2026-08-17
QM clone baseline: `d719f54075afee4648be75240fa02adb3a9071f0`
CoreSystem baseline: `520a7a6b410a79b4b368a33b949abea1a0da4e37`

## What this is

A full comparison of [yc-software/qm](https://github.com/yc-software/qm) ("a
multiplayer agent harness for work," 13.3k stars / 1557 forks two weeks after
launch) against Verevon v3 + CoreSystem, across the 8 subsystems where the two
projects genuinely overlap. Produced by an 8-topic, 24-agent research pass
(3.7M tokens, 743 tool calls, ~17 minutes) — one agent explored the QM side of
a topic, a second verified CoreSystem's *current* code against it (not memory,
which is stale in several places this pass corrected), a third synthesized a
gap assessment and recommendations. This document is the human synthesis of
those 24 results, not a pass-through — every claim below was either re-checked
against the raw findings or, in one case, caught as a synthesis error and
dropped (see Methodology notes at the end).

**Read the headline finding first — it matters more than any single topic.**

## Headline finding: the gap is rarely "we don't have this," it's "we built it and never finished wiring it in"

Independently, across 5 of the 8 topics, the research surfaced the exact same
failure shape that has recurred all session (§23.6 result handles, the
fine-tune cost cap, the DNS-pinning connect timeout): a real, tested,
production-grade implementation exists, and something in the last mile —a
caller, a UI field, an admin route — was never added.

- **`PromoteSkill`** (capability-core) unconditionally returns
  `FailedPrecondition` in every real deployment, because `main.go` always
  wires a durable store and the RPC only supports the no-store path. This
  strands `orchestrator-core`'s `SkillPromotionWorkflow`, which has real
  callers waiting on a dead RPC.
- **6 of orchestrator-core's 8 registered Temporal workflows** have zero
  production callers (`ScheduledRunSupervision` joined the allowlist after
  this doc's original count of 7 — see `workflowreg.go`). Two now have real
  callers: `InteractiveRunSupervision` — cron sweeper → task executor →
  `WorkflowDispatcher` → `StartWorkflow` gRPC → real `Temporal.ExecuteWorkflow`
  — and `ScheduledRunSupervision`, which Capability Core's cron routing now
  dispatches by name (`workflow_dispatcher.go`'s `ScheduledRunWorkflowType`
  constant) through the same `StartWorkflow` path. The frontend cron
  **creation form still has no field to pick a workflow type**, so even a
  fully configured deployment is limited to whichever default the dispatch
  path selects; see "space + qm style improvements.md" P1 item 4 for the
  remaining gap in ordinary user-bound `ExecuteStep` reaching a
  service-owned scheduled run.
- **`inference-core`'s `ContentSafety` classifier** — a complete, LLM-based
  moderation operation, merged into the gRPC contract — has zero callers
  anywhere outside its own tests.
- **capability-core's resource-scoped memory** (`run`/`thread`/`workspace`/
  `session`) is declared, validated, and referenced throughout
  `workplane_apis.go` — and hard-blocked on both the write path (503,
  "requires Session Core authorization") and the read path (501, "not
  implemented"), referencing a Session Core capability that doesn't exist in
  Session Core's actual contract.
- **org-core's settable spend/token-quota API** (`GET`/`PUT
  /organizations/:id/quotas`) is fully built — migration, repository,
  service, routes — and **is** a live caller of `model-gateway`'s cost-budget
  check (`org_quota.rs`, built this session). What's missing is the last
  mile again: no admin route, no frontend form, so an operator cannot
  actually *set* a limit today even though the enforcement path is real.

None of these need new architecture. Every one of them is a connection, not a
feature. They're the cheapest, highest-confidence items in the prioritized
backlog below for exactly that reason.

## Corrections to earlier findings (this session and prior)

Two things this pass found that update statements made earlier — stated
plainly rather than left for you to notice the discrepancy:

1. **Data Plane v2's `data-orchestrator-go` does not use Temporal at all.**
   It runs its own hand-rolled Postgres lease/claim job queue
   (`internal/jobs/worker.go`). The 2026-07-30 memory note "7 Temporal
   workflows have zero prod callers" conflated this with Model Plane's
   `orchestrator-core` — the two are unrelated durable-work systems in
   different planes. That memory note is now split: DPv2's queue was never a
   Temporal home, and orchestrator-core is now 6-of-7-uncalled, not 7-of-7.
2. **org-core's quota API is a live, wired enforcement path**, confirmed
   independently in this pass (`model-gateway/src/org_quota.rs` calls
   `GET /organizations/:id/quotas` and enforces the ceiling in the cost budget
   check). This does not contradict the earlier note in this session that the
   gateway's *service credential* for that call still needs registering in
   `ORG_CORE_SERVICE_CREDENTIALS` for a real deployment — that's a separate,
   still-open operational step, not a code gap. The two claims are about
   different things and both stand.

## Scorecard

## 2026-08-17 baseline reconciliation

This plan is now aligned with both repositories' current checked-out commits,
not the older `6c951c6e` CoreSystem snapshot used by the original research
pass.

### What changed in QM since the comparison pass

The current QM commit `d719f540` fixes command-form approval compilation and
keeps the deployment-layer evaluator in sync. A command-derived approval now
matches the binary plus the command at a whitespace or end boundary, with
regression coverage for both the CLI and deployment layer. This is a real
correctness improvement, but it does not alter the larger comparison: QM still
has the coherent `scopeId` resolver, durable monitor/delivery loop, scoped
workspace/memory, adapter boundary, and deployment-directory evidence; its
documented single-organization, fail-open, credential, and audit limitations
still must not be copied into CoreSystem.

### What changed in CoreSystem since the original comparison snapshot

- V3 UI-4 now has a committed narrow slice for **Definitions + Space
  installations**. The gateway composes the caller's Control Space index with
  the existing Control-joined per-Space agent read, groups by `agent_ref`, and
  exposes `/api/v1/agents/installations`; V3 renders it at
  `/agents/installations` with tests for multi-Space grouping, empty state, and
  read failure.
- The slice was live-verified read-only in the authenticated AQUATIQ AS
  organization. Unconfirmed Control bindings are omitted, which preserves the
  same executable-installation rule as the room Agent tab. This is a safe
  projection improvement, not a durable cross-Space registry, a new authority,
  or proof that a Model agent can execute the definition.
- The full Agent Studio vision remains intentionally open: blueprint
  activation is still a showcase, Page/system installations have no storage
  contract, Runs/receipts remain in the separate Task Console, and Chief/Core
  cross-Space routing needs an owner-approved registry read.
- The source lanes for capability health, scheduled-step authority,
  `tickets.create` reservation/continuation, deletion, and Application
  delivery are stronger than the original comparison, but they remain
  source/disposable evidence. Auth Core registrations, deployed service
  bindings, provider/ZDR, HA replay, owner-effect observation, immutable
  candidate/rollback, and approved promotion thresholds remain release gates.

### Reclassified status

The following labels should be used in future updates:

| Area | Current classification | Do not claim yet |
|---|---|---|
| Space cockpit | **Source progress** — Personal Space cockpit and Agent installations projection exist | QM's one-view scope computer/resources experience |
| Action catalog | **Source progress** — typed registry drift checks and actor-specific view plumbing exist | Same executable action set for every actor or live owner-effect parity |
| Scheduled work | **Source/disposable green, runtime gated** | A deployed scheduled effect or provider completion |
| Approval continuation | **Source contract, release open** | Restart-safe live continuation of a governed owner action |
| Delivery | **Source state machine, release open** | Exactly-once delivery; the target is at-least-once plus idempotent projection and explicit `unknown` |
| Capability promotion | **Source-only** | Candidate/staging/rollback promotion |

The original 8-topic findings and historical checkpoints below remain useful;
this reconciliation supersedes only their stale baseline and status wording.

| # | Topic | Verdict | One line |
|---|---|---|---|
| 1 | [Command/tool policy floor](#1-commandtool-execution-policy-floor) | Partial gap | CoreSystem's HITL/evidence/posture machinery is stronger than QM's in several ways; one undisclosed self-service floor-erasure gap on the highest-risk capability |
| 2 | [Injection screening](#2-content-security-screening-prompt-injection) | Partial gap | One real scanner, one call site (RAG only) — the largest untrusted-content surface (web fetch, browser scrape, MCP) has zero screening |
| 3 | [Egress / SSRF](#3-network-egress-control) | Partial gap | Quarry-v2 is at or above QM's rigor; one Go service (integration-corev2) has *zero* protection on outbound calls it owns |
| 4 | [Harness abstraction](#4-swappable-agent-loop-harness) | **Clear gap** | The turn-loop is hand-written three times with drifting behavior instead of once behind a boundary |
| 5 | [Skill ownership & sharing](#5-skillcapability-ownership--sharing) | **Clear gap** | No personal skill scope, no sharing, dead promotion RPC — but CoreSystem's own `mcp_servers` code already solves this pattern for a sibling entity |
| 6 | [Scoped memory](#6-per-scope-memory--durable-session-state) | **Clear gap** | Strictly single-owner-per-thread by schema; no room/channel-shared notebook concept exists — may be a deliberate boundary, treat as additive |
| 7 | [Automation & collaboration](#7-background-automation--proactive-collaboration) | Partial gap | Cron is at parity with QM; no watch/notify mechanism and no delivery-queue equivalent exist at all |
| 8 | [Deployment, admin, secrets](#8-deployment-admin-config--credential-keychain) | Partial gap | Real 15-section admin UI, and it's honest about its own fakes — but two flat credential vaults instead of one keychain, and org allow-listed models don't exist |

---

## 1. Command/tool execution policy floor

**QM.** Five hardcoded org-floor rules (`rm -rf`, force-push, `DROP/TRUNCATE
TABLE`, fork-bomb, pipe-to-shell) that a narrower scope's rules can never
outrank, because `composePolicy` puts them first in the array and
`firstMatch` returns on the first regex hit — no priority field, the *order*
is the policy. ~700 lines of shell canonicalization
(`scannableCommand`/`scanShell`) unwrap quoting, heredocs, `$()`/backticks,
ANSI-C escapes, and pipe-to-shell recursively (depth-capped at 8) before any
rule ever sees the text, so evasion has to beat one shared preprocessor, not
five independent regexes.

QM's own `SECURITY.md` admits the floor is bypassable in practice: an org
admin can `PUT` an empty command-policy at org scope through a generic admin
setter with no floor-reinjection, permanently erasing the five rules for that
org. Invalid stored regexes fail *open* (skipped, scan continues) in denylist
mode.

**CoreSystem.** No regex denylist exists anywhere — command governance is
risk-tier dispatch, not pattern matching. `trusted_capability_id()`
(`execution-core/capability_policy.rs:538-566`) binds a fixed set of tool
names to capability IDs and fails **closed** on anything unrecognized (`None`
→ `permission_denied`) — stronger than QM's fail-through-to-allow. Verified
strengths beyond that: `cap.command.shell`'s `RiskLevel=High` is a compile-time
constant separate from the sandboxed code-interpreter's lower tier; decision
evidence is Ed25519-signed and durably verified before an `Allow` is trusted
(`decision_proof.go` + `DecisionEvidenceVerifier`); capability availability is
gated by a live, fail-closed health attestation with an anti-fabrication test.
QM has no equivalent to any of the last two.

**The gap, verified independently, live:** `capabilities.go`'s upsert handler
(lines 103-130) validates only that `risk_level ∈ {low, medium, high}` before
`capabilities_store.go`'s `Upsert` runs `ON CONFLICT (org_id,kind,name) DO
UPDATE SET risk_level = EXCLUDED.risk_level` — and `authz.go`'s
`AuthorizeHTTP` (lines 72-74) gates the whole write path on holding the plain
`capability:write` scope, no separate admin check. Any write-scoped caller for
an org can set `cap.command.shell`'s `risk_level` to `low`, flipping
`policy/engine.go`'s `High → Ask` to `Low → Allow` — silently disabling HITL
for shell execution, for that org, going forward. Same shape as QM's own
documented bug. Confirmed per-org self-inflicted, not cross-tenant.

**Recommendations**

| ID | Effort | Impact | What |
|---|---|---|---|
| POL-1 | M | High | Add a floor to capability-core's write path: `cap.command.shell` (and any future `RiskLevel=High` seed) should require a separate, harder-to-obtain scope to *lower*, not just `capability:write` to touch at all. Do not route the fix through a generic setter the way QM's own `admin-resources.ts` does — that's the exact shape of QM's bug. |
| POL-2 | S | Medium | Close `registry.Get(id, "")`'s org-blind, last-write-wins in-memory index (`registry.go:280`, fed by `main.go:107`'s `orgID=''` load) — not currently exploitable (no confirmed live caller wires it into `EvaluatePolicy`), but a landmine for whoever wires it next. |
| POL-3 | S | Medium | Disclose the risk-downgrade limitation in capability-core's docs the way QM's `SECURITY.md` discloses its own bypassable floor — an honest limitation is better than a silent one. |
| POL-4 | M | Low | Consider a narrow, explicitly-non-boundary content check on capability auto-allow paths as defense-in-depth, on QM's `ORG_FLOOR_RULES` model — capability-core never inspects command/argument text at all today. |

---

## 2. Content security screening (prompt injection)

**QM.** *No verified findings this pass* — the exploration agent for this
side returned an empty placeholder. Not represented here; see Methodology.

**CoreSystem.** One real, wired scanner: `scan_injection()`
(`model-gateway/src/moderation.rs:161`), a case-insensitive match against 16
hardcoded English phrases. Its **one call site in the entire codebase** is
`retrieval.rs::add_context_entry` — RAG/knowledge-base snippets only. It
doesn't block or strip anything; a hit appends one shared warning to the
prompt, in-band, and the model has to honor it itself. It's unconditionally
on with no policy gate and, having no external dependency, no failure mode at
all.

**The gap:** the tool-result path that carries the largest actual
untrusted-content surface — `execute_web_fetch`, `execute_web_search`,
`execute_mcp` in `execution-core/runtime_loop/mod.rs`, and browser-scraped
text via `quarry_agent.rs::observation_from_wire` — has **zero** screening.
`ToolOutcome` (`tool_loop.rs:125-130`) has exactly four fields (`call_id`,
`name`, `output`, `error`) — no source/trust field, so provenance can't even
be *represented*, let alone rendered, in what the model sees.
`append_tool_outcomes` flattens internal APIs, live web fetches,
browser-scraped text, and third-party MCP output into one undifferentiated
block. Quarry's own text extraction (`observation.rs`) is a bare
`strip_tags()` capped at 500 chars — no entity-decoding safety, no
hidden-text handling.

Two things exist but aren't this: `pii_redaction_required()`
(`moderation.rs:101-135`) *is* a real, working fail-closed contract for a
sibling concern (redacts on missing bearer, unreachable capability-core, or
malformed response) — the template to build the injection gate on.
`conversation-core`'s `AllowAIProposal(ctx, orgID)` (this session's work) has
no content parameter at all — it's an entitlement gate, not a classifier.

**Recommendations**

| ID | Effort | Impact | What |
|---|---|---|---|
| INJ-1 | M | High | Add a provenance field to `ToolOutcome`, populated at each `execution-core` call site, threaded through to what the model actually reads. This is the prerequisite for everything else — right now the model cannot tell a scraped webpage from its own org's internal API. |
| INJ-2 | M | High | Give `scan_injection` a real fail-closed policy contract on the `pii_redaction_required` template, and extend its one call site to cover `web_fetch`/`web_search`/browser-scrape/MCP output, not just RAG. |
| INJ-3 | M | Medium | Wire `inference-core`'s already-built `ContentSafety`/`content_safety_json` operation as a semantic second pass — it exists, is merged, has zero callers. Needs an async/sampling design, not a blocking call inline in the tool loop. |
| INJ-4 | S | Low | Emit an audit event when injection markers fire or unscreened content is let through — today a hit only appends an in-band string; nothing is logged or surfaced as a security event. |

---

## 3. Network egress control

**QM.** Two cooperating pieces: pure host-matching config
(`egress-policy.ts`), and a standalone decision service
(`egress-authz-main.ts`) any proxy calls per request — it does the DNS lookup
itself, SSRF-checks the *resolved* IP (link-local, AWS IMDSv6
`fd00:ec2::254`, opt-in private-range blocking with per-host exceptions), and
hands the vetted IP back in a response header for the proxy's own routing
config to pin to (`ORIGINAL_DST` + `x-egress-upstream-address` in the
reference Envoy config — deliberately *not* `dynamic_forward_proxy`, which
re-resolves per connection). The pin is enforced by the proxy's own
config/contract, not by the wire protocol — QM's own docs flag that a
differently-wired caller could silently lose it.

**CoreSystem.** No centralized service; three independent implementations at
uneven rigor. **Quarry-v2** (`dns_guard.rs`/`fetch.rs`/`driver.rs`) is at or
above QM's rigor — it resolves DNS itself, checks resolved IPs, and pins the
connection **in-process** (`TlsDnsPin`), which never crosses a process/header
boundary the way QM's proxy handoff does. **verevonv3 gateway**
(`public_url.rs`) is much lighter — string/blocklist only, never resolves DNS
— and is safe today only because every call site forwards to Quarry-v2 for
the actual fetch instead of dialing directly (confirmed across `quarry.rs`,
`monitoring.rs`, `browser.rs`). **integration-corev2** — the Go service
CLAUDE.md names as owner of provider-action outbound HTTP — has **no general
guard at all**; the only related code is a single-provider same-origin check
for Meta webhook assets. Go's `net/http` has no DNS-pinning primitive by
default, and none was added.

**Recommendations**

| ID | Effort | Impact | What |
|---|---|---|---|
| SSRF-1 | M | High | **DONE 2026-08-13, closed further 2026-08-18** — see "Landed" below; the one residual gap (Microsoft OAuth client bypassing the shared guard) is fixed, plus a NAT64 well-known-prefix blind spot in the guard's own range list. |
| SSRF-2 | M | High | **DONE 2026-08-13** — resolved as a documented contract, NOT a shared crate; see correction below. |
| SSRF-3 | L | Medium | Evaluate a QM-style centralized decision service for polyglot reuse instead of N independent guards — real leverage for whichever Go/TS service comes next, since Quarry's Rust-native pinning doesn't port. |
| SSRF-4 | S | Low | Audit model-gateway's outbound calls for any caller-influenced host — it has a `CONNECT_TIMEOUT` (this session) but no SSRF guard, currently acceptable only if it never dials outward on caller/model input. |

---

## 4. Swappable agent-loop / harness

**QM.** One interface (`Harness` = `{profile, turns, models, tools}` in
`harness.ts`) implemented five ways — Pi in-process, Claude Code via
in-process MCP, Codex over a spawned JSON-RPC subprocess, OpenCode over a
spawned HTTP server + plugin, and a Mock used across 11+ test files with zero
model calls. All four real backends share **one** tool-definition factory
(`createPiTools` in `pi-tools.ts`) — names, schemas, wording, truncation
limits, and security-screening banners live once, so a prompt or guardrail
change propagates to every backend automatically. `HarnessTurnInput` hands a
backend plain callbacks (`emit`, `screenExternalContent`,
`toolApprovalGate`...), not a live core handle — a backend structurally
cannot reach state it wasn't given for that turn. No ADRs are committed for
this design; it was reconstructed from code.

**CoreSystem.** The turn loop is a hand-written Rust implementation, and
worse — three independently hand-written copies, not one, with drifting
behavior. `execution-core::run_rounds` (managed/durable RunAgent — sequential
per-call dispatch), `model-gateway::run_tool_rounds` (inline chat/SSE —
concurrent per-round dispatch via `join_all`, its own compaction, its own
dedup set), and `deep_research.rs`'s driver, whose own comment says it
mirrors `run_tool_rounds`. A repo-wide grep for `trait Harness`/`AgentLoop`/
`Runtime`/`TurnController` returns nothing outside an unrelated SLO-benchmark
harness. `inference-core`'s `routing_policy.rs`/`intent.rs` (VerevonMode ×
Complexity × BudgetPosture → model) is a real, genuinely pluggable axis — but
it answers a different question (which LLM answers *one* call) than QM's
Harness boundary (what turn-execution semantics wrap the call). Don't
conflate the two in future roadmap language.

Correction to the 2026-07-30 memory note "capability-gated tools inert:
nobody calls the attestation API": no longer true for the
`execute_step_inner` dispatch path — `GrpcCapabilityPolicy::evaluate_with_evidence`
is live-wired at server startup (`grpc.rs:696`) with real gRPC calls and
Ed25519 evidence verification.

**Recommendations**

| ID | Effort | Impact | What |
|---|---|---|---|
| HARN-1 | M | High | Extract one canonical tool-execution module before touching the loop — collapse `execute_step_inner` and `dispatch_tool` into one crate owning naming, schemas, capability gating, hooks, permission checks, and result formatting. This is QM's `primitives.ts`/`pi-tools.ts` split, and it's the prerequisite for #2. |
| HARN-2 | L | High | Define a Rust turn-loop trait (`run_turn(ctx, goal, tools, budget) -> TurnOutcome`) and refactor the three existing loops into three named strategies behind it, instead of three drifting functions. Do this *after* HARN-1. |
| HARN-3 | S | Medium | Record an ADR for the split (or the decision not to split) — neither repo has one today; write it alongside HARN-2 so the next surface (a batch/offline runner) has a recorded rationale instead of a fourth hand-copy. |

---

## 5. Skill/capability ownership & sharing

**QM.** Three real, wired layers. **Ownership**: a `Skill` is a signed,
versioned record scoped to `personal:<id>`/`channel:<ref>`/`group:<ref>`/
`team:<ref>`/`org:<id>`, with a `draft → reviewed → published → archived`
lifecycle; personal creation is direct, org/team never is. **Sharing**:
`shareArtifact` routes to a plain ACL grant (`AclStore.grant`, gated by
`canManage` — owner-only, or a manager for channel/group, explicitly *no
transitive re-share*). **Promotion**: `SkillStore.promote()` requires
`org_admin` **and** a `liveActor === true` flag — a distinct axis from
role, so an autonomous cron/trigger can never promote a skill org-wide even
if it could otherwise impersonate an admin's principal. `review()` and
`promote()` both re-verify the signature before acting, defending against a
tampered durable record. On top of all this: a fully built git-repo skill-pack
importer with path-collision detection before anything materializes, and a
DNS-pinned, redirect-and-proxy-disabled git fetch (resolves the host itself,
rejects private addresses, pins git's HTTP layer to the resolved IP,
`followRedirects=false`) — closing the SSRF-via-redirect-after-check gap a
naive "validate then let git resolve again" approach would leave open.

**CoreSystem.** `agent_skills` (capability-core, fronted by model-gateway's
`/v1/skills`, surfaced in Verevon v3's `SkillsSection.tsx`) is scoped by
`org_id` alone — no owner column, no `ScopeId`, no lifecycle beyond an
`enabled` boolean. The UI's own comment states plainly: org-wide,
admin-only. `capability-core`'s `PromoteSkill` gRPC handler unconditionally
returns `FailedPrecondition` in every real deployment (`main.go` always wires
a durable store; the RPC only has a no-store path) — dead in production, and
it strands `orchestrator-core`'s real `SkillPromotionWorkflow`. `git`-imported
skill packs don't exist at all — capability-core's own roadmap tracker
already honestly marks this unbuilt.

**The pattern to steal is already in the codebase.** `capability-core`'s
handling of `mcp_servers` — a *different* resource — already implements
personal-scope + owner + explicit-share (`scope ∈ {'user','org'}`, an
`OwnerUserID` and `SharedWith` list, owner-only-may-share enforcement). This
is QM's shape, already built, just not applied to `agent_skills`.

**Recommendations**

| ID | Effort | Impact | What |
|---|---|---|---|
| SKILL-1 | M | High | Extend the existing `mcp_servers` ownership/sharing pattern to `agent_skills` instead of building a new ACL layer — the personal + owner + share model is already solved once in this codebase. |
| SKILL-2 | M | High | Fix or remove the dead `PromoteSkill` RPC before building any admin-gated promotion flow on top of it — `orchestrator-core`'s `SkillPromotionWorkflow` already has real callers waiting. |
| SKILL-3 | S | Medium | If SKILL-1 lands, check whether `capability_registry`'s taxonomy entry (currently `team_shared` in `resource_taxonomy.json`, mirrored in `retrieval-engine-rs`) needs reclassifying so per-user grants aren't hard-rejected — currently unused as a resource type, so not a live bug, just a landmine to check. |
| SKILL-4 | L | Low | Treat git-imported skill packs as an explicit, separately-scoped item — large, security-sensitive (SSRF-hardened fetch, path-collision detection, sync engine), and capability-core's own roadmap already marks it honestly as unbuilt. Don't let it ride in as a byproduct of SKILL-1/2. |

---

## 6. Per-scope memory & durable session state

**QM.** Every memory notebook keys off one opaque `ScopeId` (`kind:ref`,
`kind ∈ personal/channel/team/org/group`). `scopeFor()` computes exactly one
writable scope per turn — a room/group turn writes to the room's own
notebook, **never** to any member's personal one; a room's recall default
(`recallMemoryScopes`) is `[room scope, org scope]`, never a member's
personal scope. The one bridge is explicit and one-directional:
`ccCaptureToPersonal()` mirrors facts spoken in a room into the *speaking*
actor's own personal notebook, tagged `(said in <room>)` for provenance.
Postgres storage is append-only per scope with `pg_advisory_xact_lock`
serialization — history and restore fall out of the same log for free.

**CoreSystem.** Strictly single-user at the layer that actually feeds chat.
`threads` (session-core) has exactly one `user_id NOT NULL` column — no
participants table, no array. `authorize_thread` resolves that single owner
and rejects any other caller. A migration comment states the intent
explicitly: *"user-owned durable state must never collide or read across
users that happen to share one organization."* **This may be a deliberate
security boundary, not an oversight** — treat it that way. A second, separate
system exists in capability-core (`agent_memory`, scopes `run/thread/
workspace/session/user/org/global`) with genuinely cross-user `org`/`global`
tiers — but that's all-or-nothing tenant-wide broadcast, not bounded room
membership, and its finer scopes (`run`/`thread`/`workspace`/`session`) are
hard-blocked on both read (501) and write (503) referencing a Session Core
capability that doesn't exist. The two memory systems never talk to each
other — `model-gateway`'s live chat path only calls session-core's.

**Recommendations**

| ID | Effort | Impact | What |
|---|---|---|---|
| MEM-1 | L | High | Add a shared-scope memory tier to session-core (nullable `room_ref`/`scope_kind` alongside the existing `user_id NOT NULL`), extending `authorize_thread` to accept "any authorized member of this room" as a second valid caller shape — additive, not a replacement for the single-owner default. |
| MEM-2 | M | Medium | Finish or formally kill capability-core's resource-scoped memory instead of leaving it half-wired — right now it declares scopes it 501s and 503s against a Session Core contract that was never built. |
| MEM-3 | S | Medium | If MEM-1 ships, add an explicit, visible capture-to-personal mirror (QM's `ccCaptureToPersonal`) rather than a silent one — CLAUDE.md's ZDR-propagation rule means any new cross-boundary content flow needs the same provenance discipline QM applies. |

---

## 7. Background automation & proactive collaboration

**QM.** Two user-configurable mechanisms and one purely internal. Crons
(`src/cron`) and job watches/monitors (`src/monitors`) are first-class agent
tools *and* have a dedicated REST API and web-UI screen — a user can set one
up just by asking in chat, or from the UI directly. Every out-of-turn message
(a cron fire, a monitor fire, a live turn's proactive `reach`) goes through
one durable, idempotency-keyed delivery queue (`reachEnqueue` → a separate
async poller per surface with retry) rather than posting directly from the
code path that decided to send it — decoupling "what to say" from "when/how
it's actually delivered." `runTrigger()` is the one shared execution path for
every non-live-chat wake (authz, consent, notice composition), so a new
background-work type is a spec plus one call, not a reimplementation.
Monitor patterns are literal-alternative-only (no regex metacharacters) to
avoid ReDoS from a user-supplied watch pattern in a long-lived poller.

**CoreSystem.** Cron is at genuine parity, arguably ahead: capability-core's
real `/api/v1/cron` REST API is fronted by a real, mounted UI screen
(`CronSchedulesSection.tsx`), and the full pipeline — sweeper → task row →
executor claim → `WorkflowDispatcher` → `orchestrator-core`'s `StartWorkflow`
gRPC → real `Temporal.ExecuteWorkflow` — is live, not a stub, for
`InteractiveRunSupervision`. But: **no watch/monitor mechanism exists at
all** — nothing lets a user say "notify me when this finishes"; task/run
completion produces only a DB status update, never a message. **No
delivery-queue equivalent** — every confirmed outbound send is either a live
turn's own response or a direct call, with no durable queue and no async
channel poller. `notification-core` — the plane's dedicated notification
service — has **zero consumers wired to task/run lifecycle events**, so a
cron-fired run's outcome is invisible to it entirely. The entire scheduled
pipeline is silently config-gated: if the Temporal address/credential is
unset, capability-core falls back to a `NatsDispatcher` whose own source
comment documents that it strands every claimed task in `running` forever.

**Recommendations**

| ID | Effort | Impact | What |
|---|---|---|---|
| AUTO-1 | S | High | Add a `workflow_type` field to `CronSchedulesSection.tsx`'s creation form — `dispatchPlan` already reads `task_template.workflow_type` and only falls back to the default when it's empty; the frontend already threads `task_template` through as opaque JSON. Nearly free, unlocks 6 already-built workflows. |
| AUTO-2 | M | High | Build a background-job "notify me" path reusing the already-durable run-event stream (`mp.v1.run.*.event` is JetStream-retained with existing consumers) — the trigger source already exists; QM's `monitor-poller.ts` → `runTrigger` → `reachEnqueue` is the shape to follow. |
| AUTO-3 | S | Medium | Close the `NatsDispatcher` silent-degradation gap before building notifications on top of it — today an unconfigured Temporal credential means tasks strand in `running` forever with no signal. |
| AUTO-4 | M | Medium | Decide whether org-wide sharing is sufficient or a QM-style bounded Project scope (`project-store.ts`: explicit owner + member list, narrower than the whole org) is actually needed — CoreSystem's only sharing granularity today is full org membership. |

---

## 8. Deployment, admin config & credential ("keychain") management

**QM.** Three layers. `qm init` scaffolds a versioned, git-committed
deployment repo (config, pinned package version, a config-shape-derived
secret catalog that only surfaces the secrets *this* deployment actually
needs) and `deployment.md` is written as an executable runbook with hard,
independently-verifiable gates ("the task is complete only after `check
--live` passes"). The admin panel is a stateless proxy trusting a
portal-signed cookie; core alone enforces admin-ness. Security posture,
command policy, and egress lists are each stored per-scope and composed
org-floor-then-scope — **tighten-only**, same shape as the raise-never-lower
ZDR rule landed in this session's own gateway work. The keychain: one root
secret, HKDF-derived purpose-separated subkeys per subsystem
(`deriveConnectorKey`); for broker-delivered credentials the agent's sandbox
**never sees the decrypted secret** — it asks core to make the call by
`{credential, url, method, body}`, and core checks entitlement, a pinned
host (suffix match), allowed methods, allowed path prefixes; for personal
credentials, an ask/grant/approve workflow (once vs. standing, audience-scoped)
gates decryption entirely.

**CoreSystem.** A real, role-gated admin surface exists —
`WorkspaceSettingsPage.tsx`, 15 sections, not API-only. ZDR and
Support-AI-mode are genuinely live-wired end to end with optimistic UI and
plan-gating. Per-org router policy (model routing table, complexity
thresholds) is real and live. And — worth noting as a *positive* parallel to
QM's own `SECURITY.md` honesty — the UI **discloses on-screen** that
MFA/domain-restriction/admin-audit are permanently disabled placeholders
("Phase 4 PR-2 de-fake"), rather than silently faking them. What's missing:
**no unified keychain** — two independently-built, single-symmetric-key
AES-256-GCM vaults (`integration-corev2`'s `Vault`, `auth-core`'s
`internal-oauth.service.ts`), no HKDF purpose separation, no credential
broker (confirmed callers receive the plaintext token directly), no
ask/grant/approve workflow. **No org-scoped model allow-listing** —
`model-gateway`'s `unowned_policy_fields()` *explicitly and permanently
refuses* to honor `OrgPolicy.allowed_models` by name, with a code comment
saying accepting it would falsely tell an operator they'd restricted models
when they hadn't. That's good defensive engineering (fail loud, not silently
inert) — but it leaves a real capability gap, not a bug. And the quota
UI-wiring gap from the headline finding: the backend enforcement is real, the
admin form to actually set a limit doesn't exist. **No self-host/`qm init`
motion exists anywhere** — and CoreSystem's architecture (one shared
multi-tenant deployment, Postgres RLS-based org isolation across planes)
actively works against that pivot; this is a product/strategy question, not
an engineering backlog item.

**Recommendations**

| ID | Effort | Impact | What |
|---|---|---|---|
| ADM-1 | M | High | Wire org-core's existing settable-quota API into an admin route and UI form — the backend (migration, service, enforcement) is real; only the "let an admin set it" surface is missing. |
| ADM-2 | L | High | Consolidate the two flat credential vaults into one HKDF purpose-separated keychain, modeled on `deriveConnectorKey` — one root secret, distinct derived subkey per subsystem, instead of two independent single-key AES-GCM stores. |
| ADM-3 | M | Medium | Make an explicit, recorded decision on org-scoped model allow-listing rather than leaving it a repeatedly-rediscoverable gap — `unowned_policy_fields()`'s refusal is the right instinct; the missing feature behind it is the open question. |
| ADM-4 | L | Low | Treat self-host/`qm init`-style deployment as a product/ADR decision, not an engineering item — the multi-tenant RLS architecture is a real structural obstacle, and this bears directly on the EU-residency competitive positioning question flagged when QM was first reviewed this session. |

---

## Prioritized backlog (cross-topic, my judgment)

Ordered by leverage — cheap/high-impact first, structural work after, product
questions last.

**Do first (cheap, high-confidence, no architecture change):**
1. AUTO-1 — cron `workflow_type` field (S/High)
2. ADM-1 — wire org-core quota UI (M/High)
3. INJ-1 + INJ-2 — provenance field + real fail-closed injection gate on the tool-result path (M/High, do together)
4. POL-1 — floor capability-core's risk-level write path (M/High)
5. INJ-3 — wire `ContentSafety` as a second pass (M/Medium, cheap since it's already built)

**Medium-term (apply an existing pattern, or close a specific dead path):**
6. SKILL-1 — extend `mcp_servers`' sharing pattern to `agent_skills` (M/High)
7. ~~SKILL-2 — fix or kill `PromoteSkill` (M/High)~~ — done, see Landed
8. SSRF-1 — SSRF guard for integration-corev2 (M/High)
9. SSRF-2 — extract Quarry's DNS-pinning into a shared crate + regression test (M/High)
10. AUTO-2 — background-job "notify me" path (M/High)
11. AUTO-3 — close the `NatsDispatcher` silent-strand gap (S/Medium) — do before AUTO-2

**Structural (needs an ADR; sequence matters):**
12. HARN-1 → HARN-2 — one tool-execution module, then one loop trait (M then L, in that order)
13. MEM-1 — shared-scope memory tier, additive to the single-owner default (L/High)
14. ADM-2 — one HKDF keychain instead of two flat vaults (L/High)

**Product/strategic (not a sprint item):**
15. ADM-4 — self-host deployment motion vs. EU-residency positioning
16. ADM-3 — decide on org-scoped model allow-listing

---

## Methodology notes

- 8 topics, 24 agents (3 per topic: QM explore → CoreSystem explore → synth),
  running against a full local clone of QM at commit-current-as-of
  2026-08-13 and this repo's `main` at `6c951c6e`.
- **One exploration returned no findings**: the QM-side agent for
  "injection-screening" returned an empty placeholder rather than real
  findings. Topic 2 above is therefore a CoreSystem-only assessment, not a
  QM comparison — there may be real QM patterns on this topic this document
  doesn't reflect.
- **One synthesis claim was caught and dropped**: policy-floor's synthesis
  initially cited `composeSecurityPosture` (a QM file/function) as evidence
  of something QM lacks — a mis-citation, not a substantive error. Corrected
  by re-reading the underlying verified-findings array directly; the four
  "CoreSystem ahead" points kept in section 1 are the ones independently
  confirmed there.
- The research agents did real independent verification — most synthesis
  sections include phrases like "verified live," "confirmed via grep,"
  "independently re-read the source" — and caught genuine errors in their
  own inputs (the DPv2/Temporal misattribution, the org-core quota
  correction). Treat this document with the same standard applied to any
  audit this session: real, but re-check a specific claim before acting on
  it if a lot rides on it.
- Not covered: QM's `src/sandbox`, `src/files`, `src/insights`,
  `src/surface-cache`, `src/onboarding`, `src/environments`, `src/directory`,
  and the Slack/portal/chassis plugins were not explored — the 8 topics were
  chosen for where the two systems' *design intent* overlaps most, not for
  exhaustive coverage of QM's surface area.

---

## Correction to section 3, and how SSRF-2 actually resolved (2026-08-13)

**This plan was wrong about model-gateway.** Section 3 stated it "has no
SSRF guard of any kind beyond a CONNECT_TIMEOUT." Verified false — it has
two, both live:

- `tools.rs::is_egress_safe` (literal-IP check for the RemoteTrigger webhook helper)
- `runtime_registries.rs::endpoint_host_is_forbidden` (hostname/IP blocklist
  for MCP server registration, actively wired)

So the repo had **four** independent Rust SSRF checks, not three. SSRF-4's
"audit model-gateway's outbound calls" is therefore partly already answered.

**More importantly, the codebase had already reached this plan's conclusion
on its own.** `grpc.rs`'s `remote_trigger` RPC does not call its own guard —
it unconditionally returns:

> `failed_precondition("remote_trigger is quarantined until hostname DNS rebinding defenses are enforced by Quarry")`

That is a direct-dial capability deliberately disabled because a
literal-IP-only check was judged insufficient, with the reason stated in the
error a caller receives. Fail closed and say why, rather than paper over the
gap. That is precedent for the decision below, arrived at independently and
earlier.

### SSRF-2's recommendation was overruled, deliberately

This plan asked for Quarry's DNS-pinning to be "extracted into a shared
primitive." That was written before the packaging constraints were known and
is the wrong answer:

- The three consumers sit in **three separate Cargo workspaces**; the
  verevonv3 gateway crate is standalone with no path dependencies outside
  itself at all.
- Across all 41 `Cargo.toml` files in the repo, **every** `path = "../..."`
  dependency stays inside one plane's own workspace. There is no precedent
  for crossing plane directories, and CLAUDE.md's plane-ownership rules
  discourage it.
- The implementations differ in kind, not just polish: Quarry resolves and
  pins (coupled to `quarry_core`/`quarry_security`, so "extracting" is closer
  to rewriting); the gateway is deliberately a pre-filter because its job is
  to forward, not to dial.

Creating the repo's first cross-plane Rust coupling to deduplicate a guard
whose users have different threat models would cost more than it saves.

**What landed instead:** the invariant the gateway's safety silently rested
on is now written down and enforced. `public_url.rs` never resolves DNS and
is sufficient only because every call site forwards the URL to Quarry as
content rather than dialing it — add one direct-dial call site and that weak
check silently becomes the whole defense.
`gateway/tests/ssrf_forward_not_fetch.rs` scans every `src/**.rs` dial and
requires its target be anchored to a fixed `AppState` config field, so the
drift fails a test instead of passing silently. Both modules' doc comments
now name which one is the boundary. The test was validated by injecting a
synthetic violation, confirming the failure, then removing it — not merely
observed to pass.

---

# Status as of 2026-08-13 evening

The headline finding above still stands, but several of its examples are now
closed. Recording that here rather than leaving the document claiming gaps that
no longer exist.

## Landed

| Item | What changed |
|---|---|
| **AUTO-1** | A cron schedule can select its workflow. The plan called this frontend-only; that was wrong — the sweeper never wrote `config_json`, so `dispatchPlan` always fell through to the default and the UI field would have been inert. Both halves fixed. |
| **ADM-1** | Org spend/token ceilings are settable from a new Forbrukstak settings section through an org-admin-gated gateway route. Enforcement was already live; only the way to set a number was missing. |
| **POL-1/2/3** | `cap.command.shell` can no longer be downgraded to `low` by any caller holding plain `capability:write` — which silently disabled human approval for shell execution. The org-blind last-write-wins `Registry.Get` chain is gone. Limitations disclosed in the service README. |
| **AUTO-3** | The task executor's fallback dispatcher no longer strands claimed tasks in `running` forever with no per-task signal. |
| **SSRF-1** | integration-corev2 has a real DNS-pinning egress guard. Microsoft Graph's `nextLink` pagination followed a URL out of an API response with **no guard of any kind** — that is now closed. |
| **SSRF-2** | Resolved as a documented contract rather than a shared crate; see the correction section above for why. The gateway's forward-don't-fetch invariant is now enforced by a test instead of being incidental. |
| **INJ-1/2/4** | Provenance and fail-closed screening on the tool-result path, built against S2.7's vocabulary. |
| **SKILL-2** | Killed, not fixed: `PromoteSkill` now falls through to the embedded `UnimplementedCapabilityCoreServer.PromoteSkill` (plain `codes.Unimplemented`), with a doc-comment citing this section by name and `server_test.go`'s `TestPromoteSkillIsUnimplemented` pinning it. `orchestrator-core`'s `SkillPromotionWorkflow` remains registered (headline finding above) but has no live caller until an admin-gated promotion flow is built on top of a real RPC. |
| *(bonus)* | Verb/Object/Outcome activity grammar and the six-tab Space cockpit shell, from the UI research document. |

## 2026-08-15 source progress: scoped scheduled-run authority

QM's useful scheduling lesson is not its job runner; it is the durable,
scope-bound handoff between a planned background task and the effect it causes.
CoreSystem now has that source-level lane without importing QM's runtime:

- Control's one-fire `model.schedule.run` decision is consumed by Capability
  Core only to prepare the exact Session Core thread. A second,
  independently scoped `model.schedule.execute` decision is minted only when
  Orchestrator reaches the Session Core start effect. The two bearers have
  distinct audiences, actions, schemas, permissions, and service principals.
  Neither is persisted in task events or Temporal input/history.
- Capability Core routes a prepared fire to a dedicated
  `ScheduledRunSupervision` workflow, carrying only deterministic non-secret
  facts: task/run ID, thread ID, Space/subject, schedule/fire identity,
  template digest, idempotency key, and the canonical non-secret task-template
  JSON—never independent `goal` or `policy`. Capability Core reconstructs that
  template from persisted task configuration (excluding only the fire intent),
  recomputes its SHA-256 digest, and refuses a mismatch before a fresh Control
  call. Orchestrator recomputes the same digest and derives the workload from
  the template; strict decoding rejects standalone execution fields. Focused
  regressions cover detached-template rejection before Control reauthorization
  plus forged digest/goal/policy workflow inputs. The template body remains
  non-bearer data, while Control and Session bind their decisions to its digest.
  Generic interactive workflows cannot acquire a prepared thread by adding
  input fields.
- Orchestrator accepts this workflow only from the authenticated
  `capability-core` service and calls a separate `StartScheduledRunActivity`.
  The shared internal secret is intentionally insufficient.
- Session Core has a dedicated owner-only `StartScheduledRun` RPC. It requires
  `service:orchestrator-core`, verifies the fresh execution decision against
  the stored prepared-thread Space/audience/privacy/resource/revision bindings,
  then creates/reuses a duplicate run only when thread, organization, and owner
  match. A fresh Control decision nonce no longer breaks a retry because
  preparation compares stable authority bindings instead.

Focused Control, Capability Core, Orchestrator, and Session Core source checks
pass, including a signed-execution-bearer regression that rejects a changed
prepared thread.

## 2026-08-16 source progress: owner continuation and scheduled-step safety

The next proof slice is also source-complete only and remains fail-closed:

- Capability Core's generic health reporter can no longer attest the reserved
  ticket capability or write a global row through a tenant-health credential;
  the signed middleware regression proves the attempted write is not persisted.
  A disposable-Postgres integration case now exercises the reserved
  `cap.tool.ticket.create` row itself: both tenant `HealthWriteScope` and the
  exact generic `service:execution-core` global-health identity receive 403,
  with no state/timestamp or global-attestation audit row change.
- The approval-delivery worker now has a dedicated `tickets.create` adapter.
  It freezes the schema, canonical payload digest, derived idempotency key, and
  owner subject, obtains a fresh Control decision, and calls the private
  Conversation Core route. On an ambiguous response it reconciles the durable
  owner receipt before retrying; if the lookup cannot settle, the worker records
  `unknown_outcome` instead of guessing.
- Scheduled work now has a bounded service-owned `ExecuteScheduledStep` runtime
  in source: Control/Orchestrator decisions are independently verified at the
  effect boundary, Session Core claims the exact run/step, Execution Core
  mints only a tenant-scoped `inference:invoke` token, reads the exact goal
  through Session Core's dedicated scheduled-context RPC, runs one tool-free
  turn, and records `completed`, `failed`, or honest
  `unknown_outcome` receipts. The lane never widens user-bound `ExecuteStep`
  and remains disabled when its verifier, service scopes, provider, or Session
  credential are absent. Disposable Postgres crash/receipt proof, explicit
  Auth Core scope registration, provider/ZDR attestation, and live candidate
  evidence remain missing.

These changes improve QM's lease/idempotency and scope-bound handoff patterns
without copying QM's single-tenant trust assumptions or enabling a Model action
before owner-plane and release evidence gates pass.
This is deliberately **not** a claim of shipped AUTO-2/notification behavior
or of full S4.1 completion: the Control service-principal registry and the
Orchestrator execution credential are intentionally not populated or rotated
by this source change, so the new path remains fail-closed in the current dev
stack. Live Control/Postgres/Temporal validation, delivery/approval references,
revocation/deletion races, and the Verevon disable/repair UI are still open.
There is a second source-level gate behind that configuration: after Session
Core creates the service-owned run, new `ScheduledRunSupervision` histories now
route through the replay-versioned dedicated scheduled-step activity rather
than the ordinary user-bound `ExecuteStep` activity. The new service bearer
proves only the Execution Core ingress identity; Session claim/receipt and the
bounded runtime adapter remain intentionally absent. Adding dev credentials
alone would only advance the failure to that boundary. The canonical-template
integrity gate is now implemented in source, but it does not substitute for the
effect-time authority contract or live proof.

The contract is now explicitly scoped in the V3 comparison plan as
`model.schedule.step` v1: only Orchestrator may obtain a short-lived execution
decision for one deterministic `{run, thread, schedule, fire, template, step}`
tuple; Session Core supplies a content-free current-run/step receipt; and the
new Execution Core RPC is service-only rather than relaxing user-delegated
`ExecuteStep`. It preserves the owner-plane authority check for each tool and
requires `unknown_outcome` reconciliation rather than retrying an ambiguous
external effect. This is source/test evidence, not deployed release proof.
The plan continues to reject QM's
in-memory web-run registry and “exactly once” delivery language; the target is
durable at-least-once work plus owner idempotency and honest reconciliation.

The release gate now has a separate read-only cross-plane preflight at
`scripts/coresystem-cross-plane-preflight.sh`. Its contract test proves that a
healthy local topology is not mistaken for readiness: Control, Application,
Data, Ingestion, Frontend, Temporal, and NATS containers are observed; named
authority/owner-effect inputs are checked for presence only; and provider/ZDR,
approval-continuation, and candidate evidence remain explicit blockers. The
root `scripts/coresystem-conformance.sh` composes this result with the Model
preflight and remains blocked until live effect probes, signed candidate/
rollback artifacts, and deployment evidence exist.

The preflight now also reports the five Execution Core `tickets.create` adapter
bindings next to Conversation Core's owner-decision and reservation inputs.
This makes the QM-style centrally wrapped owner path observable as a complete
configuration surface while keeping the Model action disabled when any hop is
missing.

The same conformance path now validates
`apps/Model Plane/docs/CAPABILITY_PROMOTION_LEDGER.tsv` against every seeded
capability migration ID. All rows are currently `source_only`, including
`cap.tool.ticket.create`; no UI flag, health attestation, or source test can
advance a row without candidate/live/rollback evidence and an operator review.

R-2 now has one reproducible health proof harness,
`apps/Model Plane/scripts/tests/capability-health-proof.sh`, combining the
generic reporter allowlist, tenant-to-global no-write, stale/unhealthy policy,
and signed disposable-Postgres checks. It deliberately leaves promotion at
`source_only`; candidate-bound stale/outage observation is still required.

The scheduled-step evidence is also reproducible through
`apps/Model Plane/scripts/tests/scheduled-step-proof.sh`: it runs the prepared
thread/Temporal retry and `unknown_outcome` workflow tests, exact Orchestrator
activity handoff tests, Execution Core contract tests, and the real-migration
Session/Postgres receipt proof. It does not claim live provider, Auth Core,
ZDR, candidate, or rollback readiness.

Runtime recheck, 2026-08-15: the running Capability Core container has no
`CONTROL_USER_CORE_URL`, `CAPABILITY_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN`,
`CONTROL_SPACE_DECISION_KEY_ID`, or `CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64`;
the Orchestrator equivalents and its Control URL/key are present only as empty
values; Session Core has no Control decision verifier. Its logs therefore say
that cron sweeping and execution reauthorization are not started. The health
endpoints remain healthy, but this is deliberately not a successful scheduled
run. No value was inspected, generated, injected, or rotated.

The same existing-config audit prevents the owner-grant browser journey: the
running Control User Core has neither its decision key ID nor private signing
key, and Conversation Core has neither matching verifier value. The V3 gateway
does have its actual `USER_CORE_URL` and `CONVERSATION_CORE_URL`, but its
protected owner-grant route returns `401` without an authenticated browser
session; `/api/auth/get-session` is not a gateway route (`404`). The missing
signer/verifier already makes a Control decision impossible, so these are
independent prerequisites for the Control → V3 gateway → Conversation Core
durable-receipt proof. No identity, key, URL, or credential was created, read,
injected, or rotated.

## 2026-08-15 source update: one owner-governed Model action remains gated

QM's centrally wrapped tool surface is a useful pattern, but CoreSystem must
not turn a Model service credential or a source-thread reference into authority
over an Application resource. The first `tickets.create` slice now makes that
distinction executable in source:

- Session Core exposes a content-free, Control-authenticated run projection;
  Control uses it to re-resolve the subject, Space, current entitlement,
  audience, and privacy policy before issuing a two-minute, target-bound
  `run-action-v1` decision. The decision binds one schema hash, payload digest,
  idempotency key, run, and thread. It is provenance for the source context,
  not a Conversation Core ACL.
- Execution Core has one fixed adapter rather than a generic cross-plane tool
  caller. It derives its retry key, rejects actor/organization/assignment
  fields from model input, asks Control for the decision immediately before the
  effect, and gives Conversation Core only the signed decision plus the narrow
  ticket payload. The tool receives only a minimal operation receipt.
- Conversation Core derives the human actor and organization only from the
  Control signature and uses its existing owner operation ledger, outbox, and
  receipt. A recognized workload still cannot use the public human endpoint.
  Its private owner transaction locks and checks an exact active conversation
  grant against the signed Space, subject, recipient-audience reference, and
  privacy-policy reference before it can write the ticket.

This is deliberately not a claim that the Model can create tickets. The
capability seed remains unavailable, the Model catalog/allowlist remains
empty, and no new credential value was created or read. The owner-grant
lifecycle and its source proofs are recorded below; the independently
authorized owner-action health contract and server-resolved Model view now
exist only as fail-closed source paths. Remaining gates are an authenticated
dev journey, a real owner readiness reporter after the ticket-specific
continuation proof, and remediation of the sealed review's Control-revocation
fence and live authenticated-transport proof. Conversation Core now requires
HTTPS for the credentialed current-Control read, with only an explicit
IP-loopback development opt-in; Control-to-Session gRPC now defaults to TLS
with an equivalent loopback-only development exception. This keeps the stronger CoreSystem
owner-plane wall while adopting QM's useful single, centrally governed tool
path.

The remaining Control-revocation/owner-commit race is not a cache invalidation
bug. The V3 plan now specifies the required `owner-effect-reservation-v1`
protocol: Conversation Core first records a content-free pending operation;
Control alone reserves and commits the exact digest under its revocation locks;
only then may Conversation Core make its local effect visible. A post-commit
crash is `unknown_outcome` and reconciles from durable Control/owner receipts,
not a blind retry. This preserves the domain-owner transaction while giving
Control a real authorization linearization point.

**2026-08-16 source progress (not a closure claim):** Control now has the
content-free reservation ledger, an authority-revision cancellation trigger,
exact commitment validation, and private Conversation-Core-only
reserve/commit/reconciliation route plumbing. Conversation Core now resolves
the exact opaque owner-grant ID, reserves and commits the signed operation at
Control, then rechecks that same grant under its ticket transaction lock before
writing the ticket/audit/outbox receipt. The new reservation credential is
distinct from the current-authority credential; neither it nor the signed
decision persists. Handler regressions prove commit precedes owner write and a
denial writes nothing; Control and Conversation Core Go suites pass. Pending
intent and unknown-state migration support exists, but crash reconciliation,
real cross-database interleavings, and deliberately provisioned dev proof are
still open. No Model ticket capability has been enabled.

The repeatable `apps/Model Plane/scripts/tests/tickets-create-proof.sh` now
adds source and disposable-Postgres evidence for this slice: Execution Core
ticket-contract tests, Control reservation cancellation/commit preservation,
Conversation owner-grant revoke/effect races, and durable unknown/receipt
replay all pass against the real migrations. This narrows the remaining work
to deployed signer/verifier configuration, authenticated cross-plane transport,
provider receipt/reconciliation, crash-after-submit observation, and immutable
candidate/rollback evidence; it does not enable the Model action.

The remaining source-level wiring omission is now closed without changing the
runtime posture: Model Plane Compose and `.env.example` declare the exact five
Execution Core `tickets.create` adapter bindings with empty defaults, and
`scripts/tests/tickets-create-config-contract-test.sh` rejects missing names or
literal values. This is a configuration contract, not credential provisioning;
the adapter stays disabled until the existing Control/Conversation references
are supplied and the authenticated dev journey is observed.

The scheduled lane has the same explicit source contract: a dedicated
`scheduled-step-config-contract-test.sh` checks the empty-default Control
bindings and the Auth Core scopes required for preparation, service-owned run
creation, Session claim/receipt, and the dedicated Execution ingress. The live
stack still lacks those runtime registrations, so this is wiring evidence only
and scheduled effects remain disabled.

## 2026-08-15 source update: personal owner grant lifecycle

The prerequisite grant lifecycle is now source-complete for one **personal
Space** ticket action without widening it into a Model capability:

- Control issues a distinct two-minute `owner-grant-v1` decision only for a
  current personal-Space `owner`/`manager` with the current durable-agent-action
  entitlement and non-ZDR policy. It binds exact org, subject, conversation,
  Space, recipient-audience reference/hash/revision, privacy policy, authority
  revision, action, idempotency key, and either `create` or the exact `revoke`
  grant ID. Shared Spaces are denied rather than inferred.
- V3's authenticated BFF is the short-lived bearer conduit. It checks the
  returned Control binding, keeps the token out of browser responses, and
  immediately forwards it over its existing signed user delegation to
  Conversation Core. The browser supplies only bounded IDs and an idempotency
  key. Owner/admin role checks are repeated at Conversation Core.
- Conversation Core verifies the separate envelope, exact path/principal/org
  binding, and operation before it writes. It persists content-free grant and
  revoke receipts/audit/outbox rows; every active effect check now intersects
  *hash and revisions* as well as Space, subject, audience ref, and privacy
  ref in the same transaction as the ticket effect. Revoke deliberately still
  works after audience/privacy revision changes, but only for the same personal
  Space/subject and exact grant path.

Focused Control, Conversation Core (full `go test ./...`), and V3 gateway
compile/role-denial checks pass. A disposable-Postgres proof now covers
grant → effect → revoke, concurrent revoke/effect fencing, exact replay,
forged/expired/wrong-target denials, and revision-change denial. This remains
source/test evidence: no configured dev key pair or authenticated running-stack
journey has been claimed. The Model remains unavailable until those runtime
gates, an independently authorized owner-action health contract, and a
server-resolved Model view succeed.

## 2026-08-15 source update: Model availability cannot be fabricated

The owner-action boundary is now protected at all three relevant Model layers:

- Execution Core does not accept caller-provided or MCP-discovered
  `tickets.create` definitions. Its fixed tool list excludes the name, and the
  reserved-name guard trims whitespace before it builds the model-facing
  purpose lock. The sandbox probe can attest only its shell/interpreter
  capabilities, never the Control → Conversation Core adapter.
- Capability Core rejects `cap.tool.ticket.create` on every generic health
  lane, including a signed `execution-core` global-health workload and a tenant
  health credential trying to claim the `global` organization. The global
  health identity is limited to the two execution-core capabilities it actually
  probes (`cap.command.sandbox`, `cap.command.shell`), so that bearer cannot
  become a universal availability authority.
- Middleware/handler tests prove neither generic route reaches the durable
  ticket health write. Full Capability Core Go tests and the 402-test
  Execution Core library suite pass.

The next source slice now makes that future boundary concrete without enabling
it. Control issues a separate two-minute `model-action-view-v1` envelope for a
server-fixed `tickets.create` schema and run; it carries current Space,
audience, privacy, retention, and authority facts but only the
`model-action:view` permission—not a target-owner effect permission. Execution
Core must use a separately scoped Control service token to obtain the view, and
Capability Core verifies the Control signature, run, tenant, expiry, exact
schema hash, and current availability before returning the one fixed tool
definition. Missing configuration, a bad signature, a stale view, or a
non-runnable health record produces an empty Model action set.

The resolved definition is not an approval bypass: Execution Core forces this
reserved owner action to pause even when a caller selected `auto` or supplied a
malformed mode, persists the stricter `ask` posture in its descriptor, and
rejects it outright from the public `ExecuteStep` RPC. A future continuation
worker must carry a durable approval that binds the run, step, schema and
payload before it may call the adapter; today that worker does not exist, so a
granted approval cannot be mistaken for execution authority.

A bounded follow-up source review found no alternate path: whitespace aliases,
caller-supplied tool definitions, MCP discovery, auto/empty/malformed
permission modes, and the current generic approval-resume worker all remain
unable to execute `tickets.create`. The full Execution Core library suite
passes 402 tests. This is source/test evidence only, not a readiness claim.

Capability Core now also has a separate owner-action health receiver. It
accepts only `conversation-core` with the exact global
`capability:owner-action:health:write` scope, only for
`cap.tool.ticket.create`, and only once its own Control public-key verifier is
configured. The generic execution health authority remains unable to write
that row. The remaining release gate is deliberately operational: Conversation
Core needs a real readiness reporter using that scope **only after** the
durable approval-continuation worker exists and has passed its run/step/schema/
payload-binding proof. The shared dev stack then needs the existing
deployment-owned keys/service principals plus the proven authenticated journey.
No key was generated, injected, or rotated here.

That continuation is intentionally a new ticket-specific contract, not an
extra case in the existing provider/shipment dispatcher. At delivery time it
must consume Session Core's active leased approval receipt and frozen
descriptor, obtain fresh Control target-action authorization and Capability
Core policy for the exact run/step/schema/payload/idempotency tuple, then let
Conversation Core repeat its current owner-grant transaction before the
effect. An ambiguous owner response must reconcile by idempotency receipt and
remain `unknown` when it cannot; it must never be blindly retried. Until that
contract and its tests exist, no readiness reporter may attest this capability
as available.

## The INJ finding that was worse than this plan said

Section 2 said `scan_injection` had one call site and the rest of the
tool-result path had none. True — but it missed that **the one call site was
itself defeated by the attack it existed to catch**. `add_context_entry`
scanned the already-truncated 600-character snippet, so a marker planted past
the cutoff sailed through unflagged. The only injection defence in the codebase
was effectively inert against middle-of-payload. Fixed, with a regression test
that plants a marker past the truncation point.

## SSRF-4: closed by investigation, not by code

This plan claimed model-gateway had "no SSRF guard of any kind beyond a
CONNECT_TIMEOUT". Verified false — `tools.rs::is_egress_safe` and
`runtime_registries.rs::endpoint_host_is_forbidden` both exist and are wired.
More importantly, `grpc.rs`'s `remote_trigger` RPC does not call its guard at
all; it returns
`failed_precondition("remote_trigger is quarantined until hostname DNS
rebinding defenses are enforced by Quarry")`. A direct-dial capability
deliberately disabled, with the reason in the error the caller receives. No
further work needed.

## Deliberately not started, and why

- **SKILL-1** (personal skill scope, grants, org promotion), **MEM-1** (shared
  room memory), **ADM-2** (HKDF keychain), **AUTO-2** (watch/notify): each is a
  leaf in the Space adoption plan's dependency graph (S3.5, S3.x, S3.4, S4.2).
  Building them org/user-scoped now would become migration debt the moment
  Space lands. They wait on that workstream, not on capacity.
- **HARN-1/2** — **withdrawn 2026-08-14, not deferred.** When the tree went
  quiet the premise was measured before any code moved, and it did not hold.
  The two dispatchers share **zero** tools: `dispatch_tool` is an 18-arm
  read-tool router whose first act is to refuse anything side-effecting, while
  `execute_step_inner` is a capability/hook/permission pipeline around
  sandboxed execution. That refusal *is* the authority boundary, and one module
  owning both would make the next accidental cross-call compile. HARN-2's
  proposed `run_turn(ctx, goal, ...)` also fits only one of the two loops:
  `run_rounds` seeds its own history from a goal because subagent isolation
  depends on it, `run_tool_rounds` receives a caller's thread because it
  continues a conversation.

  The round-budget constants this plan read as drift turned out to be
  deliberate and documented — execution-core's comment names model-gateway's
  and states the invariant that a deployed agent must never get less room than
  chat. They were enforced by comment alone, so they are now asserted in
  `execution-core/tests/cross_service_loop_contract.rs` (verified to fail when
  perturbed, not merely to pass).

  HARN-3 survived and is written:
  `docs/architecture/adr-agent-loop-and-tool-execution-boundary.md`. Real
  duplication does exist, just elsewhere — provenance/screening rendering is
  implemented twice, and that one carries no authority.
- **ADM-3/ADM-4**: product decisions, not engineering items.

## Closed 2026-08-14: the credential that was blocking ADM-1

`model-gateway` is now registered in Control Plane's
`ORG_CORE_SERVICE_CREDENTIALS`, and the ceiling enforces. Verified against the
running service: the quota read returns 200 with the credential and 401 with a
wrong token, so the check is live rather than an unauthenticated endpoint.

**The scope this section previously asked for would have broken Control Plane.**
It said to register "`org:read:any` plus settings-write". org-core's
`validateServiceCredential` rejects any `:self` scope held by a principal other
than `verevon-gateway`, and that check runs from `main.go` *before* any listener
opens — so adding `org:settings:write:self` would not have granted the cap
write, it would have stopped org-core from booting. The registration is
`org:read:any` only.

Cap writes were never missing either, they live one plane over:
`PUT .../quotas/:key` sits behind org-core's membership guard and requires a
verified v3 HMAC delegation naming the acting user, because setting an org's
spend cap is a governed admin action. The Verevon gateway already holds the
scope, mints that delegation, and serves the write — which is the route the
Forbrukstak UI uses. `SetPolicy`'s own service-authenticated cap write returns
403 by design; that is documented at `put_org_quota` so the next reader does not
try to widen the credential.

The secret has one canonical local home rather than a copy per plane. Both
launchers read Control's persisted store, so a rotation cannot leave a stale
copy shadowing the live value — which would have failed silently, since a wrong
token 403s the read and `fetch_org_limits` fails open.

## The pattern that keeps recurring

Three times today a proto addition compiled cleanly for its own service and
broke a *test-only* trait implementation in another — `delete_thread` /
`delete_threads`, then `DeleteSpaceThreads`, both in execution-core's
`MockSession`. A per-service `cargo build` never catches it because the library
builds; only the test target fails. After any session-core proto change, run a
workspace-wide `cargo test`, not a build.
