# The Verevon Creed — Vision, Goals, and Execution Plan

**Companion to:** the Audit Ledger (67-item verification, 2026-08-25) and `Verevon-ai-first.md` (market strategy).
**Basis:** 33 action contracts / 183 mutating UI operations / 14 model tools / 0 runs — every number verified against source and the live fleet on 2026-08-25/26.

---

## 0. The vision

Verevon is AI-first when a model and a person are two clients of the same governed contract — neither one privileged, neither one blind. That is tested by four clauses, and each one is a measurable number, not a slogan:

| Clause | Score | Basis |
|---|---|---|
| **1. Every UI is an API** | ~18% | 33 typed contracts / 183 mutating operations across 38 client modules |
| **2. AI can do everything a human can** | 0% | `MODEL_EXECUTABLE_ACTION_IDS` is an empty set; 14 read-only tools vs 183 human ops |
| **3. Everything AI can do, a human can do too** | largely holds | All 14 advertised tools are reads/artifacts with UI counterparts; 2 exceptions to close |
| **4. Everything AI does, the human can see** | unproven | The console consumes 14 typed event classes — but 0 runs, 0 threads, 0 messages, ever |

The clauses are ordered by dependency, not importance. Clause 1 is the substrate: an action contract is the *only* thing all other clauses attach to — the model's tool definition, the human's button, the approval gate, and the audit event are four views of one registry entry. Clause 2 is what makes the product AI-first rather than AI-adjacent. Clauses 3 and 4 are what make clause 2 **safe to want**: capability without parity is a shadow system, and capability without witness is an unaccountable one.

**The honest reading of today's scorecard: the danger runs the opposite way from the usual worry.** The risk in this codebase is not a model doing things humans can't see — it is a model that can do almost nothing at all, sitting on top of an unusually good contract layer that nothing has switched on. The empty allowlist is not a bug; its own comment calls it *"the honest and safe default"* until each action's owner proves the governed path. The plan below keeps that discipline and industrializes it.

---

## 1. The goal, stated honestly

Clause 2 must never literally reach 100% — and saying so up front is what keeps the creed from becoming a security anti-goal.

Some operations are human-only by design: authentication and session management (18 ops in `auth-client`), organization deletion, payment-method changes, membership grants. The goal is therefore a **three-tier denominator**, where every one of the 183 operations lands in exactly one tier, with the third tier short, explicit, and justified in writing:

| Tier | Meaning | Governance |
|---|---|---|
| **model-eligible** | Model executes directly through the governed path | Owner contract + Capability Core binding + forged-call denial, per action |
| **approval-gated** | Model proposes; a person approves; the exact approved effect executes | Encrypted continuation descriptor + receipt (the dispatcher that already runs) |
| **human-only** | Model can never invoke it, and the exemption is documented in the registry itself | An `exempt` marker with a stated reason, enforced by the same CI gate |

### Milestones — target scores per clause

| Milestone | Clause 1 · contract | Clause 2 · capability | Clause 3 · parity | Clause 4 · witness |
|---|---|---|---|---|
| **M1** — loop proven | 18% | 1 action live end-to-end | affordance map test in CI | **proven once**, trace recorded |
| **M2** — coverage engine | ≥50% | ≥10 low-risk actions | 0 model-only tools | every run renders in the console |
| **M3** — full classification | ≥90% + explicit exemptions | all eligible actions live | holds, CI-enforced | continuous, CI-proven |
| **M4** — outward parity | the same registry served to external agents (MCP server) and in-browser agents (WebMCP) | | same witness, all surfaces |

Milestones are gates, not dates: M2 does not open until M1's trace exists, because every behavioural claim in the audit is inferential until the fleet has processed one real request. Rough shape if staffing holds: M1 in 1–2 weeks, M2 around week 6, M3 around week 12.

---

## 2. What the plan must respect

Three findings from this week's verification change the shape of the work — two of them corrections to earlier assumptions.

### Eligibility is earned per action, not unlocked by config

The earlier framing — "one credential variable blocks everything" — was wrong. `PLANE_SERVICE_PRINCIPALS_JSON` already holds **15 principals**, and 2 of 29 capabilities are already health-attested. The real gate is deliberate code: capability-core's generic health route allowlists exactly two capability IDs, with the comment *"a scope alone is intentionally not a universal availability authority."* So clause 2 grows one vertical slice at a time: owner exposes the governed operation contract → Capability Core binds it → owner enforces forged-call denial → the action ID enters the allowlist. The plan's job is to make that slice a **repeatable one-week recipe** instead of a bespoke project.

