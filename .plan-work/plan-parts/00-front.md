# Model Plane and chat/agent build plan, 2026-10-14

**Status:** DRAFT FOR APPROVAL. Planning only: no product code, configuration or live-tree file was changed, nothing was committed or pushed. This document is the only artifact written into the repository.
**Plans for:** the Model Plane (Rust and Go) and Verevon's web chat and agent pages (`apps/Frontend Plane/verevonv3`), per the audit `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md` (read-only; copied from the scratch folder, SHA-256 identical).
**Base:** worktree fast-forwarded to commit `afb83928` (the audited HEAD; it was 61 commits behind). The audit also read the live tree `C:/dev/CoresSystem`, which has 324 uncommitted or untracked entries on top of that HEAD. Files that exist only as that uncommitted work are tagged `[live-only]` throughout; the first workstream slice (WS01-S0) lands them before anything depends on them.
**Sibling plans:** (a) Verevon Space, (b) Quarry v2 and browser. Shared pieces and proposed owners are in section 5; items marked "FLAG FOR THE USER TO RECONCILE" need a decision across the three planning sessions.
**How it was produced:** 6 common digests (Haiku 5.5 max, plus two Sonnet 5.5 overlap maps), 27 per-workstream code sweeps (Haiku 5.5 max), 27 file-level designs (Sonnet 5.5 high), plan compilation (Sonnet 5.5 high), then an adversarial review and a path/line check (see the review log at the end). Per the user's instruction during the run, no Opus and no Fable were used; two early overlap digests ran at Sonnet xhigh before that instruction arrived. Everything read from files was treated as data. Design inputs and sweeps are kept in `.plan-work/` (untracked).

## Executive summary

**What this plan is.** 27 workstreams (WS01-WS27) that resolve the audit's 22 Priority items plus the Wave 1 and Wave 2 Recommended items they depend on (F-t3-016, 025, 028, 029, 030, 032, 033, 039, 049, 063, 066, 067), each with a file-level change list, contracts, flags, tests, definition of done, evidence and effort; ~12 epics for Waves 3-5; 40 decisions (DEC-01..DEC-40); one dependency graph, proto-tag and migration-number reservation, hot-file matrix, merge trains and lane assignment.

