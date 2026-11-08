# Wave 3-5 execution control — Model Plane

Date: 2026-11-07. Branch: `model-plane` (merged `main` @ `2a776c837` fast-forward).
Controller: Crouton Alpha (this session). Switch policy: **build dark, flips later** (all switches default off, no live proof in cells).

## Cell model (3 agents per task)
| Role | Model | Job |
|---|---|---|
| Orchestrator | Opus 5.5 High (`anthropic/claude-opus-5-5#high`) | Slice plan, DoD gate, merge verdict, status update |
| Writer | Haiku 5.5 Max (`anthropic/claude-haiku-5-5#max`) | Code only, slice file list + unit tests, no deploy, no switch flips |
| Verifier | Sonnet 5.5 Medium (`anthropic/claude-sonnet-5-5#medium`) | Tier-A/B tests, hostile review, depth findings `D3-*` |

Rules per cell: same worktree checkout; writer commits `feat(mp-epXX): ...`; verifier does not edit source; orchestrator merges only on green verifier. AGENTS.md authority boundaries + ZDR propagation required.

## Start order (owner-chosen 2026-11-07)
1. EP-13 S1 — picker search + list price at `/v1/models` (switch `MODEL_GATEWAY_CATALOGUE_PRICES` off)
2. EP-14 S1 — settled-turn folding (048, FE-only)
3. EP-8 S1 — memory containment (exclude Space/agent_run threads from dreaming) — blocks personalization
4. EP-2 S1 — web reading upgrade (outline+chunk+reference id, `read_page`/`find_in_page`)
5. EP-6 phase-1 — Automations hub + Control-authorised run-now

Then remaining EP-1,3,4,5,7,9,10,11,12 (Wave 3), Wave-4 platform, Wave-5 F-t3-069..098 on demand.

## Status board
| Cell | Epic slice | Status | Evidence |
|---|---|---|---|
| C1 | EP-13 S1 | done 2026-11-07 (cc4566b70 PASS) | verifier PASS; D3-001 budget closed |
| C2 | EP-14 S1 | done 2026-11-07 (16673db40 model-plane; gap a480f985d on mp-ep14-s1-gap superseded) | 125 S1 tests; alt design archived |
| C3 | EP-8-S2 (D3-010) | done 2026-11-07 (345311759) | 21 learning_events green; LF fixed |
| C4 | EP-2 S1/S2/S3 hardening | done 2026-11-07 (8d7d38fbe PASS) | writer tool_loop diff; verifier 122+314 green; 3 zdr_posture fail cross-cell |
| C5 | EP-6 phase-1 gaps | done 2026-11-07 (49666f070 PASS) | go + 47 vitest green; AuthorizeFire+InsertFire |
| C6 | EP-1 takeover | dispatched 2026-11-08 (a) give-back caller | — |
| C7 | EP-3 S4 wiring (A) | dispatched 2026-11-08 | — |
| C8 | EP-4 inline+MCP Apps | done 2026-11-08 (c3ff2128b PASS) | sentinel null fix; 6/6 vitest |
| C9 | EP-5 deck pipeline | done 2026-11-08 (b8b720e19 PASS) | python 47 run; 1 skipped symlink |
| C10 | EP-7 goals/triggers | dispatched 2026-11-08 | — |
| C11 | EP-9 policy/egress | dispatched 2026-11-08 | — |
| C12 | EP-10 connectors | dispatched 2026-11-08 | — |
| C13 | EP-11 authored behaviours | dispatched 2026-11-08 | — |
| C14 | EP-12 sharing/API | dispatched 2026-11-08 | — |