### The parity primitive already exists — protect it

`ActionCommand.tsx` carries the whole creed in one comment: *"Human clicks and model-initiated commands both pass through this component contract."* One registry entry already yields the button, the preview, the tool definition, and the approval flag. Nothing in this plan invents a new mechanism; every workstream widens or wires this one.

### Some clause-4 legs are blocked on signing work, and no backfill exists

"The human can see" includes being *notified* — and notification-core's identity-sync consumer is deliberately disabled until **signed, revisioned authority events** ship (audit item AP-P0-4). Its subscriber tables hold 0 rows, the only write path is the disabled consumer, and no backfill or replay source exists. The notifications leg of clause 4 therefore has a hard prerequisite the frontend cannot route around, and it needs a backfill built alongside the signing work or existing users stay dark forever.

---

## 3. The plan

Five phases. Phase 0 is a single vertical that moves two clauses off zero; everything after it runs the same recipe at scale.

### Phase 0 — Prove the loop: one action, end to end
**Window:** 1–2 weeks · gates everything

Drive **`tickets.create`** through the entire governed path. It is the right first vertical because the hard machinery already exists and has never fired: two-phase reservation/commit fencing with a `FOR UPDATE` guard and idempotent replay, the AES-GCM approval-continuation dispatcher (built, running, blocked on one empty encryption key), and conversation-core's HITL gate (`CreateAIAction` → `AllowAIProposal`).

1. Fill the schedule/run-action/owner-effect lane tokens by extending `run-control-plane.sh` — mint each token **once**, write registry entry and consumer env in the same pass. Never hand-edit two files; independent minting is the drift machine behind every credential failure this month.
2. Set `SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY` and align `EXECUTION_ORG_CORE_SERVICE_TOKEN`. Today the empty key means a pause doesn't merely fail to resume — **the pause itself returns `unavailable`** — so HITL cannot even begin.
3. Run the full chain live: model proposes → HITL approval → descriptor-bound execution through the fence → owner receipt → typed activity events in the run console.
4. Write the forged-call denial test, then add `tickets.create` as the **first entry** in `MODEL_EXECUTABLE_ACTION_IDS`.

**Moves:** clause 2 → first action live · clause 4 → proven once, trace recorded · the fleet's first real request — discharging the "never exercised" caveat on ~15 audit items at once.

### Phase 1 — Coverage engine: 33 contracts toward 183
**Window:** weeks 2–6 · parallel to Phase 0

Triage all 183 mutating operations into the three tiers, then convert client by client, highest leverage first: `tickets` (22), `inbox` (16), `knowledge` (10), `social` / `spaces` / `settings` (6 each). `auth-client` (18) and `org-deletion` (4) go straight to the human-only tier with written reasons.

- **The ratchet:** extend the existing registry-to-dispatcher parity test into a CI gate that fails when any client adds a mutating call without a registry action or an explicit `exempt` tag. Coverage becomes monotonic — it can only go up.
- **The scoreboard:** a script that computes all four clause scores from source (registry count, client scan, allowlist size, tool/affordance map, live run count) so the creed is a number in CI, not a slogan in a doc.

**Moves:** clause 1 → ≥50% by M2, ≥90% by M3 with the remainder explicitly exempt.

### Phase 2 — Eligibility pipeline: the recipe at scale
**Window:** weeks 4–12

Turn Phase 0's vertical into a per-action pipeline, batched by the registry's own risk metadata: the 16 low-risk reversible actions first, then medium-risk with approval previews, and the 3 high-risk actions permanently approval-gated. Each action ships with owner contract, capability binding, forged-call denial test, and its allowlist entry in one PR.

- Schedule the **capability-health widening decisions** with each owning plane — the two-ID allowlist in `availability.go` is a design decision per owner, not a config change, and 27 capabilities wait on it.
- Close the dispatcher's descriptor gaps: three approval sites currently mint no continuation descriptor, so their approvals cannot resume the exact approved effect.

**Moves:** clause 2 → ≥10 actions by M2, all eligible actions by M3.

### Phase 3 — Parity & witness: hard guarantees, not habits
**Window:** continuous · gates at every milestone