**The shape of the work.**
1. **Phase 0 is landing, not building.** About 78 Model Plane entries and the ZDR/session-core wave exist only in the working tree. Nearly every design names WS01-S0 (eight atomic commits, tag `mp-baseline-0`) as a precondition. The taxonomy gate is red at HEAD today.
2. **Finish what is built but dark before building new.** Deploy and prove the audit outbox fix and the ZDR design (WS01), make MCP tools callable through the signed gate (WS05), put the run context on Do runs (WS02), connectors (WS23) and the browser grant half (WS21) on their already-built sides. Every slice carries a `[FINISH]` or `[BUILD]` tag (see the "Dominant type" column below and each workstream's slice table); WS06, WS10, WS17, WS19, WS22 and WS25 are mostly new.
3. **The spine is the durable loop.** WS02 (run context) then WS03 (checkpoint, resume, park) then WS13 (approvals v2), WS14 (ask_user, plan-continue), WS20 (scheduled Do runs), WS21. It runs as one serial queue by a fixed pair of engineers because it all edits `execution-core` `agent.rs`, `runtime_loop/mod.rs` and `execution.proto`.
4. **Decisions first.** 40 decisions; section 2.2 lists the slices that can start with no decision, and each register row says whether it blocks building or only specific slices. The ones that most change the build: loop host and plan-continue semantics (DEC for WS03/WS14), entitlement ownership (ADR for WS10), audit-chain trust anchor (WS06), unattended-authority contract (WS20), shared proto/scope vocabulary with the Space session (WS13/WS14/WS17), thread workspace vs the 2026-08-22 SandboxBackend deferral (WS22).
5. **Scale.** About 400-520 engineer-days for Wave 1 and 620-850 for Wave 2 (design day-ranges, excluding owner rulings, the external ZDR provider contract and the sibling sessions). With 8 engineers: Wave 1 in 14-19 weeks (spine-bound 10-11), Wave 2 overlapping, whole programme ~37-49 weeks; with 5 engineers ~60-80 weeks. Cutting scope shortens Wave 2 more than adding engineers.
6. **Honest corrections to the audit** found while designing (the code wins where they differ): e.g. the audit's "no proto change" for cause codes is false (`CreateApprovalRequest` has no context channel); durable send receipts do not exist for user messages (WS04 is larger than M); `agentic_run_stream` is not registered with the cancel registry at HEAD; the audit's gateway-pulls-entitlements design needs inference-core as the single Model Plane puller; the ratchet in F-t3-067 is suppression-based, not hash-based; CI has effectively never run on the integration branch. These are listed per workstream under "Verified starting point".
7. **The first three pull requests** (section 7b): PR-1 land the live baseline; PR-2 make CI and the `buf breaking` gate real; PR-3 the run-context proto contract on `RunAgentRequest` (tag 18, 27 literal sites) with no behaviour change.

**What I need from you:** approve or adjust the plan, answer the ranked questions in section 7a (the first five unblock the most work), and reconcile the flagged cross-session items in section 5.

**Reading order.** 1 Goal and criteria -> 2 Decisions -> 3 Workstreams (3.0 index and finish-vs-build split, 3.1 Wave 1, 3.2 Wave 2, 3.3 Waves 3-5 epics) -> 4 Sequencing -> 5 Cross-session interfaces -> 6 Verification and regression -> 7 Open questions and first three PRs.

### Workstream index

| WS | Audit items | Wave | Owner plane / service | Dominant type |
|---|---|---|---|---|
| WS01 Ops closure (land baseline, ZDR attestation and proof, audit outbox deploy, release safety) | F-t3-005, 004 (phase 0), 039 | W0/W1 | Model Plane + Control Plane (ops) | FINISH |
| WS02 Run context + prompt-cache | F-t3-001, 022 | W1 | model-gateway, execution-core, inference-core, cost-core | FINISH + BUILD |
| WS03 Durable loop | F-t3-002 | W1 | execution-core, session-core | BUILD |
| WS04 Replica-safe run control | F-t3-016 | W1 | model-gateway, session-core | FINISH + BUILD |
| WS05 Declared tool classes + MCP | F-t3-003 | W1 | capability-core, execution-core, model-gateway | FINISH + BUILD |
| WS06 Audit hash chain, compliance API, SIEM | F-t3-004 (rest) | W1 | Control Plane audit-core, org-core | BUILD |
| WS07 Memory controls + learning governance | F-t3-020, 007 | W1 | session-core, capability-core, Control, Frontend | FINISH + BUILD |
| WS08 Attachments v2 | F-t3-017 | W1 | model-gateway, execution-core, Frontend | BUILD |
| WS09 Transcript performance | F-t3-021 | W1 | Frontend (+ session-core paging) | BUILD |
| WS10 Model entitlements | F-t3-023 | W1 | org-core, inference-core, Frontend | BUILD |
| WS11 Effort ladder | F-t3-029 | W1 | inference-core | FINISH + BUILD |
| WS12 Frontend CI gates + contract guardrails | F-t3-066, 067 | W0/W1 | Frontend + CI | BUILD |
| WS13 Approvals v2 | F-t3-006 | W2 | session-core, model-gateway, Frontend, Control (step-up) | BUILD |
| WS14 Finish human-in-the-loop | F-t3-011 | W2 | execution-core, session-core, Frontend | BUILD |
| WS15 Typed lifecycle events | F-t3-049 | W2 | model-gateway, Frontend | BUILD |
| WS16 Attention model | F-t3-015 | W2 | session-core, notification-core, Frontend | BUILD |
| WS17 History foundation | F-t3-012 | W2 | session-core, Frontend | BUILD |
| WS18 Spaces as the chat project | F-t3-013 | W2 | Application Plane, model-gateway, Frontend | BUILD |
| WS19 Verified Office deliverables | F-t3-014 | W2 | execution-core, capability-core, Frontend | BUILD |
| WS20 Scheduled governed Do runs | F-t3-018 | W2 | orchestrator-core, capability-core, Control | BUILD |
| WS21 Agent browser on real grants | F-t3-008 | W2 | browser-broker, model-gateway, Control (+ Quarry session) | FINISH + BUILD |
| WS22 Per-thread sandbox computer | F-t3-009 | W2 | sandbox-manager, execution-core | BUILD |
| WS23 Connected apps in chat | F-t3-010 | W2 | execution-core, integration-corev2, model-gateway | FINISH + BUILD |
| WS24 Do-mode action admission + file tools | F-t3-025, 030 | W2 | capability-core, execution-core, Frontend | BUILD |
| WS25 Org agent policy registry, loop guard, hooks | F-t3-032, 063 | W2 | Control, capability-core, model-gateway | BUILD |
| WS26 Content-safety hardening | F-t3-033 | W2 | inference-core, model-gateway, session-core | BUILD |
| WS27 Live cost meter | F-t3-028 | W2 | cost-core, model-gateway, Frontend | BUILD |

---