- Build human affordances for the two exceptions — `code_interpreter` and `result_query` — then add the CI test: **advertised tool list ⊆ UI affordance map**. Clause 3 stops being an observation and becomes an invariant.
- Apply the verb/object/outcome activity grammar to the run console's event stream, so every model action reads as a sentence a colleague would write.
- Harden stream-resume into a real witness channel: set `REDIS_URL` (the buffer is single-process today), add an event cursor (every resume currently replays from zero), replay tool and citation events, not just text.
- Land the signed membership fan-out (AP-P0-4) **with a backfill**, then enable notification-core's identity sync — the notifications leg of clause 4.

**Moves:** clause 3 → 0 model-only tools, enforced · clause 4 → every run renders, continuously proven.

### Phase 4 — Outward parity: the same contract, three agent surfaces
**Window:** after M2 · mostly wiring

One registry, three consumers — all inheriting the same risk metadata, approval gates, and witness stream. Deliberately sequenced after M2 so the bridges don't ship advertising an empty set:

- **MCP server** exposing the registry to external agents. Today `mcp-bridge` only *consumes* external MCP servers; nothing exposes Verevon outward. This is what makes "every UI is an API" true beyond Verevon's own walls.
- **WebMCP bridge** behind a flag for in-browser agents — `document.modelContext.registerTool` maps nearly field-for-field onto `ActionDescriptor`, headers already permit it, and `requiresApproval` actions return a needs-approval result instead of executing. Cheap and reversible; the spec is a W3C incubation, so a bet, not a foundation.
- **SSR for shareable read-only resources only** — a report or Space summary an external agent can actually read. Not whole-app SSR: the frontend is mid-migration and the workspace correctly stays `noindex` (robots.txt shipped 2026-08-26).

**Moves:** M4 — external, in-browser, and platform agents all governed by one contract.

---

## 4. Operating rules

Five rules make the scorecard monotonic. Each one is a CI check, not a convention.

1. **Contract first.** No meaningful UI operation ships without a registry action or an explicit exemption. Already the CLAUDE.md convention — Phase 1 turns it into a failing test.
2. **Eligibility is earned, never bulk-flipped.** An action enters the allowlist only with its owner's governed contract, a Capability Core binding, and a forged-call denial test. The allowlist is a ledger of proofs, not a feature flag.
3. **No model-only capabilities.** Every advertised tool has a human affordance before it ships. Enforced as a subset test: tool list ⊆ affordance map.
4. **Every action is witnessed.** Every model-initiated execution emits typed events the run console renders. An action that cannot be watched cannot be eligible.
5. **Exemptions are product, not gaps.** The human-only list is part of the creed — short, written down in the registry, and reviewed like code. "AI can do everything a human can" means everything *except this list, on purpose*.

---

## 5. Risks and dependencies

| Risk | Consequence | Mitigation |
|---|---|---|
| **Fleet is unexercised** — every behavioural claim is inferential until Phase 0's trace | The plan could rest on machinery that fails on first contact | Phase 0 is deliberately tiny and first; nothing scales before the trace exists |
| **Capability-health widening is per-owner design work**, not config | Clause 2 stalls if treated as an ops task | Scheduled as explicit design decisions with each owning plane in Phase 2 |
| **Notification signing (AP-P0-4) has no backfill** | Even after signing lands, existing users stay dark — a permanent silent gap | Backfill built alongside the signing work, never after it |
| **Clause 2 read literally becomes an anti-goal** | Auth, billing, and deletion ops must never be model-executable | The three-tier denominator and rule 5 make the exemption list explicit product |
| **Nothing is externally anchored** — local main is ahead of origin, work uncommitted | The running binaries are the only copy of some behavior | Commit and push before building on top; a plan on an unanchored tree is a plan on sand |
| **WebMCP is a W3C incubation**, subject to change | Bridge could need rework | Flag-gated, ~a day of wiring, zero coupling into the registry itself |

---

## Appendix — numbers behind this plan

- 33 actions in `src/shared/actions/action-registry.ts` (6 approval-required, 3 high-risk, 31 reversible)
- 183 mutating operations across 38 client modules under `src/shared/api`
- 14 unique tools in model-gateway `builtin_tool_defs()`
- `MODEL_EXECUTABLE_ACTION_IDS` empty by design
- 29 capabilities live, 2 attested (`code_runtime_probed` → available, `shell_sandbox_probed` → approval_required)
- `session_core` runs / threads / messages: all zero

**Companion documents:** Audit Ledger (67-item verification, 2026-08-25) · `Verevon-ai-first.md` (market strategy) · `GATEWAY_DEDUPLICATION_PLAN.md` (gateway authority cleanup).
